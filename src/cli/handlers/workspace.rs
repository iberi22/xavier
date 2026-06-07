//! Workspace handlers for information and MCP tool integration.

use axum::{extract::State, Json};

use crate::cli::state::CliState;

pub async fn workspace_info_handler(
    State(state): State<CliState>,
) -> impl axum::response::IntoResponse {
    Json(serde_json::json!({
        "workspace_id": state.workspace_id,
        "workspace_dir": state.workspace_dir.to_string_lossy(),
    }))
}

pub async fn mcp_tools_handler() -> impl axum::response::IntoResponse {
    Json(serde_json::json!({
        "tools": [
            {"name": "memory_search", "description": "Search memory with semantic + lexical hybrid search"},
            {"name": "memory_add", "description": "Add a new memory entry with metadata and zone tagging"},
            {"name": "memory_delete", "description": "Delete a memory entry by path"},
            {"name": "memory_stats", "description": "Get memory statistics and counts"},
            {"name": "memory_export", "description": "Export all memories as JSON"},
            {"name": "code_scan", "description": "Scan a codebase and index symbols into the code graph"},
            {"name": "code_find", "description": "Find code symbols by name, kind, or file path"},
            {"name": "code_context", "description": "Get surrounding context for a code symbol"},
            {"name": "code_stats", "description": "Get code graph statistics"},
            {"name": "agent_register", "description": "Register a new AI agent"},
            {"name": "agent_list", "description": "List active agents"},
            {"name": "agent_heartbeat", "description": "Send heartbeat for an agent"},
            {"name": "agent_push_context", "description": "Push context document to an agent"},
            {"name": "agent_unregister", "description": "Unregister an agent"},
        ]
    }))
}
