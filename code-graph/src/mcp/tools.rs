//! Tool handlers for MCP

use crate::impact::ImpactAnalyzer;
use crate::indexer::Indexer;
use crate::mcp::context_builder::ContextBuilder;
use crate::query::QueryEngine;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

pub async fn handle_codegraph_search(
    query_engine: Arc<QueryEngine>,
    arguments: Value,
) -> anyhow::Result<Value> {
    let query = arguments.get("query").and_then(|v| v.as_str()).unwrap_or("");
    let limit = arguments.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

    let result = query_engine.search(query, limit)?;

    let symbols: Vec<Value> = result.symbols.into_iter().map(|s| {
        json!({
            "name": s.name,
            "kind": format!("{:?}", s.kind),
            "file": s.file_path,
            "line": s.start_line,
            "signature": s.signature,
            "stable_id": s.stable_id
        })
    }).collect();

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string_pretty(&json!({
                "symbols": symbols,
                "count": result.total
            }))?
        }]
    }))
}

pub async fn handle_codegraph_explore(
    query_engine: Arc<QueryEngine>,
    indexer: Arc<Indexer>,
    root_path: &Path,
    arguments: Value,
) -> anyhow::Result<Value> {
    let symbols_queries = arguments.get("symbols").and_then(|v| v.as_array());
    let depth = arguments.get("depth").and_then(|v| v.as_u64()).unwrap_or(2) as usize;
    let max_chars = arguments.get("max_chars").and_then(|v| v.as_u64()).unwrap_or(8000) as usize;

    let mut resolved_symbols = Vec::new();
    let mut impact_analyses = Vec::new();
    let impact_analyzer = ImpactAnalyzer::new(query_engine.db());

    if let Some(queries) = symbols_queries {
        for q in queries {
            if let Some(query_str) = q.as_str() {
                let search_result = query_engine.search(query_str, 1)?;
                if let Some(sym) = search_result.symbols.into_iter().next() {
                    if let Some(ref id) = sym.stable_id {
                        let impact = impact_analyzer.analyze(id, depth)?;
                        impact_analyses.push(impact);
                    }
                    resolved_symbols.push(sym);
                }
            }
        }
    }

    let stale_files = indexer.get_stale_files(root_path)?;
    let builder = ContextBuilder::new(max_chars, stale_files);
    let context = builder.build_surgical_context(resolved_symbols, impact_analyses);

    Ok(json!({
        "content": [{
            "type": "text",
            "text": context
        }]
    }))
}
