//! Code handlers for scanning, searching, and analyzing codebases.

use axum::extract::State;
use axum::Json;
use serde::Serialize;
use std::path::PathBuf;
use tracing::{info, warn};

use crate::cli::code_dump::{perform_dump, perform_load};
// TODO: Re-implement code_find_symbols and filter_symbols_by_query via code_graph::query::QueryEngine
use crate::cli::security::secure_optional_request_field;
use crate::cli::state::CliState;
use crate::cli::types::*;
use crate::cli::utils::estimate_tokens;
use code_graph::types::{EdgeType, Symbol, SymbolKind};

use xavier::ports::inbound::input_security_port::SecureInputResult;

/// Auto-index `src/` directory into the code graph.
///
/// Scans all source files under the project's `src/` directory and builds
/// the code graph with symbols, imports, and relationships.
pub async fn code_index_handler(
    State(state): State<CliState>,
    payload: Option<axum::Json<serde_json::Value>>,
) -> Json<serde_json::Value> {
    let base_path = payload
        .as_ref()
        .and_then(|payload| payload.get("path"))
        .and_then(|v| v.as_str())
        .unwrap_or("src");
    info!("Code index request: path={}", base_path);

    let code_graph = state.code_graph.read().await;
    match code_graph
        .indexer
        .index(std::path::Path::new(base_path), true)
        .await
    {
        Ok(stats) => Json(serde_json::json!({
            "status": "ok",
            "indexed_files": stats.total_files,
            "indexed_symbols": stats.total_symbols,
            "indexed_imports": stats.total_imports,
            "duration_ms": stats.duration_ms,
            "paths": [base_path],
            "languages": stats.languages,
            "message": format!("Indexed {} files, {} symbols, {} imports across {:?}",
                stats.total_files, stats.total_symbols, stats.total_imports, stats.languages),
        })),
        Err(error) => Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
            "indexed_files": 0,
            "indexed_symbols": 0,
            "indexed_imports": 0,
            "paths": [base_path],
        })),
    }
}

pub async fn code_dump_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<serde_json::Value>,
) -> impl axum::response::IntoResponse {
    let path = payload.get("path").and_then(|v| v.as_str()).unwrap_or(".");

    let code_graph = state.code_graph.read().await;
    match perform_dump(&code_graph, path).await {
        Ok(dump_path) => axum::Json(serde_json::json!({
            "status": "ok",
            "message": format!("Code graph dumped to {}", dump_path.display()),
            "path": dump_path.to_string_lossy(),
        })),
        Err(error) => axum::Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
        })),
    }
}

pub async fn code_load_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<serde_json::Value>,
) -> impl axum::response::IntoResponse {
    let path = payload.get("path").and_then(|v| v.as_str()).unwrap_or(".");

    match perform_load(path).await {
        Ok(new_state) => {
            let mut code_graph = state.code_graph.write().await;
            *code_graph = new_state;
            axum::Json(serde_json::json!({
                "status": "ok",
                "message": "Code graph loaded successfully from dump",
            }))
        }
        Err(error) => axum::Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
        })),
    }
}

pub async fn code_scan_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeScanPayload>,
) -> impl axum::response::IntoResponse {
    let requested_path = payload.path.unwrap_or_else(|| ".".to_string());

    let sec_result = state
        .security
        .process_input(&requested_path)
        .await
        .unwrap_or_else(|_| SecureInputResult {
            allowed: false,
            sanitized_input: None,
            original_input: requested_path.clone(),
            detection_confidence: 1.0,
            is_injection: true,
            attack_type: "unknown".to_string(),
        });

    if !sec_result.allowed {
        info!(
            "code/scan blocked by security: injection detected (confidence={})",
            sec_result.detection_confidence
        );
        return axum::Json(serde_json::json!({
            "status": "blocked",
            "reason": "security_policy_violation",
            "detection": {
                "is_injection": sec_result.is_injection,
                "confidence": sec_result.detection_confidence,
                "attack_type": sec_result.attack_type,
            }
        }));
    }

    let workspace_root =
        std::path::absolute(&state.workspace_dir).unwrap_or_else(|_| PathBuf::from("."));
    let Ok(abs_path) = std::path::absolute(&requested_path) else {
        return axum::Json(serde_json::json!({
            "status": "error",
            "message": "invalid path",
            "indexed_files": 0,
        }));
    };
    if !abs_path.starts_with(&workspace_root) {
        warn!(
            "Path traversal blocked: {} is outside workspace root {}",
            abs_path.display(),
            workspace_root.display()
        );
        return axum::Json(serde_json::json!({
            "status": "error",
            "message": "path outside workspace not allowed",
            "indexed_files": 0,
        }));
    }

    let path = requested_path;
    info!("Code scan request: path={}", path);

    let code_graph = state.code_graph.read().await;
    match code_graph.indexer.index(std::path::Path::new(&path), true).await {
        Ok(stats) => {
            // Automatically trigger dump after successful scan
            let dump_msg = match perform_dump(&code_graph, &path).await {
                Ok(dump_path) => format!(" (Dumped to {})", dump_path.display()),
                Err(e) => format!(" (Dump failed: {})", e),
            };

            axum::Json(serde_json::json!({
                "status": "ok",
                "indexed_files": stats.total_files,
                "indexed_symbols": stats.total_symbols,
                "indexed_imports": stats.total_imports,
                "duration_ms": stats.duration_ms,
                "paths": [path],
                "languages": stats.languages,
                "message": format!("Scan complete. {}{}", stats.to_string(), dump_msg),
            }))
        }
        Err(error) => axum::Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
            "indexed_files": 0,
            "indexed_symbols": 0,
            "indexed_imports": 0,
            "paths": [path],
        })),
    }
}

pub async fn code_find_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeFindPayload>,
) -> impl axum::response::IntoResponse {
    let sec_result = state
        .security
        .process_input(&payload.query)
        .await
        .unwrap_or_else(|_| SecureInputResult {
            allowed: false,
            sanitized_input: None,
            original_input: payload.query.clone(),
            detection_confidence: 1.0,
            is_injection: true,
            attack_type: "unknown".to_string(),
        });

    if !sec_result.allowed {
        info!(
            "code/find blocked by security: injection detected (confidence={})",
            sec_result.detection_confidence
        );
        return axum::Json(serde_json::json!({
            "status": "blocked",
            "reason": "security_policy_violation",
            "blocked": true,
            "detection": {
                "is_injection": sec_result.is_injection,
                "confidence": sec_result.detection_confidence,
                "attack_type": sec_result.attack_type,
            }
        }));
    }

    let query = sec_result
        .sanitized_input
        .as_deref()
        .unwrap_or(&sec_result.original_input)
        .to_string();
    let pattern = match secure_optional_request_field(
        state.security.as_ref(),
        "code/find pattern",
        payload.pattern.as_deref(),
    )
    .await
    {
        Ok(pattern) => pattern,
        Err(sec_result) => {
            info!(
                "code/find blocked by security: pattern rejected (confidence={})",
                sec_result.detection_confidence
            );
            return axum::Json(serde_json::json!({
                "status": "blocked",
                "reason": "security_policy_violation",
                "blocked": true,
                "field": "pattern",
                "detection": {
                    "is_injection": sec_result.is_injection,
                    "confidence": sec_result.detection_confidence,
                    "attack_type": sec_result.attack_type,
                }
            }));
        }
    };
    let kind = match secure_optional_request_field(
        state.security.as_ref(),
        "code/find kind",
        payload.kind.as_deref(),
    )
    .await
    {
        Ok(kind) => kind,
        Err(sec_result) => {
            info!(
                "code/find blocked by security: kind rejected (confidence={})",
                sec_result.detection_confidence
            );
            return axum::Json(serde_json::json!({
                "status": "blocked",
                "reason": "security_policy_violation",
                "blocked": true,
                "field": "kind",
                "detection": {
                    "is_injection": sec_result.is_injection,
                    "confidence": sec_result.detection_confidence,
                    "attack_type": sec_result.attack_type,
                }
            }));
        }
    };
    let limit = payload.limit.clamp(1, 100);
    info!(
        "Code find request: query={}, limit={}, kind={:?}, pattern={:?}",
        query, limit, kind, pattern
    );

    let code_graph = state.code_graph.read().await;
    let symbols = match code_graph.query.search(&query, limit) {
        Ok(result) => result.symbols,
        Err(_) => Vec::new(),
    };

    let results: Vec<_> = symbols
        .into_iter()
        .map(|symbol| {
            serde_json::json!({
                "id": symbol.id,
                "stable_id": symbol.stable_id,
                "path": symbol.file_path,
                "symbol": symbol.name,
                "symbol_type": format!("{:?}", symbol.kind),
                "language": format!("{:?}", symbol.lang),
                "line": symbol.start_line,
                "end_line": symbol.end_line,
                "signature": symbol.signature,
                "parent": symbol.parent,
                "complexity": symbol.complexity,
            })
        })
        .collect();

    axum::Json(serde_json::json!({
        "status": "ok",
        "query": query,
        "count": results.len(),
        "results": results,
    }))
}

pub async fn code_stats_handler(
    State(state): State<CliState>,
) -> impl axum::response::IntoResponse {
    let code_graph = state.code_graph.read().await;
    match code_graph.db.stats() {
        Ok(stats) => axum::Json(serde_json::json!({
            "status": "ok",
            "total_files": stats.total_files,
            "total_symbols": stats.total_symbols,
            "total_imports": stats.total_imports,
            "languages": stats.languages,
        })),
        Err(error) => axum::Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
            "total_files": 0,
            "total_symbols": 0,
            "total_imports": 0,
        })),
    }
}

pub async fn code_context_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeContextPayload>,
) -> impl axum::response::IntoResponse {
    let sec_result = state
        .security
        .process_input(&payload.query)
        .await
        .unwrap_or_else(|_| SecureInputResult {
            allowed: false,
            sanitized_input: None,
            original_input: payload.query.clone(),
            detection_confidence: 1.0,
            is_injection: true,
            attack_type: "unknown".to_string(),
        });

    if !sec_result.allowed {
        info!(
            "code/context blocked by security: injection detected (confidence={})",
            sec_result.detection_confidence
        );
        return axum::Json(serde_json::json!({
            "status": "blocked",
            "reason": "security_policy_violation",
            "blocked": true,
            "detection": {
                "is_injection": sec_result.is_injection,
                "confidence": sec_result.detection_confidence,
                "attack_type": sec_result.attack_type,
            }
        }));
    }

    let limit = payload.limit.clamp(1, 100);
    let kind_limit = if payload.query.trim().is_empty() {
        limit
    } else {
        10_000
    };
    let budget_tokens = payload.budget_tokens.clamp(100, 8000);

    let code_graph = state.code_graph.read().await;
    let mut symbols = if let Some(kind) = payload.kind.as_deref() {
        match kind.to_ascii_lowercase().as_str() {
            "function" | "fn" => code_graph.query.functions(kind_limit).unwrap_or_default(),
            "struct" => code_graph.query.structs(kind_limit).unwrap_or_default(),
            "class" => code_graph.query.classes(kind_limit).unwrap_or_default(),
            "enum" => code_graph.query.enums(kind_limit).unwrap_or_default(),
            _ => code_graph
                .query
                .search(&payload.query, limit)
                .map(|result| result.symbols)
                .unwrap_or_default(),
        }
    } else {
        code_graph
            .query
            .search(&payload.query, limit)
            .map(|result| result.symbols)
            .unwrap_or_default()
    };
    // filter_symbols_by_query removed — not available in code_graph API
    // The search already filters by query via QueryEngine::search
    symbols.truncate(limit);

    let mut used_tokens = 0usize;
    let mut context = Vec::new();

    for symbol in symbols {
        let signature = symbol.signature.clone().unwrap_or_default();
        let compact = serde_json::json!({
            "symbol": symbol.name,
            "symbol_type": format!("{:?}", symbol.kind),
            "language": format!("{:?}", symbol.lang),
            "path": symbol.file_path,
            "line": symbol.start_line,
            "end_line": symbol.end_line,
            "signature": signature,
            "stable_id": symbol.stable_id,
            "complexity": symbol.complexity,
        });
        let estimated = estimate_tokens(&compact.to_string());
        if used_tokens + estimated > budget_tokens && !context.is_empty() {
            break;
        }
        used_tokens += estimated;
        context.push(compact);
    }

    axum::Json(serde_json::json!({
        "status": "ok",
        "query": payload.query,
        "budget_tokens": budget_tokens,
        "estimated_tokens": used_tokens,
        "count": context.len(),
        "context": context,
    }))
}

pub async fn code_dependencies_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeGraphQueryPayload>,
) -> impl axum::response::IntoResponse {
    code_graph_edges_response(&state, payload, false, false).await
}

pub async fn code_reverse_dependencies_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeGraphQueryPayload>,
) -> impl axum::response::IntoResponse {
    code_graph_edges_response(&state, payload, true, false).await
}

pub async fn code_call_chain_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeGraphQueryPayload>,
) -> impl axum::response::IntoResponse {
    code_graph_edges_response(&state, payload, false, true).await
}

pub async fn code_hubs_handler(State(state): State<CliState>) -> impl axum::response::IntoResponse {
    let code_graph = state.code_graph.read().await;
    match code_graph
        .query
        .hubs(default_min_degree(), default_graph_limit())
    {
        Ok(hubs) => {
            let (items, truncated, estimated_tokens) =
                truncate_json_items(hubs, default_graph_budget());
            axum::Json(serde_json::json!({
                "status": "ok",
                "count": items.len(),
                "min_degree": default_min_degree(),
                "estimated_tokens": estimated_tokens,
                "_truncated": truncated,
                "results": items,
            }))
        }
        Err(error) => axum::Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
        })),
    }
}

pub async fn code_hotspots_handler(
    State(state): State<CliState>,
) -> impl axum::response::IntoResponse {
    let code_graph = state.code_graph.read().await;
    match code_graph
        .query
        .hotspots(default_min_complexity(), default_graph_limit())
    {
        Ok(hotspots) => {
            let (items, truncated, estimated_tokens) =
                truncate_json_items(hotspots, default_graph_budget());
            axum::Json(serde_json::json!({
                "status": "ok",
                "count": items.len(),
                "min_complexity": default_min_complexity(),
                "estimated_tokens": estimated_tokens,
                "_truncated": truncated,
                "results": items,
            }))
        }
        Err(error) => axum::Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
        })),
    }
}

async fn code_graph_edges_response(
    state: &CliState,
    payload: CodeGraphQueryPayload,
    reverse: bool,
    call_chain: bool,
) -> axum::Json<serde_json::Value> {
    let sec_result = state
        .security
        .process_input(&payload.query)
        .await
        .unwrap_or_else(|_| SecureInputResult {
            allowed: false,
            sanitized_input: None,
            original_input: payload.query.clone(),
            detection_confidence: 1.0,
            is_injection: true,
            attack_type: "unknown".to_string(),
        });

    if !sec_result.allowed {
        return axum::Json(serde_json::json!({
            "status": "blocked",
            "reason": "security_policy_violation",
            "blocked": true,
            "detection": {
                "is_injection": sec_result.is_injection,
                "confidence": sec_result.detection_confidence,
                "attack_type": sec_result.attack_type,
            }
        }));
    }

    let query = sec_result
        .sanitized_input
        .unwrap_or_else(|| sec_result.original_input.clone());
    let edge_type = if call_chain {
        Some(::code_graph::types::EdgeType::Calls)
    } else {
        match parse_code_edge_type(payload.edge_type.as_deref()) {
            Ok(edge_type) => edge_type,
            Err(message) => {
                return axum::Json(serde_json::json!({
                    "status": "error",
                    "message": message,
                }))
            }
        }
    };
    let depth = payload.depth.clamp(1, 8);
    let limit = payload.limit.clamp(1, 1000);
    let budget_tokens = payload.budget_tokens.clamp(100, 16_000);

    let code_graph = state.code_graph.read().await;
    let result = if call_chain {
        code_graph.query.call_chain(&query, depth, limit)
    } else if reverse {
        code_graph
            .query
            .reverse_dependencies(&query, edge_type, depth, limit)
    } else {
        code_graph
            .query
            .dependencies(&query, edge_type, depth, limit)
    };

    match result {
        Ok(edges) => {
            let (items, truncated, estimated_tokens) = truncate_json_items(edges, budget_tokens);
            axum::Json(serde_json::json!({
                "status": "ok",
                "query": query,
                "depth": depth,
                "limit": limit,
                "budget_tokens": budget_tokens,
                "estimated_tokens": estimated_tokens,
                "count": items.len(),
                "_truncated": truncated,
                "results": items,
            }))
        }
        Err(error) => axum::Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
        })),
    }
}

fn parse_code_edge_type(
    value: Option<&str>,
) -> std::result::Result<Option<::code_graph::types::EdgeType>, String> {
    let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    match value.to_ascii_lowercase().as_str() {
        "calls" | "call" => Ok(Some(::code_graph::types::EdgeType::Calls)),
        "defines" | "define" => Ok(Some(::code_graph::types::EdgeType::Defines)),
        "uses" | "use" => Ok(Some(::code_graph::types::EdgeType::Uses)),
        "imports" | "import" => Ok(Some(::code_graph::types::EdgeType::Imports)),
        "contains" | "contain" => Ok(Some(::code_graph::types::EdgeType::Contains)),
        "references" | "reference" | "refs" => Ok(Some(::code_graph::types::EdgeType::References)),
        _ => Err(format!("unsupported edge_type: {}", value)),
    }
}

fn truncate_json_items<T: Serialize>(
    items: Vec<T>,
    budget_tokens: usize,
) -> (Vec<serde_json::Value>, bool, usize) {
    let mut output = Vec::new();
    let mut used_tokens = 0usize;
    let mut truncated = false;

    for item in items {
        let value = serde_json::to_value(item).unwrap_or_else(|_| serde_json::json!({}));
        let estimated = estimate_tokens(&value.to_string());
        if used_tokens + estimated > budget_tokens && !output.is_empty() {
            truncated = true;
            break;
        }
        used_tokens += estimated;
        output.push(value);
    }

    (output, truncated, used_tokens)
}

#[derive(Debug, serde::Deserialize)]
pub struct CodeGraphViewQuery {
    pub mode: Option<String>,
}

pub async fn code_graph_view_handler(
    State(state): State<CliState>,
    axum::extract::Query(_query): axum::extract::Query<CodeGraphViewQuery>,
) -> impl axum::response::IntoResponse {
    let code_graph = state.code_graph.read().await;
    let symbols = code_graph.db.get_all_symbols().unwrap_or_default();
    let edges = code_graph.db.get_all_edges().unwrap_or_default();

    // Map to JSON
    let nodes: Vec<serde_json::Value> = symbols.iter().map(|s| {
        serde_json::json!({
            "id": s.stable_id.clone().unwrap_or_else(|| s.name.clone()),
            "label": s.name,
            "type": format!("{:?}", s.kind).to_lowercase(),
            "file_path": s.file_path,
        })
    }).collect();

    let links: Vec<serde_json::Value> = edges.iter().map(|e| {
        serde_json::json!({
            "source": e.from_symbol,
            "target": e.to_symbol,
            "relation": format!("{:?}", e.edge_type).to_lowercase(),
        })
    }).collect();

    axum::Json(serde_json::json!({
        "status": "ok",
        "nodes": nodes,
        "links": links,
    }))
}
