use crate::domain::memory::MemoryRecord;
use crate::ports::inbound::MemoryQueryPort;
use axum::{
    response::{IntoResponse, Json as AxumJson},
};
use serde::Deserialize;
use serde_json::json;

// ═════════════════════════════════════════════════════════════════════════════
// Handlers
// ═════════════════════════════════════════════════════════════════════════════

/// Health.
pub async fn health() -> impl IntoResponse {
    AxumJson(json!({
        "status": "ok",
        "service": "xavier-headless",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

#[derive(Debug, Deserialize)]
pub struct ContextParams {
    pub query: String,
    pub limit: Option<usize>,
}

/// Context.
pub async fn context(memory: &dyn MemoryQueryPort, params: ContextParams) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(10);
    // Use MemoryQueryPort search
    match memory.search(&params.query, limit, None).await {
        Ok(results) => {
            let items: Vec<MemoryRecord> = results.into_iter().take(limit).collect();
            AxumJson(json!({
                "items": items,
                "total": items.len(),
            }))
            .into_response()
        }
        Err(e) => crate::error::ApiError::internal(e.to_string()).into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub text: String,
    pub limit: Option<usize>,
    pub filters: Option<serde_json::Value>,
}

/// Memory search.
pub async fn memory_search(memory: &dyn MemoryQueryPort, req: SearchRequest) -> impl IntoResponse {
    let limit = req.limit.unwrap_or(10);
    // Use MemoryQueryPort search
    match memory.search(&req.text, limit, None).await {
        Ok(results) => {
            let results: Vec<MemoryRecord> = results.into_iter().take(limit).collect();
            AxumJson(json!({
                "results": results,
                "total": results.len(),
            }))
            .into_response()
        }
        Err(e) => crate::error::ApiError::internal(e.to_string()).into_response(),
    }
}

/// Tools.
pub async fn tools() -> impl IntoResponse {
    AxumJson(json!({
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

#[derive(Debug, Deserialize)]
pub struct ToolExecuteRequest {
    pub args: serde_json::Value,
}

/// Execute tool.
pub async fn execute_tool(name: String, req: ToolExecuteRequest) -> impl IntoResponse {
    // Simple mock execution for now
    AxumJson(json!({
        "result": {
            "tool": name,
            "args_received": req.args,
            "status": "executed"
        },
        "execution_time_ms": 5,
    }))
}

/// Provider status.
pub async fn provider_status(active_provider: String) -> impl IntoResponse {
    AxumJson(json!({
        "active": active_provider,
        "available": ["openai", "anthropic", "groq", "local"],
        "strategy": "manual", // Default to manual
        "quota": {
            "remaining_percentage": 0.85,
            "reset_at": null
        }
    }))
}
