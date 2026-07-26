//! Code handlers for scanning, searching, and analyzing codebases.

use axum::extract::State;
use axum::Json;
use serde::Serialize;
use std::path::PathBuf;
use tracing::{info, warn};

use crate::cli::code_dump::{perform_dump, perform_load};
use xavier::codebase::codegraph_sidecar::{
    ensure_codegraph_sidecar_soft, maybe_sync_colby_project, EnsureOutcome,
};
// TODO: Re-implement code_find_symbols and filter_symbols_by_query via code_graph::query::QueryEngine
use crate::cli::security::secure_optional_request_field;
use crate::cli::state::CliState;
use crate::cli::types::*;
use crate::cli::utils::estimate_tokens;
use code_graph::types::{CodeEdge, EdgeType, Symbol, SymbolKind};

use xavier::ports::inbound::input_security_port::SecureInputResult;

fn ensure_sidecar_for_workspace(workspace: &std::path::Path) -> EnsureOutcome {
    ensure_codegraph_sidecar_soft(workspace)
}

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

    let sidecar = ensure_sidecar_for_workspace(&state.workspace_dir);

    let code_graph = state.code_graph.read().await;
    match code_graph
        .indexer
        .index(std::path::Path::new(base_path), true)
        .await
    {
        Ok(stats) => {
            if let Some(bin) = sidecar.bin_path.as_ref() {
                maybe_sync_colby_project(std::path::Path::new(base_path), bin);
            }

            // Automatically trigger soft dump after successful index
            let (dump_msg, dump_path_str, dump_success) = match perform_dump(&code_graph, base_path).await {
                Ok(dump_path) => (
                    format!(" (Dumped to {})", dump_path.display()),
                    Some(dump_path.to_string_lossy().into_owned()),
                    true,
                ),
                Err(e) => (
                    format!(" (Dump failed: {})", e),
                    None,
                    false,
                ),
            };

            Json(serde_json::json!({
                "status": "ok",
                "indexed_files": stats.total_files,
                "indexed_symbols": stats.total_symbols,
                "indexed_imports": stats.total_imports,
                "duration_ms": stats.duration_ms,
                "paths": [base_path],
                "languages": stats.languages,
                "codegraph_sidecar": sidecar.message,
                "codegraph_available": sidecar.available,
                "codegraph_dump_path": dump_path_str,
                "codegraph_dump_success": dump_success,
                "message": format!("Indexed {} files, {} symbols, {} imports across {:?}{}",
                    stats.total_files, stats.total_symbols, stats.total_imports, stats.languages, dump_msg),
            }))
        }
        Err(error) => Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
            "indexed_files": 0,
            "indexed_symbols": 0,
            "indexed_imports": 0,
            "paths": [base_path],
            "codegraph_sidecar": sidecar.message,
            "codegraph_available": sidecar.available,
        })),
    }
}

/// Code dump handler.
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

/// Code load handler.
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

/// Code scan handler.
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

    // Consent-first Colby sidecar (server usually non-TTY → skip/honour env). Soft-fail.
    let sidecar = ensure_sidecar_for_workspace(&state.workspace_dir);

    let code_graph = state.code_graph.read().await;
    match code_graph
        .indexer
        .index(std::path::Path::new(&path), true)
        .await
    {
        Ok(stats) => {
            if let Some(bin) = sidecar.bin_path.as_ref() {
                maybe_sync_colby_project(std::path::Path::new(&path), bin);
            }

            // Automatically trigger soft dump after successful scan
            let (dump_msg, dump_path_str, dump_success) = match perform_dump(&code_graph, &path).await {
                Ok(dump_path) => (
                    format!(" (Dumped to {})", dump_path.display()),
                    Some(dump_path.to_string_lossy().into_owned()),
                    true,
                ),
                Err(e) => (
                    format!(" (Dump failed: {})", e),
                    None,
                    false,
                ),
            };

            axum::Json(serde_json::json!({
                "status": "ok",
                "indexed_files": stats.total_files,
                "indexed_symbols": stats.total_symbols,
                "indexed_imports": stats.total_imports,
                "duration_ms": stats.duration_ms,
                "paths": [path],
                "languages": stats.languages,
                "codegraph_sidecar": sidecar.message,
                "codegraph_available": sidecar.available,
                "codegraph_dump_path": dump_path_str,
                "codegraph_dump_success": dump_success,
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
            "codegraph_sidecar": sidecar.message,
            "codegraph_available": sidecar.available,
        })),
    }
}

/// Code find handler.
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

/// Code stats handler.
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

/// Code context handler.
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

/// Code dependencies handler.
pub async fn code_dependencies_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeGraphQueryPayload>,
) -> impl axum::response::IntoResponse {
    code_graph_edges_response(&state, payload, false, false).await
}

/// Code reverse dependencies handler.
pub async fn code_reverse_dependencies_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeGraphQueryPayload>,
) -> impl axum::response::IntoResponse {
    code_graph_edges_response(&state, payload, true, false).await
}

/// Code call chain handler.
pub async fn code_call_chain_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<CodeGraphQueryPayload>,
) -> impl axum::response::IntoResponse {
    code_graph_edges_response(&state, payload, false, true).await
}

/// Code hubs handler.
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

/// Code hotspots handler.
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

/// Code graph view handler.
pub async fn code_graph_view_handler(
    State(state): State<CliState>,
    axum::extract::Query(params): axum::extract::Query<CodeGraphViewParams>,
) -> impl axum::response::IntoResponse {
    let edge_type_str = match secure_optional_request_field(
        state.security.as_ref(),
        "code/graph/view edge_type",
        params.edge_type.as_deref(),
    )
    .await
    {
        Ok(et) => et,
        Err(sec_result) => {
            return axum::Json(serde_json::json!({
                "status": "blocked",
                "reason": "security_policy_violation",
                "blocked": true,
                "field": "edge_type",
                "detection": {
                    "is_injection": sec_result.is_injection,
                    "confidence": sec_result.detection_confidence,
                    "attack_type": sec_result.attack_type,
                }
            }));
        }
    };

    let edge_type = match parse_code_edge_type(edge_type_str.as_deref()) {
        Ok(et) => et,
        Err(message) => {
            return axum::Json(serde_json::json!({
                "status": "error",
                "message": message,
            }))
        }
    };

    let depth = params.depth.clamp(1, 8);
    let limit = params.limit.clamp(1, 1000);

    let code_graph = state.code_graph.read().await;
    let total_symbols = code_graph.db.stats().map(|s| s.total_symbols).unwrap_or(0);

    let mut seed_symbols = Vec::new();
    let mut candidate_edges = Vec::new();

    if params.mode == "ego" {
        let query_param = match params.query.as_ref() {
            Some(q) => q,
            None => {
                return axum::Json(serde_json::json!({
                    "status": "error",
                    "message": "query is required for ego mode",
                }))
            }
        };

        let sec_result = state
            .security
            .process_input(query_param)
            .await
            .unwrap_or_else(|_| SecureInputResult {
                allowed: false,
                sanitized_input: None,
                original_input: query_param.clone(),
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

        if query.len() == 64 && query.chars().all(|ch| ch.is_ascii_hexdigit()) {
            if let Ok(Some(sym)) = code_graph.db.symbol_by_stable_id(&query) {
                seed_symbols.push(sym);
            }
        } else {
            if let Ok(result) = code_graph.db.find_symbols(&query, 1) {
                if let Some(sym) = result.symbols.into_iter().next() {
                    seed_symbols.push(sym);
                }
            }
        }

        let edges_res = if edge_type == Some(::code_graph::types::EdgeType::Calls) {
            code_graph.query.call_chain(&query, depth, limit)
        } else {
            code_graph
                .query
                .dependencies(&query, edge_type, depth, limit)
        };

        match edges_res {
            Ok(edges) => candidate_edges = edges,
            Err(err) => {
                return axum::Json(serde_json::json!({
                    "status": "error",
                    "message": err.to_string(),
                }))
            }
        }
    } else {
        // default to "overview"
        let hubs = match code_graph.query.hubs(params.min_degree, limit) {
            Ok(h) => h,
            Err(err) => {
                return axum::Json(serde_json::json!({
                    "status": "error",
                    "message": err.to_string(),
                }))
            }
        };

        seed_symbols = hubs.into_iter().map(|h| h.symbol).collect();

        for sym in &seed_symbols {
            if let Some(ref stable_id) = sym.stable_id {
                if let Ok(from_edges) =
                    code_graph
                        .db
                        .find_edges_from(stable_id, edge_type.clone(), limit)
                {
                    candidate_edges.extend(from_edges);
                }
                if let Ok(to_edges) =
                    code_graph
                        .db
                        .find_edges_to(stable_id, edge_type.clone(), limit)
                {
                    candidate_edges.extend(to_edges);
                }
            }
        }
    }

    let (nodes, links, truncated) = map_edges_to_graph(
        seed_symbols,
        candidate_edges,
        params.include_file_nodes,
        limit,
        &code_graph.db,
    );

    let shown_nodes = nodes.len();
    let shown_links = links.len();

    axum::Json(serde_json::json!({
        "status": "ok",
        "layer": "code",
        "truncated": truncated,
        "nodes": nodes,
        "links": links,
        "stats": {
            "total_symbols": total_symbols,
            "shown_nodes": shown_nodes,
            "shown_links": shown_links,
        }
    }))
}

fn map_edges_to_graph(
    seed_symbols: Vec<Symbol>,
    candidate_edges: Vec<CodeEdge>,
    include_file_nodes: bool,
    limit: usize,
    db: &::code_graph::db::CodeGraphDB,
) -> (Vec<serde_json::Value>, Vec<serde_json::Value>, bool) {
    let mut symbol_map: std::collections::HashMap<String, Symbol> =
        std::collections::HashMap::new();
    let mut seed_symbols_filtered = Vec::new();

    for sym in seed_symbols {
        if let Some(ref stable_id) = sym.stable_id {
            if !include_file_nodes
                && (stable_id.starts_with("file:") || stable_id.starts_with("module:"))
            {
                continue;
            }
            symbol_map.insert(stable_id.clone(), sym.clone());
            seed_symbols_filtered.push(sym);
        }
    }

    let mut filtered_edges = Vec::new();
    let mut edge_keys = std::collections::HashSet::new();

    for edge in candidate_edges {
        let from = &edge.from_symbol;
        let to = &edge.to_symbol;

        if !include_file_nodes
            && (from.starts_with("file:")
                || from.starts_with("module:")
                || to.starts_with("file:")
                || to.starts_with("module:"))
            {
                continue;
            }

        let edge_key = (
            from.clone(),
            to.clone(),
            edge.edge_type.as_str().to_string(),
        );
        if !edge_keys.insert(edge_key) {
            continue;
        }

        filtered_edges.push(edge);
    }

    let mut shown_node_ids = std::collections::HashSet::new();
    let mut final_nodes = Vec::new();

    for sym in seed_symbols_filtered {
        if let Some(ref stable_id) = sym.stable_id {
            if final_nodes.len() >= limit {
                break;
            }
            if shown_node_ids.insert(stable_id.clone()) {
                final_nodes.push(sym);
            }
        }
    }

    let mut accepted_edges = Vec::new();
    let mut pending_edges = Vec::new();

    for edge in filtered_edges {
        let from = &edge.from_symbol;
        let to = &edge.to_symbol;

        let from_in = shown_node_ids.contains(from);
        let to_in = shown_node_ids.contains(to);

        if from_in && to_in {
            accepted_edges.push(edge);
        } else if from_in || to_in {
            pending_edges.push(edge);
        }
    }

    for edge in pending_edges {
        let from = &edge.from_symbol;
        let to = &edge.to_symbol;

        let from_in = shown_node_ids.contains(from);
        let neighbor_id = if from_in { to } else { from };

        if shown_node_ids.contains(neighbor_id) {
            accepted_edges.push(edge);
            continue;
        }

        if final_nodes.len() < limit {
            let sym_opt = if let Ok(Some(sym)) = db.symbol_by_stable_id(neighbor_id) {
                Some(sym)
            } else {
                if neighbor_id.starts_with("file:") || neighbor_id.starts_with("module:") {
                    let parts: Vec<&str> = neighbor_id.splitn(2, ':').collect();
                    let name = parts.get(1).unwrap_or(&neighbor_id.as_str()).to_string();
                    let kind = if neighbor_id.starts_with("file:") {
                        SymbolKind::File
                    } else {
                        SymbolKind::Module
                    };
                    Some(Symbol {
                        id: None,
                        stable_id: Some(neighbor_id.clone()),
                        name,
                        kind,
                        lang: ::code_graph::types::Language::Unknown,
                        file_path: "".to_string(),
                        start_line: 0,
                        end_line: 0,
                        start_col: 0,
                        end_col: 0,
                        signature: None,
                        parent: None,
                        complexity: None,
                    })
                } else {
                    None
                }
            };

            if let Some(sym) = sym_opt {
                shown_node_ids.insert(neighbor_id.clone());
                final_nodes.push(sym);
                accepted_edges.push(edge);
            }
        }
    }

    let mut all_possible_node_ids = std::collections::HashSet::new();
    for id in symbol_map.keys() {
        all_possible_node_ids.insert(id.clone());
    }
    for edge in &accepted_edges {
        all_possible_node_ids.insert(edge.from_symbol.clone());
        all_possible_node_ids.insert(edge.to_symbol.clone());
    }
    let truncated = all_possible_node_ids.len() > limit;

    let nodes_json: Vec<serde_json::Value> = final_nodes
        .into_iter()
        .map(|sym| {
            let id = sym.stable_id.unwrap_or_default();
            let label = sym.name;
            let kind = format!("{:?}", sym.kind);
            let meta = serde_json::json!({
                "path": sym.file_path,
                "line": sym.start_line,
                "lang": format!("{:?}", sym.lang),
                "complexity": sym.complexity.unwrap_or(0.0),
            });
            serde_json::json!({
                "id": id,
                "label": label,
                "kind": kind,
                "meta": meta,
            })
        })
        .collect();

    let links_json: Vec<serde_json::Value> = accepted_edges
        .into_iter()
        .map(|edge| {
            serde_json::json!({
                "source": edge.from_symbol,
                "target": edge.to_symbol,
                "relation": edge.edge_type.as_str(),
                "weight": edge.confidence as f64,
            })
        })
        .collect();

    (nodes_json, links_json, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_graph::db::CodeGraphDB;
    use code_graph::types::{CodeEdge, EdgeType, Language, Symbol, SymbolKind};

    #[test]
    fn test_map_edges_to_graph() {
        let db = CodeGraphDB::in_memory().unwrap();

        let s1 = Symbol {
            id: Some(1),
            stable_id: Some("stable-1".to_string()),
            name: "func_one".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "src/one.rs".to_string(),
            start_line: 10,
            end_line: 20,
            start_col: 1,
            end_col: 1,
            signature: None,
            parent: None,
            complexity: Some(1.5),
        };

        let s2 = Symbol {
            id: Some(2),
            stable_id: Some("stable-2".to_string()),
            name: "func_two".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "src/two.rs".to_string(),
            start_line: 30,
            end_line: 40,
            start_col: 1,
            end_col: 1,
            signature: None,
            parent: None,
            complexity: Some(2.5),
        };

        db.insert_symbol(&s1).unwrap();
        db.insert_symbol(&s2).unwrap();

        let seed_symbols = vec![s1.clone()];
        let candidate_edges = vec![CodeEdge {
            id: Some(1),
            from_symbol: "stable-1".to_string(),
            to_symbol: "stable-2".to_string(),
            edge_type: EdgeType::Calls,
            file_path: "src/one.rs".to_string(),
            line: 15,
            confidence: 1.0,
            metadata: None,
        }];

        // Test with include_file_nodes = false
        let (nodes, links, truncated) =
            map_edges_to_graph(seed_symbols, candidate_edges, false, 10, &db);

        assert!(!truncated);
        assert_eq!(nodes.len(), 2);
        assert_eq!(links.len(), 1);

        assert_eq!(nodes[0]["id"], "stable-1");
        assert_eq!(nodes[0]["label"], "func_one");
        assert_eq!(nodes[0]["kind"], "Function");
        assert_eq!(nodes[0]["meta"]["complexity"], 1.5);

        assert_eq!(nodes[1]["id"], "stable-2");
        assert_eq!(nodes[1]["label"], "func_two");
        assert_eq!(nodes[1]["kind"], "Function");
        assert_eq!(nodes[1]["meta"]["complexity"], 2.5);

        assert_eq!(links[0]["source"], "stable-1");
        assert_eq!(links[0]["target"], "stable-2");
        assert_eq!(links[0]["relation"], "Calls");
        assert_eq!(links[0]["weight"], 1.0);
    }
}
