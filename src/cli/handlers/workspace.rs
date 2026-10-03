//! Workspace handlers for information and MCP tool integration.

use axum::http::StatusCode;
use axum::{extract::State, response::IntoResponse, Extension, Json};
use std::sync::Arc;
use xavier::workspace::WorkspaceContext;

use crate::cli::state::CliState;

#[derive(Debug, serde::Deserialize)]
pub struct McpToolCallPayload {
    pub name: String,
    pub arguments: serde_json::Value,
}

/// MCP tools a space token may call: only those that act on the request's
/// `WorkspaceContext` alone (audited in `tools_memory`). Search and get need
/// Read; add needs Write. Every other tool is refused.
fn space_tool_action(name: &str) -> Option<xavier::espacio::SpaceAction> {
    use xavier::espacio::SpaceAction;
    match name {
        "mem_search" | "search_memory" | "memory_search" | "get_memory" => Some(SpaceAction::Read),
        "create_memory" | "memory_save" => Some(SpaceAction::Write),
        _ => None,
    }
}

#[allow(clippy::result_large_err)]
fn space_tool_gate(
    space: &xavier::espacio::tokens::SpaceContext,
    workspace: &WorkspaceContext,
    tool: &str,
) -> Result<(), axum::response::Response> {
    let forbidden = || {
        (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"status":"error","message":"Forbidden"})),
        )
            .into_response()
    };
    // The workspace must be this token's own space, never the default one.
    if workspace.workspace_id != space.space_id {
        return Err(forbidden());
    }
    match space_tool_action(tool) {
        Some(action) if xavier::espacio::can(space.role, action) => Ok(()),
        _ => Err(forbidden()),
    }
}

/// Handler for POST /mcp/tools/call
pub async fn mcp_tools_call_handler(
    State(state): State<CliState>,
    Extension(workspace): Extension<WorkspaceContext>,
    claims: Option<Extension<xavier::security::auth::Claims>>,
    root: Option<Extension<xavier::adapters::inbound::http::routes::RootCredential>>,
    space: Option<Extension<xavier::espacio::tokens::SpaceContext>>,
    requester: Option<Extension<xavier::security::clearance::ClearanceLevel>>,
    Json(payload): Json<McpToolCallPayload>,
) -> axum::response::Response {
    use xavier::server::mcp::server::{handle_tool_call, with_root_credential};

    // Space tokens: the workspace is the token's own space (inserted by the
    // auth middleware) and only space-scoped memory tools are reachable.
    if let Some(Extension(space)) = &space {
        if let Err(resp) = space_tool_gate(space, &workspace, &payload.name) {
            return resp;
        }
    }

    // Convert CliState to AppState
    let workspace_registry = Arc::new(xavier::workspace::WorkspaceRegistry::new());
    let app_state = xavier::AppState {
        workspace_registry,
        code_indexer: state.code_graph.read().await.indexer.clone(),
        code_query: state.code_graph.read().await.query.clone(),
        code_db: state.code_graph.read().await.db.clone(),
        indexer: xavier::memory::file_indexer::FileIndexer::new(
            xavier::memory::file_indexer::FileIndexerConfig::default(),
            Some(state.code_graph.read().await.indexer.clone()),
        ),
        agent_indexer: xavier::memory::agent_indexer::AgentIndexer::new(
            xavier::memory::file_indexer::FileIndexer::new(
                xavier::memory::file_indexer::FileIndexerConfig::default(),
                Some(state.code_graph.read().await.indexer.clone()),
            ),
        ),
        security_service: Arc::new(xavier::app::security_service::SecurityService::new()),
        code_graph_dump_path: None,
    };

    let claims_ref = claims.as_ref().map(|c| &c.0);
    // Caller identity for tools that cross spaces (linked search): the space
    // token's SpaceAuth and the same clearance ceiling REST applies.
    let caller = xavier::server::mcp::tools_memory::McpCaller {
        space: space.as_ref().map(|e| e.0.clone()),
        clearance: requester
            .map(|Extension(level)| level)
            .unwrap_or_else(xavier::security::clearance::default_clearance),
    };
    match xavier::server::mcp::tools_memory::with_mcp_caller(
        caller,
        with_root_credential(
            root.is_some() && space.is_none(),
            handle_tool_call(
                app_state,
                workspace,
                claims_ref,
                &payload.name,
                payload.arguments,
            ),
        ),
    )
    .await
    {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(e) => {
            let message = e.to_string();
            let code = if message.contains("Security policy violation")
                || message.contains("blocked by security policy")
            {
                xavier::server::mcp::types::XAVIER_ERROR_SECURITY
            } else if message.contains("Missing")
                || message.contains("must be")
                || message.contains("Invalid")
            {
                xavier::server::mcp::types::XAVIER_ERROR_VALIDATION
            } else if message.contains("not found") || message.contains("Memory not found") {
                xavier::server::mcp::types::XAVIER_ERROR_NOT_FOUND
            } else {
                xavier::server::mcp::types::XAVIER_ERROR_INTERNAL
            };

            let err_response = serde_json::json!({
                "error": {
                    "code": code,
                    "message": message,
                }
            });
            (StatusCode::BAD_REQUEST, Json(err_response)).into_response()
        }
    }
}

/// Workspace info handler.
pub async fn workspace_info_handler(
    State(state): State<CliState>,
) -> impl axum::response::IntoResponse {
    Json(serde_json::json!({
        "workspace_id": state.workspace_id,
        "workspace_dir": state.workspace_dir.to_string_lossy(),
    }))
}

/// Legacy REST `GET /mcp/tools` list (compat). Prefer JSON-RPC MCP on :8100.
pub async fn mcp_tools_handler() -> impl axum::response::IntoResponse {
    use axum::http::{header, HeaderMap, HeaderValue};

    let mut headers = HeaderMap::new();
    headers.insert(
        header::HeaderName::from_static("deprecation"),
        HeaderValue::from_static("true"),
    );
    headers.insert(
        header::HeaderName::from_static("link"),
        HeaderValue::from_static(
            "</mcp>; rel=\"successor-version\"; title=\"JSON-RPC MCP on port 8100\"",
        ),
    );

    (
        headers,
        Json(serde_json::json!({
            "deprecated": true,
            "note": "Legacy REST /mcp/tools on the main HTTP API (:8006). Prefer JSON-RPC MCP on port 8100 via `xavier mcp` or `xavier http --mcp-port`. Canonical agent loop: mem_search → memory_context/get_memory → create_memory.",
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
        })),
    )
}
