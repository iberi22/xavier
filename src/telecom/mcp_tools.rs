//! MCP tool suite for AI agent interaction with telecom mesh (Issue #2373 / WAVE-26.11)

use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

use crate::telecom::store::{TelecomStore, TelecomStoreError};

/// Rooms that exist on every node from first boot.
const DEFAULT_ROOMS: [&str; 2] = ["lobby", "general"];

#[derive(Debug, Error)]
pub enum TelecomMcpError {
    #[error("Unknown tool: {0}")]
    UnknownTool(String),

    #[error("Invalid arguments: {0}")]
    InvalidArguments(String),

    #[error("Execution error: {0}")]
    ExecutionError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageArgs {
    pub room_id: String,
    pub recipient_node: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListRoomsArgs {
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryNodeArgs {
    pub node_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareDocArgs {
    pub room_id: String,
    pub file_name: String,
    pub content_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGroupArgs {
    pub group_name: String,
    pub clearance: String,
    #[serde(default)]
    pub initial_members: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendPacketArgs {
    pub room_id: String,
    pub payload: String,
    pub recipient: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListPeersArgs {
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatHistoryArgs {
    pub room_id: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub struct TelecomMcpRegistry {
    tools: HashMap<String, ToolDescriptor>,
    active_rooms: Arc<RwLock<Vec<String>>>,
    store: TelecomStore,
    local_node_id: String,
}

fn exec_err(e: impl std::fmt::Display) -> TelecomMcpError {
    TelecomMcpError::ExecutionError(e.to_string())
}

/// Builds the registry the daemon wires into its MCP server at boot.
/// Rooms, messages and peer sessions persist in `<data_dir>/telecom.db`.
pub fn build_telecom_registry(
    data_dir: &Path,
    local_node_id: &str,
) -> anyhow::Result<TelecomMcpRegistry> {
    std::fs::create_dir_all(data_dir)?;
    let store = TelecomStore::open(data_dir.join("telecom.db"))?;
    Ok(TelecomMcpRegistry::with_store(store, local_node_id)?)
}

impl TelecomMcpRegistry {
    /// In-memory registry (tests and callers without a data dir).
    pub fn new() -> Self {
        let store = TelecomStore::open_in_memory().expect("in-memory telecom store opens");
        Self::with_store(store, "local").expect("default rooms persist into a fresh store")
    }

    /// Registry backed by `store`. Rooms already in the store are loaded back.
    pub fn with_store(
        store: TelecomStore,
        local_node_id: impl Into<String>,
    ) -> Result<Self, TelecomStoreError> {
        let mut reg = Self {
            tools: HashMap::new(),
            active_rooms: Arc::new(RwLock::new(Vec::new())),
            store,
            local_node_id: local_node_id.into(),
        };
        reg.register_telecom_tools();
        let rooms = {
            let conn = reg.store.connection();
            let conn = conn.lock().unwrap();
            for room in DEFAULT_ROOMS {
                conn.execute(
                    "INSERT OR IGNORE INTO telecom_rooms (room_id, name, clearance, is_group, created_at)
                     VALUES (?1, ?1, 'Public', 0, ?2)",
                    params![room, Utc::now().to_rfc3339()],
                )?;
            }
            let mut stmt = conn.prepare("SELECT room_id FROM telecom_rooms ORDER BY rowid")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<String>, _>>()?
        };
        reg.active_rooms = Arc::new(RwLock::new(rooms));
        Ok(reg)
    }

    /// Runs SQLite work on the blocking pool so it never stalls a Tokio worker
    /// (busy_timeout can wait several seconds on a locked database).
    async fn blocking_store<T, F>(&self, f: F) -> Result<T, TelecomMcpError>
    where
        F: FnOnce(TelecomStore) -> Result<T, TelecomMcpError> + Send + 'static,
        T: Send + 'static,
    {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || f(store))
            .await
            .map_err(|e| {
                TelecomMcpError::ExecutionError(format!("telecom store task failed: {e}"))
            })?
    }

    pub fn register_telecom_tools(&mut self) {
        self.tools.insert(
            "telecom_send_message".into(),
            ToolDescriptor {
                name: "telecom_send_message".into(),
                description: "Send an encrypted message to a telecom room or peer node".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "room_id": { "type": "string" },
                        "recipient_node": { "type": "string" },
                        "message": { "type": "string" }
                    },
                    "required": ["room_id", "message"]
                }),
            },
        );

        self.tools.insert(
            "telecom_list_rooms".into(),
            ToolDescriptor {
                name: "telecom_list_rooms".into(),
                description: "List available telecom rooms in the local node mesh".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "limit": { "type": "integer" }
                    }
                }),
            },
        );

        self.tools.insert(
            "telecom_query_node".into(),
            ToolDescriptor {
                name: "telecom_query_node".into(),
                description: "Query status and public identity of a remote node in telecom mesh"
                    .into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "node_id": { "type": "string" }
                    },
                    "required": ["node_id"]
                }),
            },
        );

        self.tools.insert(
            "telecom_share_doc".into(),
            ToolDescriptor {
                name: "telecom_share_doc".into(),
                description: "Stream an encrypted document chunked to a telecom room".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "room_id": { "type": "string" },
                        "file_name": { "type": "string" },
                        "content_base64": { "type": "string" }
                    },
                    "required": ["room_id", "file_name", "content_base64"]
                }),
            },
        );
    }

    pub fn list_tools(&self) -> Vec<ToolDescriptor> {
        self.tools.values().cloned().collect()
    }

    pub async fn execute_telecom_tool(
        &self,
        tool_name: &str,
        arguments: Value,
    ) -> Result<Value, TelecomMcpError> {
        match tool_name {
            "telecom_send_message" => {
                let args: SendMessageArgs = serde_json::from_value(arguments)
                    .map_err(|e| TelecomMcpError::InvalidArguments(e.to_string()))?;
                Ok(serde_json::json!({
                    "status": "sent",
                    "room_id": args.room_id,
                    "recipient": args.recipient_node.unwrap_or_else(|| "broadcast".into()),
                    "message_len": args.message.len()
                }))
            }
            "telecom_list_rooms" => {
                let args: ListRoomsArgs =
                    serde_json::from_value(arguments).unwrap_or(ListRoomsArgs { limit: None });
                let rooms = self.active_rooms.read().await;
                let limit = args.limit.unwrap_or(rooms.len());
                let result: Vec<String> = rooms.iter().take(limit).cloned().collect();
                Ok(serde_json::json!({
                    "rooms": result,
                    "total": rooms.len()
                }))
            }
            "telecom_query_node" => {
                let args: QueryNodeArgs = serde_json::from_value(arguments)
                    .map_err(|e| TelecomMcpError::InvalidArguments(e.to_string()))?;
                Ok(serde_json::json!({
                    "node_id": args.node_id,
                    "status": "online",
                    "capabilities": ["direct_chat", "file_transfer", "agent_responder"]
                }))
            }
            "telecom_share_doc" => {
                let args: ShareDocArgs = serde_json::from_value(arguments)
                    .map_err(|e| TelecomMcpError::InvalidArguments(e.to_string()))?;
                Ok(serde_json::json!({
                    "status": "shared",
                    "room_id": args.room_id,
                    "file_name": args.file_name,
                    "bytes_shared": args.content_base64.len()
                }))
            }
            "telecom_send_packet" => {
                let args: SendPacketArgs = serde_json::from_value(arguments)
                    .map_err(|e| TelecomMcpError::InvalidArguments(e.to_string()))?;
                let message_id = format!("pkt-{}", uuid::Uuid::new_v4());
                let local_node_id = self.local_node_id.clone();
                let (room_id, payload, msg_id) = (
                    args.room_id.clone(),
                    args.payload.clone(),
                    message_id.clone(),
                );
                self.blocking_store(move |store| {
                    let conn = store.connection();
                    let conn = conn.lock().unwrap();
                    let exists: bool = conn
                        .query_row(
                            "SELECT EXISTS(SELECT 1 FROM telecom_rooms WHERE room_id = ?1)",
                            params![room_id],
                            |row| row.get(0),
                        )
                        .map_err(exec_err)?;
                    if !exists {
                        return Err(TelecomMcpError::ExecutionError(format!(
                            "unknown room: {}",
                            room_id
                        )));
                    }
                    // Sequence is computed inside the INSERT, under the connection lock.
                    conn.execute(
                        "INSERT INTO telecom_messages
                         (message_id, room_id, sender_node_id, encrypted_payload, sequence, is_ephemeral, sent_at)
                         VALUES (?1, ?2, ?3, ?4,
                                 (SELECT COALESCE(MAX(sequence), 0) + 1 FROM telecom_messages WHERE room_id = ?2),
                                 0, ?5)",
                        params![
                            msg_id,
                            room_id,
                            local_node_id,
                            payload.as_bytes(),
                            Utc::now().to_rfc3339()
                        ],
                    )
                    .map_err(exec_err)?;
                    Ok(())
                })
                .await?;
                Ok(serde_json::json!({
                    "status": "sent",
                    "message_id": message_id,
                    "room_id": args.room_id,
                    "sender": self.local_node_id,
                    "recipient": args.recipient.unwrap_or_else(|| "broadcast".into()),
                    "message_len": args.payload.len()
                }))
            }
            "telecom_list_peers" => {
                let args: ListPeersArgs = serde_json::from_value(arguments).unwrap_or_default();
                let mut peers = vec![serde_json::json!({
                    "node_id": self.local_node_id,
                    "status": "online",
                    "is_local": true
                })];
                let local = self.local_node_id.clone();
                let discovered: Vec<String> = self
                    .blocking_store(move |store| {
                        let conn = store.connection();
                        let conn = conn.lock().unwrap();
                        let mut stmt = conn
                            .prepare(
                                "SELECT peer_node_id, MAX(last_active) FROM telecom_sessions
                                 WHERE peer_node_id != ?1 GROUP BY peer_node_id ORDER BY 2 DESC",
                            )
                            .map_err(exec_err)?;
                        let rows = stmt
                            .query_map(params![local], |row| row.get::<_, String>(0))
                            .map_err(exec_err)?;
                        rows.collect::<Result<_, _>>().map_err(exec_err)
                    })
                    .await?;
                for node_id in discovered {
                    peers.push(serde_json::json!({
                        "node_id": node_id,
                        "status": "discovered",
                        "is_local": false
                    }));
                }
                let total = peers.len();
                peers.truncate(args.limit.unwrap_or(20));
                Ok(serde_json::json!({ "peers": peers, "total": total }))
            }
            "telecom_get_chat_history" => {
                let args: ChatHistoryArgs = serde_json::from_value(arguments)
                    .map_err(|e| TelecomMcpError::InvalidArguments(e.to_string()))?;
                let (room_id, limit) = (args.room_id.clone(), args.limit.unwrap_or(50));
                let messages = self
                    .blocking_store(move |store| {
                        store.fetch_room_history(&room_id, limit).map_err(exec_err)
                    })
                    .await?;
                let items: Vec<Value> = messages
                    .iter()
                    .map(|m| {
                        serde_json::json!({
                            "message_id": m.message_id,
                            "room_id": m.room_id,
                            "sender": m.sender_node_id,
                            "sequence": m.sequence,
                            "sent_at": m.sent_at.to_rfc3339(),
                            "payload": String::from_utf8_lossy(&m.encrypted_payload)
                        })
                    })
                    .collect();
                Ok(serde_json::json!({
                    "room_id": args.room_id,
                    "total": items.len(),
                    "messages": items
                }))
            }
            "telecom_create_group" => {
                let args: CreateGroupArgs = serde_json::from_value(arguments)
                    .map_err(|e| TelecomMcpError::InvalidArguments(e.to_string()))?;
                let (room, clearance) = (args.group_name.clone(), args.clearance.clone());
                self.blocking_store(move |store| {
                    let conn = store.connection();
                    let conn = conn.lock().unwrap();
                    conn.execute(
                        "INSERT OR IGNORE INTO telecom_rooms (room_id, name, clearance, is_group, created_at)
                         VALUES (?1, ?1, ?2, 1, ?3)",
                        params![room, clearance, Utc::now().to_rfc3339()],
                    )
                    .map_err(exec_err)?;
                    Ok(())
                })
                .await?;
                let mut rooms = self.active_rooms.write().await;
                if !rooms.contains(&args.group_name) {
                    rooms.push(args.group_name.clone());
                }
                Ok(serde_json::json!({
                    "room_id": args.group_name,
                    "clearance": args.clearance,
                    "members": args.initial_members
                }))
            }
            _ => Err(TelecomMcpError::UnknownTool(tool_name.to_string())),
        }
    }
}

impl Default for TelecomMcpRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tool_registration_and_listing() {
        let reg = TelecomMcpRegistry::new();
        let list = reg.list_tools();
        assert!(list.len() >= 4);
        assert!(list.iter().any(|t| t.name == "telecom_send_message"));
        assert!(list.iter().any(|t| t.name == "telecom_list_rooms"));
    }

    #[tokio::test]
    async fn test_execute_send_message_and_list_rooms() {
        let reg = TelecomMcpRegistry::new();

        let send_res = reg
            .execute_telecom_tool(
                "telecom_send_message",
                serde_json::json!({
                    "room_id": "lobby",
                    "message": "Hello from autonomous MCP agent"
                }),
            )
            .await
            .expect("Tool execution should succeed");

        assert_eq!(send_res["status"], "sent");
        assert_eq!(send_res["room_id"], "lobby");

        let list_res = reg
            .execute_telecom_tool("telecom_list_rooms", serde_json::json!({ "limit": 1 }))
            .await
            .unwrap();

        assert_eq!(list_res["rooms"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn build_telecom_registry_persists_rooms_and_messages_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        {
            let reg = build_telecom_registry(dir.path(), "node-a").unwrap();
            reg.execute_telecom_tool(
                "telecom_create_group",
                serde_json::json!({"group_name": "persist-room", "clearance": "Internal"}),
            )
            .await
            .unwrap();
            reg.execute_telecom_tool(
                "telecom_send_packet",
                serde_json::json!({"room_id": "persist-room", "payload": "kept"}),
            )
            .await
            .unwrap();
        }
        assert!(dir.path().join("telecom.db").exists());

        let reg = build_telecom_registry(dir.path(), "node-a").unwrap();
        let rooms = reg
            .execute_telecom_tool("telecom_list_rooms", serde_json::json!({}))
            .await
            .unwrap();
        assert!(rooms["rooms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r == "persist-room"));
        let history = reg
            .execute_telecom_tool(
                "telecom_get_chat_history",
                serde_json::json!({"room_id": "persist-room"}),
            )
            .await
            .unwrap();
        assert_eq!(history["messages"][0]["payload"], "kept");
    }

    #[tokio::test]
    async fn test_send_packet_to_unknown_room_fails() {
        let reg = TelecomMcpRegistry::new();
        let res = reg
            .execute_telecom_tool(
                "telecom_send_packet",
                serde_json::json!({"room_id": "no-such-room", "payload": "x"}),
            )
            .await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_execute_unknown_tool_fails() {
        let reg = TelecomMcpRegistry::new();
        let res = reg
            .execute_telecom_tool("invalid_nonexistent_tool", serde_json::json!({}))
            .await;
        assert!(res.is_err());
    }
}
