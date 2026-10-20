//! MCP Tools Extension for Telecom Messaging and Room Management (#2395 / WAVE-27.17)

use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::server::mcp::types::{MCPTool, MCPToolResult};
use crate::telecom::mcp_tools::TelecomMcpRegistry;

/// Returns the expanded suite of Telecom MCP Tools.
pub fn register_telecom_mcp_tools() -> Vec<MCPTool> {
    vec![
        MCPTool {
            name: "telecom_send_packet".to_string(),
            description:
                "Store a text message for a telecom room on this node. Payloads are saved as plain text locally; no end-to-end encryption is applied on this path"
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
    registry: Option<Arc<RwLock<TelecomMcpRegistry>>>,
) -> anyhow::Result<Value> {
    let registry = registry.ok_or_else(|| anyhow::anyhow!("telecom backend unavailable"))?;
    let res = registry
        .read()
        .await
        .execute_telecom_tool(name, arguments)
        .await?;
    Ok(serde_json::to_value(MCPToolResult::structured(res, false))?)
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
    async fn test_send_packet_without_backend_returns_error() {
        let send_args = json!({
            "room_id": "test-room",
            "payload": "encrypted-msg",
            "recipient": "node-b"
        });
        let res = handle_telecom_tool("telecom_send_packet", send_args, None).await;
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().to_string(), "telecom backend unavailable");
    }

    #[tokio::test]
    async fn test_list_peers_without_backend_returns_error() {
        let res = handle_telecom_tool("telecom_list_peers", json!({}), None).await;
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().to_string(), "telecom backend unavailable");
    }

    #[tokio::test]
    async fn test_create_group_accepts_call_without_initial_members() {
        let registry = Arc::new(RwLock::new(TelecomMcpRegistry::new()));
        let group_args = json!({
            "group_name": "Ops War Room",
            "clearance": "Confidential"
        });
        let group_res =
            handle_telecom_tool("telecom_create_group", group_args, Some(registry.clone()))
                .await
                .unwrap();
        assert!(group_res.get("structuredContent").is_some());

        let rooms = registry
            .read()
            .await
            .execute_telecom_tool("telecom_list_rooms", json!({}))
            .await
            .unwrap();
        assert!(rooms["rooms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r == "Ops War Room"));
    }
}
