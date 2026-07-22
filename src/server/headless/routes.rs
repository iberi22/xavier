use crate::domain::memory::MemoryRecord;
use crate::ports::inbound::MemoryQueryPort;
use axum::{
    response::{IntoResponse, Json as AxumJson},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct HeadlessHealthResponse {
    pub status: &'static str,
    pub service: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Serialize)]
pub struct HeadlessContextResponse {
    pub items: Vec<MemoryRecord>,
    pub total: usize,
}

#[derive(Debug, Serialize)]
pub struct HeadlessSearchResponse {
    pub results: Vec<MemoryRecord>,
    pub total: usize,
}

#[derive(Debug, Serialize)]
pub struct HeadlessToolInfo {
    pub name: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Serialize)]
pub struct HeadlessToolsResponse {
    pub tools: Vec<HeadlessToolInfo>,
}

#[derive(Debug, Serialize)]
pub struct HeadlessToolExecutionResult {
    pub tool: String,
    pub args_received: serde_json::Value,
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct HeadlessToolExecuteResponse {
    pub result: HeadlessToolExecutionResult,
    pub execution_time_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct HeadlessProviderQuota {
    pub remaining_percentage: f64,
    pub reset_at: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct HeadlessProviderStatusResponse {
    pub active: String,
    pub available: Vec<&'static str>,
    pub strategy: &'static str,
    pub quota: HeadlessProviderQuota,
}

// ═════════════════════════════════════════════════════════════════════════════
// Handlers
// ═════════════════════════════════════════════════════════════════════════════

/// Health.
pub async fn health() -> impl IntoResponse {
    AxumJson(HeadlessHealthResponse {
        status: "ok",
        service: "xavier-headless",
        version: env!("CARGO_PKG_VERSION"),
    })
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
            AxumJson(HeadlessContextResponse {
                total: items.len(),
                items,
            })
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
            AxumJson(HeadlessSearchResponse {
                total: results.len(),
                results,
            })
            .into_response()
        }
        Err(e) => crate::error::ApiError::internal(e.to_string()).into_response(),
    }
}

/// Tools.
pub async fn tools() -> impl IntoResponse {
    AxumJson(HeadlessToolsResponse {
        tools: vec![
            HeadlessToolInfo { name: "memory_search", description: "Search memory with semantic + lexical hybrid search" },
            HeadlessToolInfo { name: "memory_add", description: "Add a new memory entry with metadata and zone tagging" },
            HeadlessToolInfo { name: "memory_delete", description: "Delete a memory entry by path" },
            HeadlessToolInfo { name: "memory_stats", description: "Get memory statistics and counts" },
            HeadlessToolInfo { name: "memory_export", description: "Export all memories as JSON" },
            HeadlessToolInfo { name: "code_scan", description: "Scan a codebase and index symbols into the code graph" },
            HeadlessToolInfo { name: "code_find", description: "Find code symbols by name, kind, or file path" },
            HeadlessToolInfo { name: "code_context", description: "Get surrounding context for a code symbol" },
            HeadlessToolInfo { name: "code_stats", description: "Get code graph statistics" },
            HeadlessToolInfo { name: "agent_register", description: "Register a new AI agent" },
            HeadlessToolInfo { name: "agent_list", description: "List active agents" },
            HeadlessToolInfo { name: "agent_heartbeat", description: "Send heartbeat for an agent" },
            HeadlessToolInfo { name: "agent_push_context", description: "Push context document to an agent" },
            HeadlessToolInfo { name: "agent_unregister", description: "Unregister an agent" },
        ]
    })
}

#[derive(Debug, Deserialize)]
pub struct ToolExecuteRequest {
    pub args: serde_json::Value,
}

/// Execute tool.
pub async fn execute_tool(name: String, req: ToolExecuteRequest) -> impl IntoResponse {
    // Simple mock execution for now
    AxumJson(HeadlessToolExecuteResponse {
        result: HeadlessToolExecutionResult {
            tool: name,
            args_received: req.args,
            status: "executed",
        },
        execution_time_ms: 5,
    })
}

/// Provider status.
pub async fn provider_status(active_provider: String) -> impl IntoResponse {
    AxumJson(HeadlessProviderStatusResponse {
        active: active_provider,
        available: vec!["openai", "anthropic", "groq", "local"],
        strategy: "manual", // Default to manual
        quota: HeadlessProviderQuota {
            remaining_percentage: 0.85,
            reset_at: None,
        }
    })
}
