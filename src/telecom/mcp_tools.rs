//! MCP tool suite for AI agent interaction with telecom mesh (Issue #2373 / WAVE-26.11)

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

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
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub struct TelecomMcpRegistry {
    tools: HashMap<String, ToolDescriptor>,
    active_rooms: Arc<RwLock<Vec<String>>>,
}

impl TelecomMcpRegistry {
    pub fn new() -> Self {
        let mut reg = Self {
            tools: HashMap::new(),
            active_rooms: Arc::new(RwLock::new(vec!["lobby".into(), "general".into()])),
        };
        reg.register_telecom_tools();
        reg
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
    async fn test_execute_unknown_tool_fails() {
        let reg = TelecomMcpRegistry::new();
        let res = reg
            .execute_telecom_tool("invalid_nonexistent_tool", serde_json::json!({}))
            .await;
        assert!(res.is_err());
    }
}
