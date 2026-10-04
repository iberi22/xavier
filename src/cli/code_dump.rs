use crate::cli::state::CodeGraphState;
use anyhow::{anyhow, Result};
use code_graph::db::CodeGraphDB;
use code_graph::query::QueryEngine;
use code_graph::types::{CodeEdge, Symbol};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{info, warn};

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

/// Soft-dump threshold: above this, sync skips dump by default (avoids long stalls).
pub const DUMP_SOFT_SKIP_SYMBOLS: u64 = 25_000;

/// Soft wrapper over perform_dump that never panics and logs warnings on error.
pub async fn soft_perform_dump(state: &CodeGraphState, scanned_path: &str) -> Option<PathBuf> {
    match perform_dump(state, scanned_path).await {
        Ok(path) => Some(path),
        Err(e) => {
            warn!("Soft CodeGraph dump failed: {}", e);
            None
        }
    }
}

/// Perform a dump of the code graph to `.xavier/codegraph.json`.
///
/// Heavy work runs in `spawn_blocking` so HTTP handlers stay responsive.
/// Uses compact JSON (not pretty) for large graphs.
pub async fn perform_dump(state: &CodeGraphState, scanned_path: &str) -> Result<PathBuf> {
    let db = Arc::clone(&state.db);
    let query = Arc::clone(&state.query);
    let scanned = scanned_path.to_string();
    tokio::task::spawn_blocking(move || perform_dump_blocking(db, query, &scanned))
        .await
        .map_err(|e| anyhow!("CodeGraph dump task join error: {}", e))?
}

fn perform_dump_blocking(
    db: Arc<CodeGraphDB>,
    query: Arc<QueryEngine>,
    scanned_path: &str,
) -> Result<PathBuf> {
    let stats = db.stats()?;
    let symbols = db.get_all_symbols()?;
    let edges = db.get_all_edges()?;
    let hotspots = query.hotspots(0.0, 100)?;
    let hubs = query.hubs(0, 100)?;

    let dump_path = codegraph_dump_path_for_target(scanned_path);
    if let Some(parent) = dump_path.parent() {
        let parent_path: &Path = parent;
        if !parent_path.exists() {
            std::fs::create_dir_all(parent_path)?;
        }
    }

    // `<root>/.xavier/codegraph.json` — the root is two levels up.
    let repo_root = dump_path.parent().and_then(|p| p.parent());
    let repo_name = repo_root
        .and_then(|root| root.file_name())
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

    // Compact JSON — pretty-print of 60k+ symbols stalls for minutes.
    let json = serde_json::to_string(&dump)?;
    std::fs::write(&dump_path, json)?;

    info!(
        "Code graph dumped to {} ({} symbols, {} edges)",
        dump_path.display(),
        dump.symbols.len(),
        dump.edges.len()
    );

    Ok(dump_path)
}

/// Perform a load of the code graph from .xavier/codegraph.json into the per-repo database.
pub async fn perform_load(repo_path: &str) -> Result<CodeGraphState> {
    let dump_path = codegraph_dump_path_for_target(repo_path);

    if !dump_path.exists() {
        return Err(anyhow!(
            "No code graph dump found at {}",
            dump_path.display()
        ));
    }

    let json = tokio::fs::read_to_string(&dump_path).await?;
    let dump: CodeGraphDump = serde_json::from_str(&json)?;

    let repo_root = find_repo_root(repo_path);
    let canonical = repo_root.canonicalize().unwrap_or(repo_root);
    let db_path = xavier::codebase::repo_identity::code_graph_db_path_for_root(&canonical);

    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let db = Arc::new(
        crate::codebase::connection_manager::ConnectionManager::global()
            .get_code_graph_db(&db_path)?,
    );
    db.insert_symbols(&dump.symbols)?;
    db.insert_edges(&dump.edges)?;

    let indexer = Arc::new(code_graph::indexer::Indexer::new(Arc::clone(&db)));
    let query = Arc::new(code_graph::query::QueryEngine::new(Arc::clone(&db)));

    if let Some(head) = xavier::codebase::repo_identity::git_head(&canonical) {
        let checkpoint = canonical
            .join(".xavier")
            .join(xavier::codebase::repo_identity::SYNC_CHECKPOINT_FILE);
        let _ = std::fs::write(checkpoint, format!("{}\n", head));
    }

    info!(
        "Code graph loaded from {} to {} ({} symbols, {} edges)",
        dump_path.display(),
        db_path.display(),
        dump.symbols.len(),
        dump.edges.len()
    );

    Ok(CodeGraphState { db, indexer, query })
}

fn find_repo_root(start_path: &str) -> PathBuf {
    let start = std::path::absolute(start_path).unwrap_or_else(|_| PathBuf::from(start_path));
    let mut current = start.clone();

    loop {
        if current.join(".git").exists() {
            return current;
        }
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => break,
        }
    }

    // No `.git` above the target: the target itself IS the root. Falling
    // back to the process cwd here is what made `dump` silently ignore its
    // path argument (the daemon's cwd is its own workspace, not the
    // caller's).
    start
}

/// Single source of truth for the portable dump location of `target`.
///
/// Writer ([`perform_dump`]) and reader ([`perform_load`]) both resolve
/// through this, so a dump can never be written where it is not read.
pub fn codegraph_dump_path_for_target(target: &str) -> PathBuf {
    xavier::codebase::codegraph_paths::codegraph_dump_path_for(&find_repo_root(target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_soft_perform_dump_never_panics() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let git_dir = temp_dir.path().join(".git");
        std::fs::create_dir(&git_dir).expect("failed to create .git");

        // Create .xavier as a file (not a directory) so creating or writing inside it fails
        let xavier_file = temp_dir.path().join(".xavier");
        std::fs::write(&xavier_file, "not a directory").expect("failed to write .xavier file");

        // Build an in-memory CodeGraphDB that is valid
        let db = Arc::new(CodeGraphDB::in_memory().unwrap());
        let indexer = Arc::new(code_graph::indexer::Indexer::new(Arc::clone(&db)));
        let query = Arc::new(QueryEngine::new(Arc::clone(&db)));
        let state = CodeGraphState { db, indexer, query };

        // Test that calling soft_perform_dump with a path inside temp_dir returns None instead of panicking
        let result = soft_perform_dump(&state, temp_dir.path().to_str().unwrap()).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_perform_dump_fails_when_unwritable() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let git_dir = temp_dir.path().join(".git");
        std::fs::create_dir(&git_dir).expect("failed to create .git");

        // Create .xavier as a file (not a directory) so creating or writing inside it fails
        let xavier_file = temp_dir.path().join(".xavier");
        std::fs::write(&xavier_file, "not a directory").expect("failed to write .xavier file");

        // Build an in-memory CodeGraphDB that is valid
        let db = Arc::new(CodeGraphDB::in_memory().unwrap());
        let indexer = Arc::new(code_graph::indexer::Indexer::new(Arc::clone(&db)));
        let query = Arc::new(QueryEngine::new(Arc::clone(&db)));
        let state = CodeGraphState { db, indexer, query };

        // Test that calling perform_dump directly returns an Err
        let result = perform_dump(&state, temp_dir.path().to_str().unwrap()).await;
        assert!(result.is_err());
    }
}
