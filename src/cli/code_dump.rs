use crate::cli::state::CodeGraphState;
use anyhow::{anyhow, Result};
use code_graph::types::{CodeEdge, Symbol};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::info;

#[derive(Debug, Serialize, Deserialize)]
pub struct CodeGraphMeta {
    pub repo: String,
    pub scanned_at: String,
    pub total_files: u64,
    pub total_symbols: u64,
    pub total_edges: u64,
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CodeGraphDump {
    pub _meta: CodeGraphMeta,
    pub symbols: Vec<Symbol>,
    pub edges: Vec<CodeEdge>,
    pub hotspots: Vec<code_graph::types::ComplexityHotspot>,
    pub hubs: Vec<code_graph::types::HubNode>,
}

/// Perform a dump of the code graph to .xavier/codegraph.json
pub async fn perform_dump(state: &CodeGraphState, scanned_path: &str) -> Result<PathBuf> {
    let stats = state.db.stats()?;
    let symbols = state.db.get_all_symbols()?;
    let edges = state.db.get_all_edges()?;
    let hotspots = state.query.hotspots(0.0, 1000)?;
    let hubs = state.query.hubs(0, 1000)?;

    let repo_root = find_repo_root(scanned_path);
    let xavier_dir = repo_root.join(".xavier");
    if !xavier_dir.exists() {
        std::fs::create_dir_all(&xavier_dir)?;
    }

    let dump_path = xavier_dir.join("codegraph.json");
    let repo_name = repo_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let dump = CodeGraphDump {
        _meta: CodeGraphMeta {
            repo: repo_name,
            scanned_at: chrono::Utc::now().to_rfc3339(),
            total_files: stats.total_files,
            total_symbols: stats.total_symbols,
            total_edges: edges.len() as u64,
            version: "1.0".to_string(),
        },
        symbols,
        edges,
        hotspots,
        hubs,
    };

    let json = serde_json::to_string_pretty(&dump)?;
    std::fs::write(&dump_path, json)?;

    info!(
        "Code graph dumped to {} ({} symbols, {} edges)",
        dump_path.display(),
        dump.symbols.len(),
        dump.edges.len()
    );

    Ok(dump_path)
}

/// Perform a load of the code graph from .xavier/codegraph.json into an in-memory DB
pub async fn perform_load(repo_path: &str) -> Result<CodeGraphState> {
    let repo_root = find_repo_root(repo_path);
    let dump_path = repo_root.join(".xavier").join("codegraph.json");

    if !dump_path.exists() {
        return Err(anyhow!(
            "No code graph dump found at {}",
            dump_path.display()
        ));
    }

    let json = std::fs::read_to_string(&dump_path)?;
    let dump: CodeGraphDump = serde_json::from_str(&json)?;

    let db = Arc::new(code_graph::db::CodeGraphDB::in_memory()?);
    db.insert_symbols(&dump.symbols)?;
    db.insert_edges(&dump.edges)?;

    let indexer = Arc::new(code_graph::indexer::Indexer::new(Arc::clone(&db)));
    let query = Arc::new(code_graph::query::QueryEngine::new(Arc::clone(&db)));

    info!(
        "Code graph loaded from {} ({} symbols, {} edges)",
        dump_path.display(),
        dump.symbols.len(),
        dump.edges.len()
    );

    Ok(CodeGraphState { db, indexer, query })
}

fn find_repo_root(start_path: &str) -> PathBuf {
    let mut current = std::path::absolute(start_path)
        .unwrap_or_else(|_| PathBuf::from(start_path));

    loop {
        if current.join(".git").exists() {
            return current;
        }
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => break,
        }
    }

    // Fallback to current directory if no .git found
    std::path::absolute(".").unwrap_or_else(|_| PathBuf::from("."))
}
