//! MCP tools for CodeGraph Structural Intelligence
//!
//! Exposes `codegraph_explore`, `trace_path`, `get_architecture`, and `detect_changes`.
//! These tools are backed by the real `code_graph` engine held in `AppState`
//! (`code_db`, `code_query`, `code_indexer`).

use super::types::*;
use crate::workspace::WorkspaceContext;
use crate::AppState;
use code_graph::db::cypher::TraversalPath;
use code_graph::types::IndexStats;
use serde_json::{json, Value};
use std::path::PathBuf;

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
    state: AppState,
    _workspace: WorkspaceContext,
    name: &str,
    arguments: Value,
) -> anyhow::Result<Value> {
    let result = match name {
        "codegraph_explore" => handle_explore(state, arguments).await,
        "trace_path" => handle_trace_path(state, arguments).await,
        "get_architecture" => handle_get_architecture(state).await,
        "detect_changes" => handle_detect_changes(state).await,
        _ => anyhow::bail!("Unknown code graph tool: {}", name),
    };

    match result {
        Ok(value) => Ok(value),
        Err(err) => {
            tracing::error!(error = %err, tool = name, "code graph tool failed");
            let payload = json!({
                "tool": name,
                "error": err.to_string()
            });
            Ok(serde_json::to_value(MCPToolResult::structured(
                payload, true,
            ))?)
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Resolve a symbol name to its stable_id via the query engine.
/// Returns the first match (highest score) if any.
fn resolve_stable_id(state: &AppState, query: &str) -> anyhow::Result<Option<String>> {
    // If it already looks like a stable_id (64 hex chars), return as-is.
    if query.len() == 64 && query.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(Some(query.to_string()));
    }
    let result = state.code_query.search(query, 1)?;
    Ok(result.symbols.into_iter().next().and_then(|s| s.stable_id))
}

/// Convert a stable_id (or pseudo-node like "file:..." / "module:...") into a
/// human-readable label for display. Pseudo-nodes are returned verbatim.
fn label_for_node(state: &AppState, node_id: &str) -> String {
    if node_id.starts_with("file:") || node_id.starts_with("module:") {
        return node_id.to_string();
    }
    match state.code_db.symbol_by_stable_id(node_id) {
        Ok(Some(symbol)) => format!(
            "{} ({}:{})",
            symbol.name, symbol.file_path, symbol.start_line
        ),
        Ok(None) => node_id.to_string(),
        Err(_) => node_id.to_string(),
    }
}

/// Read a slice of source code for a symbol from disk. Lines are 1-based and
/// inclusive. Returns None if the file cannot be read (degraded gracefully).
fn read_source_slice(file_path: &str, start_line: u32, end_line: u32) -> Option<String> {
    let candidates = candidate_paths(file_path);
    let content = candidates
        .into_iter()
        .find_map(|p| std::fs::read_to_string(&p).ok())?;
    let start = start_line.saturating_sub(1) as usize;
    let take = (end_line.saturating_sub(start_line) + 1).max(1) as usize;
    let slice: Vec<&str> = content.lines().skip(start).take(take).collect();
    let header_line = start_line;
    Some(
        slice
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:>5} | {}", header_line as usize + i, line))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// Build candidate absolute paths for a stored (often repo-relative) file path.
fn candidate_paths(file_path: &str) -> Vec<PathBuf> {
    let mut out = Vec::with_capacity(3);
    let direct = PathBuf::from(file_path);
    out.push(direct.clone());
    if direct.is_absolute() {
        return out;
    }
    if let Ok(cwd) = std::env::current_dir() {
        out.push(cwd.join(file_path));
    }
    out
}

// ── Tool handlers ────────────────────────────────────────────────────

async fn handle_explore(state: AppState, arguments: Value) -> anyhow::Result<Value> {
    let query = arguments
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let max_files = arguments
        .get("max_files")
        .and_then(|v| v.as_u64())
        .unwrap_or(8) as usize;
    let limit = max_files.clamp(1, 50);

    if query.trim().is_empty() {
        anyhow::bail!("'query' is required and must be non-empty");
    }

    let result = state.code_query.search(query, limit)?;
    let total = result.total;

    // If the query looks like a file path, also try find_by_file as a fallback.
    let mut symbols = result.symbols;
    if symbols.is_empty() && (query.contains('/') || query.contains('\\')) {
        let cleaned = query.replace('\\', "/");
        symbols = state
            .code_db
            .find_by_file(&cleaned)
            .unwrap_or_default()
            .into_iter()
            .take(limit)
            .collect();
    }

    let entries: Vec<Value> = symbols
        .iter()
        .map(|symbol| {
            let source = read_source_slice(&symbol.file_path, symbol.start_line, symbol.end_line);
            let callers = state
                .code_db
                .find_edges_to(
                    symbol.stable_id.as_deref().unwrap_or(""),
                    Some(code_graph::types::EdgeType::Calls),
                    10,
                )
                .unwrap_or_default();
            let callees = state
                .code_db
                .find_edges_from(
                    symbol.stable_id.as_deref().unwrap_or(""),
                    Some(code_graph::types::EdgeType::Calls),
                    10,
                )
                .unwrap_or_default();
            json!({
                "name": symbol.name,
                "kind": format!("{:?}", symbol.kind),
                "language": format!("{:?}", symbol.lang),
                "file": symbol.file_path,
                "start_line": symbol.start_line,
                "end_line": symbol.end_line,
                "signature": symbol.signature,
                "source": source,
                "callers": callers.iter().map(|e| label_for_node(&state, &e.from_symbol)).collect::<Vec<_>>(),
                "callees": callees.iter().map(|e| label_for_node(&state, &e.to_symbol)).collect::<Vec<_>>(),
            })
        })
        .collect();

    let payload = json!({
        "query": query,
        "total_matches": total,
        "returned": entries.len(),
        "symbols": entries,
    });
    Ok(serde_json::to_value(MCPToolResult::structured(
        payload, false,
    ))?)
}

async fn handle_trace_path(state: AppState, arguments: Value) -> anyhow::Result<Value> {
    let symbol = arguments
        .get("symbol")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let max_depth = arguments
        .get("max_depth")
        .and_then(|v| v.as_u64())
        .unwrap_or(5) as u32;
    let reverse = arguments
        .get("reverse")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    if symbol.trim().is_empty() {
        anyhow::bail!("'symbol' is required and must be non-empty");
    }

    let stable_id = resolve_stable_id(&state, symbol)?
        .ok_or_else(|| anyhow::anyhow!("Symbol '{}' not found in the code graph", symbol))?;

    let paths: Vec<TraversalPath> = state.code_db.trace_path(&stable_id, max_depth, reverse)?;

    let direction = if reverse { "callers" } else { "callees" };
    let annotated: Vec<Value> = paths
        .iter()
        .map(|p| {
            // Annotate each segment of path_str with human labels.
            let segments: Vec<String> = p
                .path_str
                .split(" -> ")
                .map(|seg| label_for_node(&state, seg.trim()))
                .collect();
            json!({
                "target": label_for_node(&state, &p.target_symbol),
                "depth": p.depth,
                "path": segments.join(" -> "),
            })
        })
        .collect();

    let payload = json!({
        "symbol": label_for_node(&state, &stable_id),
        "direction": direction,
        "reverse": reverse,
        "max_depth": max_depth,
        "paths_found": annotated.len(),
        "paths": annotated,
    });
    Ok(serde_json::to_value(MCPToolResult::structured(
        payload, false,
    ))?)
}

async fn handle_get_architecture(state: AppState) -> anyhow::Result<Value> {
    let stats: IndexStats = state.code_db.stats()?;
    let hubs = state.code_db.hub_nodes(3, 15).unwrap_or_default();
    let hotspots = state
        .code_db
        .complexity_hotspots(8.0, 10)
        .unwrap_or_default();
    let all_functions = state
        .code_db
        .find_by_kind(code_graph::types::SymbolKind::Function, 500)?;

    // Heuristic entry points: functions with no incoming Calls edges.
    let mut entry_points = Vec::new();
    for func in &all_functions {
        let id = match func.stable_id.as_deref() {
            Some(id) => id,
            None => continue,
        };
        let incoming = state
            .code_db
            .find_edges_to(id, Some(code_graph::types::EdgeType::Calls), 1)
            .unwrap_or_default();
        if incoming.is_empty() {
            entry_points.push(json!({
                "name": func.name,
                "file": func.file_path,
                "line": func.start_line,
            }));
            if entry_points.len() >= 20 {
                break;
            }
        }
    }

    let payload = json!({
        "stats": {
            "total_files": stats.total_files,
            "total_symbols": stats.total_symbols,
            "total_imports": stats.total_imports,
            "languages": stats.languages.iter().map(|lc| {
                json!({"language": format!("{:?}", lc.lang), "count": lc.count})
            }).collect::<Vec<_>>(),
        },
        "entry_points": entry_points,
        "hubs": hubs.iter().take(15).map(|h| json!({
            "symbol": format!("{} ({}:{})", h.symbol.name, h.symbol.file_path, h.symbol.start_line),
            "incoming": h.incoming,
            "outgoing": h.outgoing,
            "total_degree": h.total,
        })).collect::<Vec<_>>(),
        "complexity_hotspots": hotspots.iter().map(|c| json!({
            "symbol": format!("{} ({}:{})", c.symbol.name, c.symbol.file_path, c.symbol.start_line),
            "complexity": c.symbol.complexity,
            "risk_score": c.risk_score,
        })).collect::<Vec<_>>(),
    });
    Ok(serde_json::to_value(MCPToolResult::structured(
        payload, false,
    ))?)
}

async fn handle_detect_changes(state: AppState) -> anyhow::Result<Value> {
    // Run git diff to list changed files (unstaged + staged, renamed tracked).
    let output = std::process::Command::new("git")
        .args(["diff", "--name-only", "HEAD"])
        .output();

    let changed_files: Vec<String> = match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().replace('\\', "/"))
            .filter(|l| !l.is_empty())
            .collect(),
        Ok(out) => {
            // Not a git repo or git missing — degrade gracefully.
            let payload = json!({
                "available": false,
                "reason": "git diff HEAD failed (not a git repo or git unavailable)",
                "stderr": String::from_utf8_lossy(&out.stderr).to_string(),
            });
            return Ok(serde_json::to_value(MCPToolResult::structured(
                payload, false,
            ))?);
        }
        Err(err) => {
            let payload = json!({
                "available": false,
                "reason": format!("could not run git: {}", err),
            });
            return Ok(serde_json::to_value(MCPToolResult::structured(
                payload, false,
            ))?);
        }
    };

    if changed_files.is_empty() {
        let payload = json!({
            "available": true,
            "changed_files": 0,
            "impacted_symbols": [],
        });
        return Ok(serde_json::to_value(MCPToolResult::structured(
            payload, false,
        ))?);
    }

    let mut impacts: Vec<Value> = Vec::new();
    for file in &changed_files {
        let symbols = state.code_db.find_by_file(file).unwrap_or_default();
        for symbol in symbols {
            let id = match symbol.stable_id.as_deref() {
                Some(id) => id,
                None => continue,
            };
            // Who depends on this symbol? reverse trace.
            let callers = state.code_db.trace_path(id, 2, true).unwrap_or_default();
            impacts.push(json!({
                "changed_symbol": format!("{} ({}:{})", symbol.name, symbol.file_path, symbol.start_line),
                "kind": format!("{:?}", symbol.kind),
                "callers_within_reach": callers.len(),
                "sample_callers": callers.iter().take(5).map(|p| label_for_node(&state, &p.target_symbol)).collect::<Vec<_>>(),
            }));
            if impacts.len() >= 100 {
                break;
            }
        }
    }

    let payload = json!({
        "available": true,
        "changed_files": changed_files.len(),
        "files": changed_files,
        "impacted_symbols": impacts,
    });
    Ok(serde_json::to_value(MCPToolResult::structured(
        payload, false,
    ))?)
}
