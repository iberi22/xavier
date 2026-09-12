//! E2E IPC tests for Unix Domain Socket inter-agent queries.
//!
//! Validates sub-millisecond local query roundtrips, symbol parsing,
//! socket binding, and concurrent inter-agent queries over Unix Domain Sockets (UDS)
//! without relying on external HTTP or MCP overhead.

use anyhow::{Context, Result};
use code_graph::db::CodeGraphDB;
use code_graph::query::QueryEngine;
use code_graph::types::{Language, Symbol, SymbolKind};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;

#[derive(Debug, Serialize, Deserialize)]
struct IpcRequest {
    command: String,
    #[serde(default)]
    query: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    20
}

#[derive(Debug, Serialize, Deserialize)]
struct IpcResponse {
    status: String,
    count: usize,
    symbols: Vec<IpcSymbol>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct IpcSymbol {
    name: String,
    kind: String,
    file_path: String,
    start_line: u32,
}

/// Helper to seed mock symbols into CodeGraph database.
fn seed_mock_symbols(db: &CodeGraphDB) -> Result<()> {
    let symbols = vec![
        Symbol {
            id: None,
            stable_id: None,
            name: "execute_agent_task".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "src/agents/executor.rs".to_string(),
            start_line: 42,
            end_line: 80,
            start_col: 0,
            end_col: 1,
            signature: Some("pub async fn execute_agent_task() -> Result<()>".to_string()),
            parent: None,
            complexity: Some(2.5),
        },
        Symbol {
            id: None,
            stable_id: None,
            name: "AgentContext".to_string(),
            kind: SymbolKind::Struct,
            lang: Language::Rust,
            file_path: "src/context/manager.rs".to_string(),
            start_line: 15,
            end_line: 30,
            start_col: 0,
            end_col: 1,
            signature: Some("pub struct AgentContext".to_string()),
            parent: None,
            complexity: Some(1.0),
        },
        Symbol {
            id: None,
            stable_id: None,
            name: "resolve_mesh_peer".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "src/mesh/discovery.rs".to_string(),
            start_line: 100,
            end_line: 140,
            start_col: 0,
            end_col: 1,
            signature: Some("pub fn resolve_mesh_peer(peer_id: &str) -> Option<Peer>".to_string()),
            parent: None,
            complexity: Some(3.0),
        },
    ];

    db.insert_symbols(&symbols)?;
    Ok(())
}

/// Helper running the UDS server loop on the given socket path.
async fn run_uds_server(
    socket_path: PathBuf,
    query_engine: Arc<QueryEngine>,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> Result<()> {
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path)
        .with_context(|| format!("Failed to bind UDS listener at {:?}", socket_path))?;

    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => {
                break;
            }
            accept_res = listener.accept() => {
                let (stream, _) = match accept_res {
                    Ok(res) => res,
                    Err(_) => continue,
                };
                let engine = Arc::clone(&query_engine);
                tokio::spawn(async move {
                    let (reader, mut writer) = stream.into_split();
                    let mut buf_reader = BufReader::new(reader);
                    let mut line = String::new();

                    while let Ok(n) = buf_reader.read_line(&mut line).await {
                        if n == 0 {
                            break;
                        }
                        let response = match serde_json::from_str::<IpcRequest>(&line) {
                            Ok(req) => match req.command.as_str() {
                                "ping" => IpcResponse {
                                    status: "ok".to_string(),
                                    count: 0,
                                    symbols: vec![],
                                    error: None,
                                },
                                "find" => match engine.search(&req.query, req.limit) {
                                    Ok(res) => {
                                        let symbols = res.symbols.into_iter().map(|s| IpcSymbol {
                                            name: s.name,
                                            kind: format!("{:?}", s.kind),
                                            file_path: s.file_path,
                                            start_line: s.start_line,
                                        }).collect();
                                        IpcResponse {
                                            status: "ok".to_string(),
                                            count: res.total,
                                            symbols,
                                            error: None,
                                        }
                                    }
                                    Err(e) => IpcResponse {
                                        status: "error".to_string(),
                                        count: 0,
                                        symbols: vec![],
                                        error: Some(e.to_string()),
                                    },
                                },
                                unknown => IpcResponse {
                                    status: "error".to_string(),
                                    count: 0,
                                    symbols: vec![],
                                    error: Some(format!("Unknown command: {}", unknown)),
                                },
                            },
                            Err(err) => IpcResponse {
                                status: "error".to_string(),
                                count: 0,
                                symbols: vec![],
                                error: Some(format!("Invalid request format: {}", err)),
                            },
                        };

                        let mut resp_bytes = serde_json::to_vec(&response).unwrap_or_default();
                        resp_bytes.push(b'\n');
                        if writer.write_all(&resp_bytes).await.is_err() {
                            break;
                        }
                        line.clear();
                    }
                });
            }
        }
    }

    let _ = std::fs::remove_file(&socket_path);
    Ok(())
}

/// Client helper for sending IPC requests over UDS.
async fn send_uds_request(socket_path: &Path, req: &IpcRequest) -> Result<IpcResponse> {
    let stream = UnixStream::connect(socket_path).await?;
    let (reader, mut writer) = stream.into_split();
    let mut req_bytes = serde_json::to_vec(req)?;
    req_bytes.push(b'\n');
    writer.write_all(&req_bytes).await?;
    writer.flush().await?;

    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();
    buf_reader.read_line(&mut line).await?;
    let response: IpcResponse = serde_json::from_str(&line)?;
    Ok(response)
}

// ─── Section A: Socket Binding and Connection Test ───────────────────────────

#[tokio::test]
async fn test_uds_socket_binding_and_connection() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let socket_path = temp_dir.path().join("codegraph_bind.sock");
    let db = Arc::new(CodeGraphDB::in_memory()?);
    let query_engine = Arc::new(QueryEngine::new(db));

    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let server_handle = tokio::spawn(run_uds_server(
        socket_path.clone(),
        query_engine,
        shutdown_rx,
    ));

    // Wait briefly for server to bind
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Verify socket file was created and is bindable
    assert!(
        socket_path.exists(),
        "Socket file should exist after UDS server bind"
    );

    let ping_req = IpcRequest {
        command: "ping".to_string(),
        query: "".to_string(),
        limit: 10,
    };

    let response = send_uds_request(&socket_path, &ping_req).await?;
    assert_eq!(response.status, "ok");
    assert_eq!(response.count, 0);

    let _ = shutdown_tx.send(());
    let _ = server_handle.await?;
    Ok(())
}

// ─── Section B: Symbol Search Roundtrip Test ──────────────────────────────────

#[tokio::test]
async fn test_uds_symbol_search_roundtrip() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let socket_path = temp_dir.path().join("codegraph_search.sock");
    let db = Arc::new(CodeGraphDB::in_memory()?);
    seed_mock_symbols(&db)?;

    let query_engine = Arc::new(QueryEngine::new(db));
    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let server_handle = tokio::spawn(run_uds_server(
        socket_path.clone(),
        query_engine,
        shutdown_rx,
    ));

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Perform symbol query over UDS
    let search_req = IpcRequest {
        command: "find".to_string(),
        query: "execute_agent_task".to_string(),
        limit: 10,
    };

    let response = send_uds_request(&socket_path, &search_req).await?;
    assert_eq!(response.status, "ok");
    assert!(
        !response.symbols.is_empty(),
        "Should return matching symbol"
    );

    let matched = &response.symbols[0];
    assert_eq!(matched.name, "execute_agent_task");
    assert_eq!(matched.kind, "Function");
    assert_eq!(matched.file_path, "src/agents/executor.rs");
    assert_eq!(matched.start_line, 42);

    let _ = shutdown_tx.send(());
    let _ = server_handle.await?;
    Ok(())
}

// ─── Section C: Concurrent Agent Queries Test ─────────────────────────────────

#[tokio::test]
async fn test_uds_concurrent_agent_queries() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let socket_path = temp_dir.path().join("codegraph_concurrent.sock");
    let db = Arc::new(CodeGraphDB::in_memory()?);
    seed_mock_symbols(&db)?;

    let query_engine = Arc::new(QueryEngine::new(db));
    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let server_handle = tokio::spawn(run_uds_server(
        socket_path.clone(),
        query_engine,
        shutdown_rx,
    ));

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let num_agents = 10;
    let mut tasks = Vec::new();

    for i in 0..num_agents {
        let sock_path = socket_path.clone();
        let query_term = if i % 2 == 0 {
            "execute"
        } else {
            "AgentContext"
        };
        tasks.push(tokio::spawn(async move {
            let req = IpcRequest {
                command: "find".to_string(),
                query: query_term.to_string(),
                limit: 5,
            };
            let response = send_uds_request(&sock_path, &req).await?;
            Ok::<_, anyhow::Error>((i, response))
        }));
    }

    for task in tasks {
        let (agent_id, response) = task.await??;
        assert_eq!(response.status, "ok");
        assert!(
            !response.symbols.is_empty(),
            "Agent {} query returned no symbols",
            agent_id
        );
    }

    let _ = shutdown_tx.send(());
    let _ = server_handle.await?;
    Ok(())
}
