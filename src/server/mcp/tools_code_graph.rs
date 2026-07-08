//! MCP tools for CodeGraph Structural Intelligence
//!
//! Exposes `codegraph_explore`, `trace_path`, `get_architecture`, and `detect_changes`.

use super::types::*;
use crate::workspace::WorkspaceContext;
use crate::AppState;
use serde_json::{json, Value};

pub fn get_code_graph_tools() -> Vec<MCPTool> {
    vec![
        MCPTool {
            name: "codegraph_explore".to_string(),
            description: "ONE tool for all code discovery. Returns the exact, line-numbered source of the symbols you name (functions, classes, routes) AND the caller/callee path between them. Use this instead of reading files or searching.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The natural language question, OR specific symbol names (e.g. 'PaymentService process' or 'src/utils.ts')"
                    },
                    "max_files": {
                        "type": "number",
                        "description": "Max files to include (default adaptive based on project size)"
                    }
                },
                "required": ["query"]
            }),
        },
        MCPTool {
            name: "trace_path".to_string(),
            description: "Cypher-like recursive path tracing for impact analysis. Shows who calls X (reverse=true) or who X calls (reverse=false).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol": {
                        "type": "string",
                        "description": "The exact symbol name to trace"
                    },
                    "max_depth": {
                        "type": "number",
                        "description": "Maximum traversal depth (default 5)"
                    },
                    "reverse": {
                        "type": "boolean",
                        "description": "If true, finds callers (impact radius). If false, finds callees (dependencies)."
                    }
                },
                "required": ["symbol"]
            }),
        },
        MCPTool {
            name: "get_architecture".to_string(),
            description: "Surfaces the high-level architecture: entry points, HTTP routes, modules, and boundaries.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        MCPTool {
            name: "detect_changes".to_string(),
            description: "Traces the impact of uncommitted Git diffs through the code graph.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
    ]
}

pub fn is_code_graph_tool(name: &str) -> bool {
    matches!(
        name,
        "codegraph_explore" | "trace_path" | "get_architecture" | "detect_changes"
    )
}

pub async fn handle_code_graph_tool(
    _state: AppState,
    _workspace: WorkspaceContext,
    name: &str,
    arguments: Value,
) -> anyhow::Result<Value> {
    match name {
        "codegraph_explore" => {
            let query = arguments.get("query").and_then(|v| v.as_str()).unwrap_or("");
            // TODO: Delegate to code_graph crate's explore logic
            let response = format!("(Mock) Explored graph for: {}. Returning line-numbered surgical context and caller paths...", query);
            super::server::mcp_text_result(response, false)
        }
        "trace_path" => {
            let symbol = arguments.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
            let max_depth = arguments.get("max_depth").and_then(|v| v.as_u64()).unwrap_or(5) as u32;
            let reverse = arguments.get("reverse").and_then(|v| v.as_bool()).unwrap_or(true);
            
            // TODO: Call `code_graph::db::cypher::trace_path`
            let dir = if reverse { "Callers of" } else { "Callees of" };
            let response = format!("(Mock) Tracing {} '{}' up to depth {}", dir, symbol, max_depth);
            super::server::mcp_text_result(response, false)
        }
        "get_architecture" => {
            super::server::mcp_text_result("(Mock) Architecture: Entry Points: main.rs. Routes: /api/v1/*".to_string(), false)
        }
        "detect_changes" => {
            super::server::mcp_text_result("(Mock) No uncommitted structural changes detected.".to_string(), false)
        }
        _ => anyhow::bail!("Unknown code graph tool: {}", name),
    }
}
