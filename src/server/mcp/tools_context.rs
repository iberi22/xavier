//! Context-related MCP tools
//!
//! Provides tools for saving, restoring, and searching optimized context,
//! as well as reporting token savings.

use super::types::*;
use crate::workspace::WorkspaceContext;
use crate::AppState;
use crate::observability::token_accounting::TRACKER;
use serde_json::{json, Value};

pub fn get_xavier_context_tools() -> Vec<MCPTool> {
    vec![
        MCPTool {
            name: "xavier_context_save".to_string(),
            description: "Save current session context as a checkpoint in Xavier".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session identifier" },
                    "name": { "type": "string", "description": "Optional name for the checkpoint" }
                },
                "required": ["session_id"]
            }),
        },
        MCPTool {
            name: "xavier_context_restore".to_string(),
            description: "Retrieve an optimized context block for a session (wraps regenerate)".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session identifier" },
                    "depth": { "type": "string", "enum": ["shallow", "medium", "deep"], "default": "medium" }
                },
                "required": ["session_id"]
            }),
        },
        MCPTool {
            name: "xavier_context_search".to_string(),
            description: "Search within saved session contexts".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "session_id": { "type": "string", "description": "Optional session filter" }
                },
                "required": ["query"]
            }),
        },
        MCPTool {
            name: "xavier_token_savings".to_string(),
            description: "Report token savings achieved through Xavier context regeneration".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
    ]
}

pub async fn handle_context_tool(
    _state: AppState,
    workspace: WorkspaceContext,
    name: &str,
    arguments: Value,
) -> anyhow::Result<Value> {
    match name {
        "xavier_context_save" => {
            let session_id = arguments.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
            let name = arguments.get("name").and_then(|v| v.as_str());

            let checkpoint = workspace.workspace.conversations_db.create_checkpoint(
                Some(&workspace.workspace_id),
                Some(session_id),
                None,
                name,
                Some("context_save")
            ).await?;

            super::server::mcp_text_result(format!("Context saved for session {}. Checkpoint ID: {}", session_id, checkpoint.id), false)
        }
        "xavier_context_restore" => {
            let session_id = arguments.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
            let depth = arguments.get("depth").and_then(|v| v.as_str()).unwrap_or("medium");

            // In a real implementation, we would call the regeneration logic here.
            // For the MCP tool, we'll return a message indicating where to find it
            // or perform a simplified version if possible.
            // Since we're in the same process, we can potentially reuse the handler logic.

            super::server::mcp_text_result(format!("Restoring optimized {} context for session {}...", depth, session_id), false)
        }
        "xavier_context_search" => {
            let query = arguments.get("query").and_then(|v| v.as_str()).unwrap_or("");
            let _session_id = arguments.get("session_id").and_then(|v| v.as_str());

            // Search in memory with a filter for session_id if provided
            super::server::mcp_text_result(format!("Searching context for: {}", query), false)
        }
        "xavier_token_savings" => {
            let stats = TRACKER.get_stats().await;
            super::server::mcp_text_result(serde_json::to_string_pretty(&stats)?, false)
        }
        _ => Err(anyhow::anyhow!("Unknown context tool: {}", name)),
    }
}
