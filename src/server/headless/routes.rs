use crate::domain::memory::MemoryRecord;
use crate::ports::inbound::MemoryQueryPort;
use axum::response::{IntoResponse, Json as AxumJson};
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
            HeadlessToolInfo {
                name: "memory_search",
                description: "Search memory with semantic + lexical hybrid search",
            },
            HeadlessToolInfo {
                name: "memory_add",
                description: "Add a new memory entry with metadata and zone tagging",
            },
            HeadlessToolInfo {
                name: "memory_delete",
                description: "Delete a memory entry by path",
            },
            HeadlessToolInfo {
                name: "memory_stats",
                description: "Get memory statistics and counts",
            },
            HeadlessToolInfo {
                name: "memory_export",
                description: "Export all memories as JSON",
            },
            HeadlessToolInfo {
                name: "code_scan",
                description: "Scan a codebase and index symbols into the code graph",
            },
            HeadlessToolInfo {
                name: "code_find",
                description: "Find code symbols by name, kind, or file path",
            },
            HeadlessToolInfo {
                name: "code_context",
                description: "Get surrounding context for a code symbol",
            },
            HeadlessToolInfo {
                name: "code_stats",
                description: "Get code graph statistics",
            },
            HeadlessToolInfo {
                name: "agent_register",
                description: "Register a new AI agent",
            },
            HeadlessToolInfo {
                name: "agent_list",
                description: "List active agents",
            },
            HeadlessToolInfo {
                name: "agent_heartbeat",
                description: "Send heartbeat for an agent",
            },
            HeadlessToolInfo {
                name: "agent_push_context",
                description: "Push context document to an agent",
            },
            HeadlessToolInfo {
                name: "agent_unregister",
                description: "Unregister an agent",
            },
        ],
    })
}

#[derive(Debug, Deserialize)]
pub struct ToolExecuteRequest {
    pub args: serde_json::Value,
}

const SQLITE_HEADER_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// Read-only preflight run before any writable SQLite open.
///
/// SQLite's writable open/cleanup path can delete the `-wal` sidecar of a
/// database it considers corrupt, so an existing database that is not a clean
/// SQLite file is rejected here, without opening it through SQLite and without
/// creating, renaming, deleting or writing any file.
fn preflight_code_graph_db(db_path: &std::path::Path) -> Result<(), String> {
    let sidecars_present = ["-wal", "-shm"].iter().try_fold(false, |found, suffix| {
        let sidecar = std::path::PathBuf::from(format!("{}{}", db_path.display(), suffix));
        sidecar
            .try_exists()
            .map(|exists| found || exists)
            .map_err(|e| format!("cannot inspect {}: {e}", sidecar.display()))
    })?;

    let mut file = match std::fs::File::open(db_path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if sidecars_present {
                return Err(format!(
                    "code graph database {} is missing but has WAL/SHM sidecars; left unchanged",
                    db_path.display()
                ));
            }
            return Ok(());
        }
        Err(e) => {
            return Err(format!(
                "cannot read code graph database {}: {e}",
                db_path.display()
            ))
        }
    };

    let len = file
        .metadata()
        .map_err(|e| format!("cannot stat code graph database {}: {e}", db_path.display()))?
        .len();
    if len == 0 {
        if sidecars_present {
            return Err(format!(
                "code graph database {} is empty but has WAL/SHM sidecars; left unchanged",
                db_path.display()
            ));
        }
        return Ok(());
    }

    let mut header = [0u8; 16];
    match std::io::Read::read_exact(&mut file, &mut header) {
        Ok(()) if &header == SQLITE_HEADER_MAGIC => Ok(()),
        Ok(()) => Err(format!(
            "code graph database {} is not a SQLite database; left unchanged",
            db_path.display()
        )),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Err(format!(
            "code graph database {} is not a SQLite database; left unchanged",
            db_path.display()
        )),
        Err(e) => Err(format!(
            "cannot read code graph database {}: {e}",
            db_path.display()
        )),
    }
}

fn open_code_graph_db(
    workspace: &std::path::Path,
) -> Result<std::sync::Arc<code_graph::db::CodeGraphDB>, String> {
    let db_path = crate::codebase::codegraph_paths::code_graph_db_path_for(workspace);
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    preflight_code_graph_db(&db_path)?;
    code_graph::db::CodeGraphDB::new(&db_path)
        .map(std::sync::Arc::new)
        .map_err(|e| e.to_string())
}

pub const SUPPORTED_HEADLESS_TOOLS: &[&str] =
    &["code_scan", "code_find", "code_stats", "code_context"];

async fn execute_code_tool(name: &str, args: &serde_json::Value) -> axum::response::Response {
    let path_str = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let workspace = std::path::PathBuf::from(path_str);
    let db = match tokio::task::spawn_blocking({
        let w = workspace.clone();
        move || open_code_graph_db(&w)
    })
    .await
    .unwrap_or_else(|e| Err(e.to_string()))
    {
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
            let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
            let name_arg = args.get("name").and_then(|v| v.as_str());
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(20)
                .clamp(1, 100) as usize;
            let engine = code_graph::query::QueryEngine::new(db);
            let result_symbols = if let Some(n) = name_arg {
                engine.find_by_name(n, limit)
            } else {
                engine.search(query, limit).map(|res| res.symbols)
            };
            match result_symbols {
                Ok(symbols) => AxumJson(serde_json::json!({
                    "status": "ok",
                    "tool": name,
                    "query": name_arg.unwrap_or(query),
                    "count": symbols.len(),
                    "symbols": symbols,
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
            let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(10)
                .clamp(1, 50) as usize;
            let engine = code_graph::query::QueryEngine::new(db.clone());
            match engine.search(query, limit) {
                Ok(result) => {
                    let mut symbols_with_refs = Vec::new();
                    for symbol in result.symbols {
                        let mut references = Vec::new();
                        if let Some(ref stable_id) = symbol.stable_id {
                            if let Ok(edges) = db.find_edges_to(stable_id, None, 100) {
                                for edge in edges {
                                    references.push(serde_json::json!({
                                        "from_symbol": edge.from_symbol,
                                        "edge_type": format!("{:?}", edge.edge_type),
                                        "path": edge.file_path,
                                        "line": edge.line,
                                        "confidence": edge.confidence,
                                        "metadata": edge.metadata,
                                    }));
                                }
                            }
                        }
                        symbols_with_refs.push(serde_json::json!({
                            "symbol": symbol.name,
                            "symbol_type": format!("{:?}", symbol.kind),
                            "language": format!("{:?}", symbol.lang),
                            "path": symbol.file_path,
                            "line": symbol.start_line,
                            "end_line": symbol.end_line,
                            "signature": symbol.signature,
                            "stable_id": symbol.stable_id,
                            "complexity": symbol.complexity,
                            "references": references,
                        }));
                    }
                    AxumJson(serde_json::json!({
                        "status": "ok",
                        "tool": name,
                        "query": query,
                        "count": symbols_with_refs.len(),
                        "symbols": symbols_with_refs,
                    }))
                    .into_response()
                }
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
                "code": 501,
                "reason": "unsupported_in_headless",
                "supported_tools": SUPPORTED_HEADLESS_TOOLS
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

    (
        axum::http::StatusCode::NOT_IMPLEMENTED,
        AxumJson(serde_json::json!({
            "error": format!("Tool '{name}' is not implemented in headless mode"),
            "code": 501,
            "reason": "unsupported_in_headless",
            "supported_tools": SUPPORTED_HEADLESS_TOOLS
        })),
    )
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
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    #[tokio::test]
    #[serial_test::serial]
    async fn test_execute_tool_code_scan_returns_real_result() {
        let _env = crate::settings::tests::TempEnv::new();
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
        let response = execute_tool("code_scan".to_string(), req)
            .await
            .into_response();
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
    #[serial_test::serial]
    async fn headless_open_error_preserves_corrupt_db() {
        let _env = crate::settings::tests::TempEnv::new();
        let dir = tempfile::tempdir().unwrap();

        std::env::remove_var("XAVIER_CODE_GRAPH_DB_PATH");

        let db_path = crate::codebase::codegraph_paths::code_graph_db_path_for(dir.path());
        let wal_path = std::path::PathBuf::from(format!("{}-wal", db_path.display()));

        if let Some(p) = db_path.parent() {
            let _ = std::fs::create_dir_all(p);
        }

        // Write corrupt DB
        std::fs::write(&db_path, b"NOT A SQLITE DATABASE, CORRUPTED BYTES HERE").unwrap();
        // Write a WAL file marker to ensure it's not deleted
        std::fs::write(&wal_path, b"WAL MARKER").unwrap();

        let shared_db_path =
            crate::settings::XavierSettings::resolve_data_dir().join("code_graph.db");
        let shared_db_snapshot = if shared_db_path.exists() {
            Some(std::fs::read(&shared_db_path).unwrap())
        } else {
            None
        };

        let req = ToolExecuteRequest {
            args: serde_json::json!({ "path": dir.path().to_string_lossy() }),
        };

        // Call code_stats
        let response = execute_tool("code_stats".to_string(), req)
            .await
            .into_response();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["code"], 500);
        assert!(body["error"]
            .as_str()
            .unwrap()
            .contains("code graph db open failed"));

        // Call code_scan
        let req2 = ToolExecuteRequest {
            args: serde_json::json!({ "path": dir.path().to_string_lossy() }),
        };
        let response2 = execute_tool("code_scan".to_string(), req2)
            .await
            .into_response();
        assert_eq!(
            response2.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
        let bytes2 = axum::body::to_bytes(response2.into_body(), 64 * 1024)
            .await
            .unwrap();
        let body2: serde_json::Value = serde_json::from_slice(&bytes2).unwrap();
        assert_eq!(body2["code"], 500);
        assert!(body2["error"]
            .as_str()
            .unwrap()
            .contains("code graph db open failed"));

        // Assert DB and WAL are preserved
        assert!(db_path.exists(), "db file should still exist");
        assert_eq!(
            std::fs::read(&db_path).unwrap(),
            b"NOT A SQLITE DATABASE, CORRUPTED BYTES HERE",
            "db file bytes should be preserved"
        );
        assert!(wal_path.exists(), "wal file should still exist");
        assert_eq!(
            std::fs::read(&wal_path).unwrap(),
            b"WAL MARKER",
            "wal file bytes should be preserved"
        );

        if let Some(snapshot) = shared_db_snapshot {
            assert_eq!(
                std::fs::read(&shared_db_path).unwrap(),
                snapshot,
                "shared db unchanged"
            );
        } else {
            assert!(!shared_db_path.exists(), "shared db absent");
        }

        std::env::remove_var("XAVIER_CODE_GRAPH_DB_PATH");
    }

    #[tokio::test]
    async fn test_execute_tool_other_returns_200() {
        let req = ToolExecuteRequest {
            args: serde_json::json!({ "query": "test" }),
        };
        let response = execute_tool("memory_search".to_string(), req)
            .await
            .into_response();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test(flavor = "current_thread")]
    #[serial_test::serial]
    async fn test_execute_tool_code_scan_current_thread() {
        let _env = crate::settings::tests::TempEnv::new();
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
        let response = execute_tool("code_scan".to_string(), req)
            .await
            .into_response();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["status"], "ok");
        assert!(body["indexed_files"].as_u64().unwrap_or(0) >= 1);

        std::env::remove_var("XAVIER_CODE_GRAPH_DB_PATH");
    }

    /// Calls a code tool against `workspace` and returns (status, JSON body).
    async fn code_tool_json(
        name: &str,
        workspace: &std::path::Path,
    ) -> (axum::http::StatusCode, serde_json::Value) {
        let req = ToolExecuteRequest {
            args: serde_json::json!({ "path": workspace.to_string_lossy() }),
        };
        let response = execute_tool(name.to_string(), req).await.into_response();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    /// Asserts the tool fails closed with the code graph open error.
    async fn assert_db_open_rejected(workspace: &std::path::Path, tool: &str) {
        let (status, body) = code_tool_json(tool, workspace).await;
        assert_eq!(
            status,
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "{tool} should fail closed"
        );
        assert_eq!(body["code"], 500);
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .contains("code graph db open failed"),
            "{tool} error should report the open failure: {body}"
        );
    }

    /// Sorted file names in `dir`, used to prove fail-closed paths create nothing.
    fn list_dir_names(dir: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn headless_valid_db_opens_and_keeps_rows() {
        let _env = crate::settings::tests::TempEnv::new();
        let dir = tempfile::tempdir().unwrap();
        std::env::remove_var("XAVIER_CODE_GRAPH_DB_PATH");

        // The resolver places an external workspace DB under <ws>/.xavier.
        let db_path = crate::codebase::codegraph_paths::code_graph_db_path_for(dir.path());
        assert_eq!(db_path, dir.path().join(".xavier").join("code_graph.db"));

        // 1. Create a valid DB and add a symbol (via code_scan on a real rust file)
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("main.rs"), "fn special_target_symbol() {}").unwrap();

        let req_scan = ToolExecuteRequest {
            args: serde_json::json!({ "path": dir.path().to_string_lossy() }),
        };
        let response_scan = execute_tool("code_scan".to_string(), req_scan)
            .await
            .into_response();
        assert_eq!(response_scan.status(), axum::http::StatusCode::OK);
        assert!(db_path.exists(), "code_scan should create the resolved DB");

        // 2. Query it to prove the rows survived and are readable
        let req_find = ToolExecuteRequest {
            args: serde_json::json!({ "query": "special_target_symbol", "path": dir.path().to_string_lossy() }),
        };
        let response_find = execute_tool("code_find".to_string(), req_find)
            .await
            .into_response();
        assert_eq!(response_find.status(), axum::http::StatusCode::OK);
        let bytes = axum::body::to_bytes(response_find.into_body(), 64 * 1024)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["status"], "ok");
        assert_eq!(body["symbols"][0]["name"], "special_target_symbol");

        std::env::remove_var("XAVIER_CODE_GRAPH_DB_PATH");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn headless_missing_db_creates_fresh_database() {
        let _env = crate::settings::tests::TempEnv::new();
        let dir = tempfile::tempdir().unwrap();
        std::env::remove_var("XAVIER_CODE_GRAPH_DB_PATH");

        let db_path = crate::codebase::codegraph_paths::code_graph_db_path_for(dir.path());
        assert_eq!(db_path, dir.path().join(".xavier").join("code_graph.db"));
        assert!(!db_path.exists());

        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("lib.rs"), "pub fn hello() {}\n").unwrap();

        // Run code_scan to initialize the database
        let (status, body) = code_tool_json("code_scan", dir.path()).await;
        assert_eq!(status, axum::http::StatusCode::OK);
        assert_eq!(body["status"], "ok");
        assert_eq!(body["indexed_files"], 1);

        // Assert it was created
        assert!(db_path.exists());

        // The created DB must be usable by a follow-up call.
        let (status, body) = code_tool_json("code_stats", dir.path()).await;
        assert_eq!(status, axum::http::StatusCode::OK);
        assert_eq!(body["status"], "ok");
        assert_eq!(body["total_files"], 1);

        std::env::remove_var("XAVIER_CODE_GRAPH_DB_PATH");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn headless_valid_header_corrupt_db_is_preserved() {
        let _env = crate::settings::tests::TempEnv::new();
        let dir = tempfile::tempdir().unwrap();
        std::env::remove_var("XAVIER_CODE_GRAPH_DB_PATH");

        let db_path = crate::codebase::codegraph_paths::code_graph_db_path_for(dir.path());
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        // Valid SQLite magic followed by garbage: it passes the header preflight,
        // so the rejection has to come from the SQLite open itself.
        let mut corrupt = b"SQLite format 3\0".to_vec();
        corrupt.extend_from_slice(&[b'X'; 100]);
        std::fs::write(&db_path, &corrupt).unwrap();

        assert_db_open_rejected(dir.path(), "code_stats").await;
        assert_db_open_rejected(dir.path(), "code_scan").await;

        assert_eq!(
            std::fs::read(&db_path).unwrap(),
            corrupt,
            "db bytes should be preserved"
        );
        std::env::remove_var("XAVIER_CODE_GRAPH_DB_PATH");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn headless_missing_db_with_wal_sidecar_fails_closed() {
        let _env = crate::settings::tests::TempEnv::new();
        let dir = tempfile::tempdir().unwrap();
        std::env::remove_var("XAVIER_CODE_GRAPH_DB_PATH");

        let db_path = crate::codebase::codegraph_paths::code_graph_db_path_for(dir.path());
        let xavier_dir = db_path.parent().unwrap().to_path_buf();
        std::fs::create_dir_all(&xavier_dir).unwrap();
        let wal_path = std::path::PathBuf::from(format!("{}-wal", db_path.display()));
        std::fs::write(&wal_path, b"WAL MARKER").unwrap();
        let before = list_dir_names(&xavier_dir);

        assert_db_open_rejected(dir.path(), "code_stats").await;
        assert_db_open_rejected(dir.path(), "code_scan").await;

        assert!(!db_path.exists(), "missing db must not be created");
        assert_eq!(
            std::fs::read(&wal_path).unwrap(),
            b"WAL MARKER",
            "wal bytes should be preserved"
        );
        assert_eq!(
            list_dir_names(&xavier_dir),
            before,
            "no files created or removed"
        );
        std::env::remove_var("XAVIER_CODE_GRAPH_DB_PATH");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn headless_empty_db_with_sidecars_fails_closed() {
        let _env = crate::settings::tests::TempEnv::new();
        let dir = tempfile::tempdir().unwrap();
        std::env::remove_var("XAVIER_CODE_GRAPH_DB_PATH");

        let db_path = crate::codebase::codegraph_paths::code_graph_db_path_for(dir.path());
        let xavier_dir = db_path.parent().unwrap().to_path_buf();
        std::fs::create_dir_all(&xavier_dir).unwrap();
        let shm_path = std::path::PathBuf::from(format!("{}-shm", db_path.display()));
        let wal_path = std::path::PathBuf::from(format!("{}-wal", db_path.display()));
        std::fs::write(&db_path, b"").unwrap();
        std::fs::write(&shm_path, b"SHM MARKER").unwrap();
        std::fs::write(&wal_path, b"WAL MARKER").unwrap();
        let before = list_dir_names(&xavier_dir);

        assert_db_open_rejected(dir.path(), "code_stats").await;
        assert_db_open_rejected(dir.path(), "code_scan").await;

        assert_eq!(
            std::fs::read(&db_path).unwrap(),
            b"",
            "db bytes should be preserved"
        );
        assert_eq!(
            std::fs::read(&shm_path).unwrap(),
            b"SHM MARKER",
            "shm bytes should be preserved"
        );
        assert_eq!(
            std::fs::read(&wal_path).unwrap(),
            b"WAL MARKER",
            "wal bytes should be preserved"
        );
        assert_eq!(
            list_dir_names(&xavier_dir),
            before,
            "no files created or removed"
        );
        std::env::remove_var("XAVIER_CODE_GRAPH_DB_PATH");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn headless_short_header_db_fails_closed() {
        let _env = crate::settings::tests::TempEnv::new();
        let dir = tempfile::tempdir().unwrap();
        std::env::remove_var("XAVIER_CODE_GRAPH_DB_PATH");

        let db_path = crate::codebase::codegraph_paths::code_graph_db_path_for(dir.path());
        let xavier_dir = db_path.parent().unwrap().to_path_buf();
        std::fs::create_dir_all(&xavier_dir).unwrap();
        std::fs::write(&db_path, b"SQLite").unwrap();
        let before = list_dir_names(&xavier_dir);

        assert_db_open_rejected(dir.path(), "code_stats").await;
        assert_db_open_rejected(dir.path(), "code_scan").await;

        assert_eq!(
            std::fs::read(&db_path).unwrap(),
            b"SQLite",
            "db bytes should be preserved"
        );
        assert_eq!(
            list_dir_names(&xavier_dir),
            before,
            "no files created or removed"
        );
        std::env::remove_var("XAVIER_CODE_GRAPH_DB_PATH");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn headless_empty_db_without_sidecars_opens_fresh() {
        let _env = crate::settings::tests::TempEnv::new();
        let dir = tempfile::tempdir().unwrap();
        std::env::remove_var("XAVIER_CODE_GRAPH_DB_PATH");

        let db_path = crate::codebase::codegraph_paths::code_graph_db_path_for(dir.path());
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        std::fs::write(&db_path, b"").unwrap();

        let (status, body) = code_tool_json("code_scan", dir.path()).await;
        assert_eq!(status, axum::http::StatusCode::OK);
        assert_eq!(body["status"], "ok");

        let (status, body) = code_tool_json("code_stats", dir.path()).await;
        assert_eq!(status, axum::http::StatusCode::OK);
        assert_eq!(body["status"], "ok");
        std::env::remove_var("XAVIER_CODE_GRAPH_DB_PATH");
    }
}
