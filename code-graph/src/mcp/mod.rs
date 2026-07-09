pub mod context_builder;
pub mod tools;

use crate::indexer::Indexer;
use crate::query::QueryEngine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{error, info};

#[derive(Debug, Deserialize, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

pub struct McpServer {
    indexer: Arc<Indexer>,
    query_engine: Arc<QueryEngine>,
    root_path: PathBuf,
}

impl McpServer {
    pub fn new(indexer: Arc<Indexer>, query_engine: Arc<QueryEngine>, root_path: PathBuf) -> Self {
        Self {
            indexer,
            query_engine,
            root_path,
        }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        info!("Starting MCP stdio server...");
        let mut reader = BufReader::new(io::stdin());
        let mut stdout = io::stdout();
        let mut line = String::new();

        while reader.read_line(&mut line).await? > 0 {
            let request: JsonRpcRequest = match serde_json::from_str(&line) {
                Ok(req) => req,
                Err(e) => {
                    error!("Invalid JSON-RPC request: {}", e);
                    line.clear();
                    continue;
                }
            };

            let response = self.handle_request(request).await;
            let mut response_json = serde_json::to_string(&response)?;
            response_json.push('\n');
            stdout.write_all(response_json.as_bytes()).await?;
            stdout.flush().await?;

            line.clear();
        }

        Ok(())
    }

    async fn handle_request(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        let id = request.id.unwrap_or(Value::Null);

        match request.method.as_str() {
            "initialize" => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": { "listChanged": false }
                    },
                    "serverInfo": {
                        "name": "code-graph",
                        "version": "0.6.1-beta"
                    }
                })),
                error: None,
            },
            "tools/list" => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(json!({
                    "tools": [
                        {
                            "name": "codegraph_search",
                            "description": "Search for symbols (functions, structs, etc.) in the codebase using FTS5.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "query": { "type": "string", "description": "Search query" },
                                    "limit": { "type": "integer", "description": "Max results", "default": 10 }
                                },
                                "required": ["query"]
                            }
                        },
                        {
                            "name": "codegraph_explore",
                            "description": "Explore surgical context for symbols, including call graph and impact radius.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "symbols": {
                                        "type": "array",
                                        "items": { "type": "string" },
                                        "description": "List of symbol names or stable IDs to explore"
                                    },
                                    "depth": { "type": "integer", "description": "Impact radius depth", "default": 2 },
                                    "max_chars": { "type": "integer", "description": "Maximum characters in output context", "default": 8000 }
                                },
                                "required": ["symbols"]
                            }
                        }
                    ]
                })),
                error: None,
            },
            "tools/call" => {
                let params = request.params.unwrap_or(Value::Null);
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

                let result = match name {
                    "codegraph_search" => {
                        tools::handle_codegraph_search(self.query_engine.clone(), arguments).await
                    }
                    "codegraph_explore" => {
                        tools::handle_codegraph_explore(
                            self.query_engine.clone(),
                            self.indexer.clone(),
                            &self.root_path,
                            arguments,
                        )
                        .await
                    }
                    _ => Err(anyhow::anyhow!("Tool not found: {}", name)),
                };

                match result {
                    Ok(val) => JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id,
                        result: Some(val),
                        error: None,
                    },
                    Err(e) => JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32000,
                            message: e.to_string(),
                            data: None,
                        }),
                    },
                }
            }
            "notifications/initialized" => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(Value::Null),
                error: None,
            },
            _ => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32601,
                    message: format!("Method not found: {}", request.method),
                    data: None,
                }),
            },
        }
    }
}
