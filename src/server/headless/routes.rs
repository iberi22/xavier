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

fn open_code_graph_db(workspace: &std::path::Path) -> Result<std::sync::Arc<code_graph::db::CodeGraphDB>, String> {
    let db_path = crate::codebase::codegraph_paths::code_graph_db_path_for(workspace);
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    code_graph::db::CodeGraphDB::new(&db_path)
        .or_else(|_| code_graph::db::CodeGraphDB::create_new(&db_path))
        .map(std::sync::Arc::new)
        .map_err(|e| e.to_string())
}

async fn execute_code_tool(name: &str, args: &serde_json::Value) -> axum::response::Response {
    let path_str = args
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or(".");
    let workspace = std::path::PathBuf::from(path_str);
    let db = match open_code_graph_db(&workspace) {
        Ok(db) => db,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                AxumJson(serde_json::json!({
                    "error": format!("code graph db open failed: {e}"),
                    "code": 500
                })),
            )
                .into_response();
        }
    };

    match name {
        "code_scan" => {
            let indexer = code_graph::indexer::Indexer::new(db);
            match indexer.index(&workspace, true).await {
                Ok(stats) => AxumJson(serde_json::json!({
                    "status": "ok",
                    "tool": name,
                    "indexed_files": stats.total_files,
                    "indexed_symbols": stats.total_symbols,
                    "indexed_imports": stats.total_imports,
                    "duration_ms": stats.duration_ms,
                    "paths": [path_str],
                    "languages": stats.languages,
                }))
                .into_response(),
                Err(e) => (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    AxumJson(serde_json::json!({
                        "error": e.to_string(),
                        "code": 500
                    })),
                )
                    .into_response(),
            }
        }
        "code_find" => {
            let query = args
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(20)
                .clamp(1, 100) as usize;
            let engine = code_graph::query::QueryEngine::new(db);
            match engine.search(query, limit) {
                Ok(result) => AxumJson(serde_json::json!({
                    "status": "ok",
                    "tool": name,
                    "query": query,
                    "count": result.symbols.len(),
                    "symbols": result.symbols,
                }))
                .into_response(),
                Err(e) => (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    AxumJson(serde_json::json!({
                        "error": e.to_string(),
                        "code": 500
                    })),
                )
                    .into_response(),
            }
        }
        "code_stats" => {
            let engine = code_graph::query::QueryEngine::new(db);
            match engine.stats() {
                Ok(stats) => AxumJson(serde_json::json!({
                    "status": "ok",
                    "tool": name,
                    "total_files": stats.total_files,
                    "total_symbols": stats.total_symbols,
                    "total_imports": stats.total_imports,
                    "languages": stats.languages,
                }))
                .into_response(),
                Err(e) => (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    AxumJson(serde_json::json!({
                        "error": e.to_string(),
                        "code": 500
                    })),
                )
                    .into_response(),
            }
        }
        "code_context" => {
            let query = args
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(10)
                .clamp(1, 50) as usize;
            let engine = code_graph::query::QueryEngine::new(db);
            match engine.search(query, limit) {
                Ok(result) => AxumJson(serde_json::json!({
                    "status": "ok",
                    "tool": name,
                    "query": query,
                    "count": result.symbols.len(),
                    "symbols": result.symbols,
                }))
                .into_response(),
                Err(e) => (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    AxumJson(serde_json::json!({
                        "error": e.to_string(),
                        "code": 500
                    })),
                )
                    .into_response(),
            }
        }
        _ => (
            axum::http::StatusCode::NOT_IMPLEMENTED,
            AxumJson(serde_json::json!({
                "error": format!("Tool '{name}' is not implemented in headless mode"),
                "code": 501
            })),
        )
            .into_response(),
    }
}

/// Execute tool.
pub async fn execute_tool(name: String, req: ToolExecuteRequest) -> impl IntoResponse {
    if name.starts_with("code_") {
        return execute_code_tool(&name, &req.args).await;
    }

    // Simple mock execution for non-code tools (unchanged contract)
    AxumJson(HeadlessToolExecuteResponse {
        result: HeadlessToolExecutionResult {
            tool: name,
            args_received: req.args,
            status: "executed",
        },
        execution_time_ms: 5,
    })
    .into_response()
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    #[tokio::test]
    async fn test_execute_tool_code_scan_returns_real_result() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("lib.rs"), "pub fn hello() {}\n").unwrap();
        // Isolate code-graph DB under the temp workspace
        std::env::set_var(
            "XAVIER_CODE_GRAPH_DB_PATH",
            dir.path().join("code_graph.db").to_string_lossy().as_ref(),
        );

        let req = ToolExecuteRequest {
            args: serde_json::json!({ "path": dir.path().to_string_lossy() }),
        };
        let response = execute_tool("code_scan".to_string(), req).await.into_response();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["status"], "ok");
        assert!(body["indexed_files"].as_u64().unwrap_or(0) >= 1);

        std::env::remove_var("XAVIER_CODE_GRAPH_DB_PATH");
    }

    #[tokio::test]
    async fn test_execute_tool_other_returns_200() {
        let req = ToolExecuteRequest {
            args: serde_json::json!({ "query": "test" }),
        };
        let response = execute_tool("memory_search".to_string(), req).await.into_response();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }
}
