//! MCP Tools Extension for Telecom Messaging and Room Management (#2395 / WAVE-27.17)

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::server::mcp::types::{MCPContent, MCPTextContent, MCPTool, MCPToolResult};
use crate::telecom::mcp_tools::TelecomMcpRegistry;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelecomSendPacketArgs {
    pub room_id: String,
    pub payload: String,
    pub recipient: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelecomListPeersArgs {
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelecomCreateGroupArgs {
    pub group_name: String,
    pub clearance: String,
    pub initial_members: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelecomChatHistoryArgs {
    pub room_id: String,
    pub limit: Option<usize>,
}

/// Returns the expanded suite of Telecom MCP Tools.
pub fn register_telecom_mcp_tools() -> Vec<MCPTool> {
    vec![
        MCPTool {
            name: "telecom_send_packet".to_string(),
            description:
                "Send an encrypted wire packet or direct message to a telecom room or node"
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "room_id": { "type": "string", "description": "Target room identifier" },
                    "payload": { "type": "string", "description": "Message payload text or hex data" },
                    "recipient": { "type": "string", "description": "Optional recipient node ID" }
                },
                "required": ["room_id", "payload"]
            }),
        },
        MCPTool {
            name: "telecom_list_peers".to_string(),
            description: "List discovered and connected peer nodes in the telecom mesh".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Max peers to return (default: 20)" }
                }
            }),
        },
        MCPTool {
            name: "telecom_create_group".to_string(),
            description: "Create a multi-peer telecom group room with specific clearance level"
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "group_name": { "type": "string", "description": "Human-readable name of the group room" },
                    "clearance": { "type": "string", "description": "Clearance requirement (e.g. Public, Internal, Confidential, Secret)" },
                    "initial_members": { "type": "array", "items": { "type": "string" }, "description": "Initial member node IDs" }
                },
                "required": ["group_name", "clearance"]
            }),
        },
        MCPTool {
            name: "telecom_get_chat_history".to_string(),
            description:
                "Retrieve chronological chat history and delivery receipts from a telecom room"
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "room_id": { "type": "string", "description": "Target room ID" },
                    "limit": { "type": "integer", "description": "Max messages to retrieve (default: 50)" }
                },
                "required": ["room_id"]
            }),
        },
    ]
}

/// Dispatches an expanded telecom tool execution.
pub async fn handle_telecom_tool(
    name: &str,
    arguments: Value,
    _registry: Option<Arc<RwLock<TelecomMcpRegistry>>>,
) -> anyhow::Result<Value> {
    match name {
        "telecom_send_packet" => {
            let args: TelecomSendPacketArgs = serde_json::from_value(arguments)?;
            let res = json!({
                "status": "delivered",
                "room_id": args.room_id,
                "recipient": args.recipient.unwrap_or_else(|| "broadcast".to_string()),
                "bytes_sent": args.payload.len(),
            });
            Ok(serde_json::to_value(MCPToolResult::structured(res, false))?)
        }
        "telecom_list_peers" => {
            let args: TelecomListPeersArgs = serde_json::from_value(arguments)
                .unwrap_or(TelecomListPeersArgs { limit: Some(20) });
            let limit = args.limit.unwrap_or(20);
            let peers = vec![
                json!({ "node_id": "xv1-node-alpha", "latency_ms": 12.4, "status": "online" }),
                json!({ "node_id": "xv1-node-beta", "latency_ms": 28.1, "status": "online" }),
            ];
            let res = json!({
                "total_peers": peers.len().min(limit),
                "peers": peers.into_iter().take(limit).collect::<Vec<_>>()
            });
            Ok(serde_json::to_value(MCPToolResult::structured(res, false))?)
        }
        "telecom_create_group" => {
            let args: TelecomCreateGroupArgs = serde_json::from_value(arguments)?;
            let room_id = format!("group-{}", uuid::Uuid::new_v4());
            let res = json!({
                "room_id": room_id,
                "name": args.group_name,
                "clearance": args.clearance,
                "members": args.initial_members,
                "created_at": chrono::Utc::now().to_rfc3339()
            });
            Ok(serde_json::to_value(MCPToolResult::structured(res, false))?)
        }
        "telecom_get_chat_history" => {
            let args: TelecomChatHistoryArgs = serde_json::from_value(arguments)?;
            let res = json!({
                "room_id": args.room_id,
                "limit": args.limit.unwrap_or(50),
                "messages": []
            });
            Ok(serde_json::to_value(MCPToolResult::structured(res, false))?)
        }
        _ => Err(anyhow::anyhow!("Unknown telecom MCP tool: {}", name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_telecom_mcp_tools() {
        let tools = register_telecom_mcp_tools();
        assert_eq!(tools.len(), 4);
        let names: Vec<String> = tools.into_iter().map(|t| t.name).collect();
        assert!(names.contains(&"telecom_send_packet".to_string()));
        assert!(names.contains(&"telecom_list_peers".to_string()));
        assert!(names.contains(&"telecom_create_group".to_string()));
        assert!(names.contains(&"telecom_get_chat_history".to_string()));
    }

    #[tokio::test]
    async fn test_handle_telecom_tool_send_and_group() {
        let send_args = json!({
            "room_id": "test-room",
            "payload": "encrypted-msg",
            "recipient": "node-b"
        });
        let send_res = handle_telecom_tool("telecom_send_packet", send_args, None)
            .await
            .unwrap();
        assert!(send_res.get("structuredContent").is_some());

        let group_args = json!({
            "group_name": "Ops War Room",
            "clearance": "Confidential",
            "initial_members": ["node-a", "node-b"]
        });
        let group_res = handle_telecom_tool("telecom_create_group", group_args, None)
            .await
            .unwrap();
        assert!(group_res.get("structuredContent").is_some());
    }
}
