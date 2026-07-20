use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use crate::adapters::inbound::http::state::check_auth;
use crate::adapters::inbound::http::AppState;


#[derive(Debug, Deserialize)]
pub struct CodeScanPayload {
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CodeFindPayload {
    #[serde(default)]
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub pattern: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CodeContextPayload {
    #[serde(default)]
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default = "default_token_budget")]
    pub budget_tokens: usize,
    #[serde(default)]
    pub kind: Option<String>,
}

fn default_token_budget() -> usize {
    800
}

fn default_limit() -> usize {
    10
}

pub async fn code_scan_handler(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<CodeScanPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_auth(&headers, &state)?;
    let requested_path = payload.path.unwrap_or_else(|| ".".to_string());

    // Security scan on path
    let sec_result = match state.security.process_input(&requested_path).await {
        Ok(res) => res,
        Err(e) => return Ok(Json(serde_json::json!({ "status": "error", "message": e.to_string() }))),
    };

    if !sec_result.allowed {
        return Ok(Json(serde_json::json!({
            "status": "blocked",
            "reason": "security_policy_violation",
            "detection": {
                "is_injection": sec_result.is_injection,
                "confidence": sec_result.detection_confidence,
                "attack_type": sec_result.attack_type,
            }
        })));
    }

    if requested_path.contains("..") {
        return Ok(Json(serde_json::json!({
            "status": "error",
            "message": "path traversal not allowed",
            "indexed_files": 0,
        })));
    }

    // Resolve workspace allowed directory
    let allowed_workspace = std::env::var("XAVIER_WORKSPACE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default());

    let canonical_workspace = match std::fs::canonicalize(&allowed_workspace) {
        Ok(path) => path,
        Err(_) => allowed_workspace,
    };

    let target_path = std::path::PathBuf::from(&requested_path);
    let canonical_target = match std::fs::canonicalize(&target_path) {
        Ok(path) => path,
        Err(_) => {
            if target_path.is_absolute() {
                target_path
            } else {
                canonical_workspace.join(&target_path)
            }
        }
    };

    if !canonical_target.starts_with(&canonical_workspace) {
        return Ok(Json(serde_json::json!({
            "status": "blocked",
            "reason": "path_outside_workspace",
            "message": "Scanned path must reside within the allowed workspace directory."
        })));
    }

    match state.code_indexer.index(&canonical_target, true).await {
        Ok(stats) => Ok(Json(serde_json::json!({
            "status": "ok",
            "indexed_files": stats.total_files,
            "indexed_symbols": stats.total_symbols,
            "indexed_imports": stats.total_imports,
            "duration_ms": stats.duration_ms,
            "paths": [requested_path],
            "languages": stats.languages,
        }))),
        Err(error) => Ok(Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
        }))),
    }
}

pub async fn code_find_handler(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<CodeFindPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_auth(&headers, &state)?;
    let sec_result = match state.security.process_input(&payload.query).await {
        Ok(res) => res,
        Err(e) => return Ok(Json(serde_json::json!({ "status": "error", "message": e.to_string() }))),
    };

    if !sec_result.allowed {
        return Ok(Json(serde_json::json!({
            "status": "blocked",
            "reason": "security_policy_violation",
            "detection": {
                "is_injection": sec_result.is_injection,
                "confidence": sec_result.detection_confidence,
                "attack_type": sec_result.attack_type,
            }
        })));
    }

    let query = sec_result.sanitized_input.as_deref().unwrap_or(&sec_result.original_input).to_string();
    let limit = payload.limit.max(1).min(100);

    let symbols = code_find_symbols(
        &state.code_query,
        &query,
        payload.kind.as_deref(),
        payload.pattern.as_deref(),
        limit,
    );

    let results: Vec<_> = symbols
        .into_iter()
        .map(|symbol| {
            serde_json::json!({
                "id": symbol.id,
                "path": symbol.file_path,
                "symbol": symbol.name,
                "symbol_type": format!("{:?}", symbol.kind),
                "language": format!("{:?}", symbol.lang),
                "line": symbol.start_line,
                "end_line": symbol.end_line,
                "signature": symbol.signature,
                "parent": symbol.parent,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "status": "ok",
        "query": query,
        "count": results.len(),
        "results": results,
    })))
}

pub async fn code_stats_handler(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_auth(&headers, &state)?;
    match state.code_db.stats() {
        Ok(stats) => Ok(Json(serde_json::json!({
            "status": "ok",
            "total_files": stats.total_files,
            "total_symbols": stats.total_symbols,
            "total_imports": stats.total_imports,
            "languages": stats.languages,
        }))),
        Err(error) => Ok(Json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
        }))),
    }
}

pub async fn code_context_handler(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<CodeContextPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    check_auth(&headers, &state)?;
    let sec_result = match state.security.process_input(&payload.query).await {
        Ok(res) => res,
        Err(e) => return Ok(Json(serde_json::json!({ "status": "error", "message": e.to_string() }))),
    };

    if !sec_result.allowed {
        return Ok(Json(serde_json::json!({
            "status": "blocked",
            "reason": "security_policy_violation",
            "detection": {
                "is_injection": sec_result.is_injection,
                "confidence": sec_result.detection_confidence,
                "attack_type": sec_result.attack_type,
            }
        })));
    }

    let limit = payload.limit.max(1).min(100);
    let kind_limit = if payload.query.trim().is_empty() { limit } else { 10_000 };
    let budget_tokens = payload.budget_tokens.max(100).min(8000);

    let (mut symbols, is_listing) = if let Some(kind) = payload.kind.as_deref() {
        match kind.to_ascii_lowercase().as_str() {
            "function" | "fn" => (state.code_query.functions(kind_limit).unwrap_or_default(), true),
            "struct" => (state.code_query.structs(kind_limit).unwrap_or_default(), true),
            "class" => (state.code_query.classes(kind_limit).unwrap_or_default(), true),
            "enum" => (state.code_query.enums(kind_limit).unwrap_or_default(), true),
            _ => (state.code_query.search(&payload.query, limit).map(|result| result.symbols).unwrap_or_default(), false),
        }
    } else {
        (state.code_query.search(&payload.query, limit).map(|result| result.symbols).unwrap_or_default(), false)
    };

    if is_listing {
        filter_symbols_by_query(&mut symbols, &payload.query);
    }

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
        });
        let estimated = (compact.to_string().len() / 4).max(1);
        if used_tokens + estimated > budget_tokens && !context.is_empty() {
            break;
        }
        used_tokens += estimated;
        context.push(compact);
    }

    Ok(Json(serde_json::json!({
        "status": "ok",
        "query": payload.query,
        "budget_tokens": budget_tokens,
        "estimated_tokens": used_tokens,
        "count": context.len(),
        "context": context,
    })))
}

// Helper functions (copied from cli.rs)

fn code_find_symbols(
    code_query: &code_graph::query::QueryEngine,
    query: &str,
    kind: Option<&str>,
    pattern: Option<&str>,
    limit: usize,
) -> Vec<code_graph::types::Symbol> {
    let limit = limit.max(1).min(100);
    let broad_limit = if query.trim().is_empty() { limit } else { 10_000 };

    let (mut symbols, is_listing) = if let Some(pattern) = pattern.filter(|p| !p.trim().is_empty()) {
        if is_supported_code_pattern(pattern) {
            (code_query.search_by_pattern(pattern, broad_limit).unwrap_or_default(), true)
        } else {
            (search_code_symbols_with_fallback(code_query, pattern, broad_limit), false)
        }
    } else if let Some(kind) = kind.filter(|k| !k.trim().is_empty()) {
        match kind.to_ascii_lowercase().as_str() {
            "function" | "fn" => (code_query.functions(broad_limit).unwrap_or_default(), true),
            "struct" => (code_query.structs(broad_limit).unwrap_or_default(), true),
            "class" => (code_query.classes(broad_limit).unwrap_or_default(), true),
            "enum" => (code_query.enums(broad_limit).unwrap_or_default(), true),
            _ => (search_code_symbols_with_fallback(code_query, query, broad_limit), false),
        }
    } else {
        (search_code_symbols_with_fallback(code_query, query, broad_limit), false)
    };

    if is_listing {
        filter_symbols_by_query(&mut symbols, query);
    }

    symbols.truncate(limit);
    symbols
}

fn is_supported_code_pattern(pattern: &str) -> bool {
    matches!(
        pattern,
        "function_call" | "function_definition" | "struct_definition" | "struct" | "class_definition" | "class" | "enum_definition" | "enum" | "module_definition" | "module" | "import" | "use_statement"
    )
}

fn search_code_symbols_with_fallback(
    code_query: &code_graph::query::QueryEngine,
    query: &str,
    limit: usize,
) -> Vec<code_graph::types::Symbol> {
    let query = query.trim();
    let mut symbols = code_query.search(query, limit).map(|result| result.symbols).unwrap_or_default();

    if symbols.is_empty() {
        if let Some(token) = best_symbol_query_token(query) {
            if token != query {
                symbols = code_query.search(token, limit).map(|result| result.symbols).unwrap_or_default();
            }
        }
    }
    symbols
}

fn best_symbol_query_token(query: &str) -> Option<&str> {
    query
        .split(|ch: char| !(ch == '_' || ch.is_ascii_alphanumeric()))
        .filter(|token| !token.is_empty())
        .filter(|token| {
            !matches!(
                token.to_ascii_lowercase().as_str(),
                "fn" | "function" | "struct" | "class" | "enum" | "async" | "pub"
            )
        })
        .max_by_key(|token| token.len())
}

fn filter_symbols_by_query(symbols: &mut Vec<code_graph::types::Symbol>, query: &str) {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() { return; }

    let query_words: Vec<&str> = query.split_whitespace().filter(|w| w.len() >= 3).collect();

    symbols.retain(|symbol| {
        let name = symbol.name.to_ascii_lowercase();
        let sig = symbol.signature.as_deref().unwrap_or_default().to_ascii_lowercase();
        let path = symbol.file_path.to_ascii_lowercase();

        // Standard substring contains
        if name.contains(&query) || sig.contains(&query) || path.contains(&query) {
            return true;
        }

        // Fuzzy/multi-word contains
        if !query_words.is_empty() {
            return query_words.iter().any(|word| {
                name.contains(word) || sig.contains(word) || path.contains(word)
            });
        }

        false
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_graph::types::{Symbol, SymbolKind, Language};

    #[test]
    fn test_filter_symbols_by_query_strict_filtering_bug_fixed_fixed() {
        let mut symbols = vec![
            Symbol {
                id: None,
                stable_id: None,
                name: "CacheStore".to_string(),
                kind: SymbolKind::Struct,
                lang: Language::Rust,
                file_path: "src/cache.rs".to_string(),
                start_line: 1,
                end_line: 10,
                start_col: 0,
                end_col: 0,
                signature: Some("struct CacheStore".to_string()),
                parent: None,
                complexity: None,
            }
        ];

        // The query "cache store" now matches because it is split into words "cache" and "store"
        filter_symbols_by_query(&mut symbols, "cache store");
        assert_eq!(symbols.len(), 1);
    }

    #[test]
    fn test_filter_symbols_by_query_case_insensitive() {
        let mut symbols = vec![
            Symbol {
                id: None,
                stable_id: None,
                name: "CacheStore".to_string(),
                kind: SymbolKind::Struct,
                lang: Language::Rust,
                file_path: "src/cache.rs".to_string(),
                start_line: 1,
                end_line: 10,
                start_col: 0,
                end_col: 0,
                signature: Some("struct CacheStore".to_string()),
                parent: None,
                complexity: None,
            }
        ];

        filter_symbols_by_query(&mut symbols, "cache");
        assert_eq!(symbols.len(), 1);
    }
}
