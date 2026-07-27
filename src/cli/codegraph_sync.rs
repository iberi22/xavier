//! Git-driven CodeGraph incremental sync.
//!
//! Pipeline: `git diff --name-status` → path deltas → [`Indexer::apply_paths`]
//! → soft-dump `.xavier/codegraph.json` → checkpoint HEAD.
//!
//! Checkpoint file: `.xavier/codegraph-sync-commit`

use anyhow::{anyhow, bail, Context, Result};
use code_graph::db::CodeGraphDB;
use code_graph::indexer::{Indexer, PathChange, PathChangeKind};
use code_graph::query::QueryEngine;
use code_graph::types::IndexStats;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use tracing::info;

use crate::cli::code_dump::{perform_dump, DUMP_SOFT_SKIP_SYMBOLS};
use crate::cli::config::code_graph_db_path;
use crate::cli::state::CodeGraphState;

pub const SYNC_CHECKPOINT_FILE: &str = "codegraph-sync-commit";

#[derive(Debug, Clone)]
pub struct GitSyncOptions {
    pub workspace: PathBuf,
    pub base: Option<String>,
    pub staged: bool,
    /// Soft-upsert symbol summaries into Xavier memory (HTTP).
    pub with_memory: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitSyncResult {
    pub status: String,
    pub mode: String,
    pub base: String,
    pub head: String,
    pub changed_paths: usize,
    pub changes: Vec<String>,
    pub indexed_files: u64,
    pub indexed_symbols: u64,
    pub indexed_imports: u64,
    pub duration_ms: u64,
    pub dump_path: Option<String>,
    pub checkpoint: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_upserts: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dump_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dump_skipped: Option<String>,
}

/// Run git-driven sync against the workspace CodeGraph DB (local, no HTTP).
pub async fn sync_codegraph_from_git(opts: GitSyncOptions) -> Result<GitSyncResult> {
    let workspace = std::path::absolute(&opts.workspace).unwrap_or(opts.workspace.clone());
    let repo_root = find_git_root(&workspace)
        .ok_or_else(|| anyhow!("No hay repositorio git en {}", workspace.display()))?;

    let head = git_rev_parse(&repo_root, "HEAD")?;
    // Prefer the same DB path the HTTP server uses for this workspace.
    let db_path = code_graph_db_path();
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let db = Arc::new(
        CodeGraphDB::new(&db_path)
            .with_context(|| format!("No se pudo abrir CodeGraph DB en {}", db_path.display()))?,
    );
    let indexer = Arc::new(Indexer::new(Arc::clone(&db)));
    let query = Arc::new(QueryEngine::new(Arc::clone(&db)));
    let state = CodeGraphState {
        db: Arc::clone(&db),
        indexer: Arc::clone(&indexer),
        query,
    };

    let result = sync_codegraph_with_state(&state, &repo_root, &opts).await?;
    // Ensure head/checkpoint reflect the local HEAD we resolved up-front.
    let _ = head;
    Ok(result)
}

/// Apply path deltas using an already-open [`CodeGraphState`] (CLI local or HTTP).
pub async fn sync_codegraph_with_state(
    state: &CodeGraphState,
    workspace: &Path,
    opts: &GitSyncOptions,
) -> Result<GitSyncResult> {
    let workspace = std::path::absolute(workspace).unwrap_or_else(|_| workspace.to_path_buf());
    let repo_root = find_git_root(&workspace)
        .ok_or_else(|| anyhow!("No hay repositorio git en {}", workspace.display()))?;

    let head = git_rev_parse(&repo_root, "HEAD")?;
    let stats_before = state.db.stats()?;
    let empty_graph = stats_before.total_symbols == 0 || stats_before.total_files == 0;

    let (mode, base_label, changes, stats) = if empty_graph {
        info!(
            "CodeGraph vacío — escaneo completo de {}",
            repo_root.display()
        );
        let stats = state.indexer.index(&repo_root, true).await?;
        (
            "full_scan".to_string(),
            "empty".to_string(),
            Vec::<PathChange>::new(),
            stats,
        )
    } else {
        let base = resolve_base_commit(&repo_root, opts.base.as_deref(), opts.staged)?;
        let changes = git_name_status(&repo_root, &base, opts.staged)?;
        let stats = if changes.is_empty() {
            let mut s = state.db.stats()?;
            s.duration_ms = 0;
            s
        } else {
            state.indexer.apply_paths(&repo_root, &changes).await?
        };
        ("git_delta".to_string(), base, changes, stats)
    };

    // Soft-dump: skip huge graphs by default (HTTP sync was hanging ~minutes on
    // pretty JSON of 60k symbols). Dump still available via `xavier code dump`.
    let (dump_path, dump_error, dump_skipped, status) = if stats.total_symbols > DUMP_SOFT_SKIP_SYMBOLS
    {
        let reason = format!(
            "omitido: {} símbolos > umbral {} (usa `xavier code dump`)",
            stats.total_symbols, DUMP_SOFT_SKIP_SYMBOLS
        );
        tracing::info!("{}", reason);
        (None, None, Some(reason), "ok".to_string())
    } else {
        match tokio::time::timeout(
            std::time::Duration::from_secs(45),
            perform_dump(state, repo_root.to_str().unwrap_or(".")),
        )
        .await
        {
            Ok(Ok(p)) => (Some(p), None, None, "ok".to_string()),
            Ok(Err(e)) => {
                tracing::warn!("Soft-dump CodeGraph falló: {}", e);
                (
                    None,
                    Some(e.to_string()),
                    None,
                    "degraded".to_string(),
                )
            }
            Err(_) => {
                let msg = "Soft-dump CodeGraph timeout (45s)".to_string();
                tracing::warn!("{}", msg);
                (None, Some(msg), None, "degraded".to_string())
            }
        }
    };
    write_checkpoint(&repo_root, &head)?;

    let change_labels: Vec<String> = changes.iter().map(format_change).collect();
    let memory_upserts = if opts.with_memory {
        match upsert_symbol_memories(&repo_root, state, &changes).await {
            Ok(n) => Some(n),
            Err(e) => {
                tracing::warn!("Memory bridge (symbol chunks) falló: {}", e);
                Some(0)
            }
        }
    } else {
        None
    };

    Ok(GitSyncResult {
        status,
        mode,
        base: base_label,
        head,
        changed_paths: change_labels.len(),
        changes: change_labels,
        indexed_files: stats.total_files,
        indexed_symbols: stats.total_symbols,
        indexed_imports: stats.total_imports,
        duration_ms: stats.duration_ms,
        dump_path: dump_path.map(|p| p.display().to_string()),
        checkpoint: checkpoint_path(&repo_root).display().to_string(),
        message: format_sync_message(&stats, empty_graph),
        memory_upserts,
        dump_error,
        dump_skipped,
    })
}

const MEMORY_SYMBOL_CAP: usize = 80;

/// Soft-upsert short symbol cards into Xavier memory for changed files.
async fn upsert_symbol_memories(
    repo_root: &Path,
    state: &CodeGraphState,
    changes: &[PathChange],
) -> Result<usize> {
    use crate::cli::commands::enums::CLI_HTTP_CLIENT;
    use crate::cli::config::{require_xavier_token, resolve_base_url};

    let token = require_xavier_token()?;
    let base_url = resolve_base_url();
    let client = CLI_HTTP_CLIENT.clone();
    let repo_name = repo_root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("repo");

    let mut paths: Vec<String> = Vec::new();
    for c in changes {
        match &c.kind {
            PathChangeKind::Added | PathChangeKind::Modified => {
                paths.push(c.path.replace('\\', "/"));
            }
            PathChangeKind::Renamed { .. } => {
                paths.push(c.path.replace('\\', "/"));
            }
            PathChangeKind::Deleted => {}
        }
    }
    paths.sort();
    paths.dedup();

    let mut upserts = 0usize;
    for path in paths {
        let symbols = state.db.find_by_file(&path).unwrap_or_default();
        for sym in symbols.into_iter().take(MEMORY_SYMBOL_CAP.saturating_sub(upserts)) {
            let stable = sym
                .stable_id
                .clone()
                .unwrap_or_else(|| sym.deterministic_id("default"));
            let mem_path = format!("code/{}/{}", repo_name, &stable[..16.min(stable.len())]);
            let content = format!(
                "symbol={} kind={:?} file={} lines={}-{} parent={} signature={}\nstable_id={}",
                sym.name,
                sym.kind,
                sym.file_path,
                sym.start_line,
                sym.end_line,
                sym.parent.as_deref().unwrap_or("-"),
                sym.signature.as_deref().unwrap_or("-"),
                stable
            );
            let body = serde_json::json!({
                "path": mem_path,
                "content": content,
                "metadata": {
                    "kind": "code_symbol",
                    "evidence_kind": "codegraph",
                    "file": sym.file_path,
                    "stable_id": stable
                }
            });
            let url = format!("{}/memory/add", base_url.trim_end_matches('/'));
            let resp = client
                .post(&url)
                .header("X-Xavier-Token", &token)
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await;
            match resp {
                Ok(r) if r.status().is_success() => upserts += 1,
                Ok(r) => {
                    tracing::debug!("memory/add {} → {}", mem_path, r.status());
                }
                Err(e) => {
                    tracing::debug!("memory/add error: {}", e);
                    break;
                }
            }
            if upserts >= MEMORY_SYMBOL_CAP {
                break;
            }
        }
        if upserts >= MEMORY_SYMBOL_CAP {
            break;
        }
    }
    Ok(upserts)
}

fn format_change(c: &PathChange) -> String {
    match &c.kind {
        PathChangeKind::Added => format!("A\t{}", c.path),
        PathChangeKind::Modified => format!("M\t{}", c.path),
        PathChangeKind::Deleted => format!("D\t{}", c.path),
        PathChangeKind::Renamed { from } => format!("R\t{}\t{}", from, c.path),
    }
}

fn format_sync_message(stats: &IndexStats, was_empty: bool) -> String {
    if was_empty {
        format!(
            "CodeGraph vacío: escaneo completo → {} archivos, {} símbolos",
            stats.total_files, stats.total_symbols
        )
    } else {
        format!(
            "Sync git aplicado → {} archivos, {} símbolos en el grafo",
            stats.total_files, stats.total_symbols
        )
    }
}

pub fn checkpoint_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".xavier").join(SYNC_CHECKPOINT_FILE)
}

pub fn read_checkpoint(repo_root: &Path) -> Option<String> {
    let path = checkpoint_path(repo_root);
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn write_checkpoint(repo_root: &Path, head: &str) -> Result<()> {
    let xavier_dir = repo_root.join(".xavier");
    std::fs::create_dir_all(&xavier_dir)?;
    std::fs::write(checkpoint_path(repo_root), format!("{}\n", head.trim()))?;
    Ok(())
}

fn resolve_base_commit(repo_root: &Path, explicit: Option<&str>, staged: bool) -> Result<String> {
    if let Some(base) = explicit {
        return Ok(base.to_string());
    }
    if staged {
        return Ok("INDEX".to_string());
    }
    if let Some(cp) = read_checkpoint(repo_root) {
        return Ok(cp);
    }
    git_rev_parse(repo_root, "HEAD~1").or_else(|_| git_rev_parse(repo_root, "HEAD"))
}

pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = std::path::absolute(start).unwrap_or_else(|_| start.to_path_buf());
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

fn git_rev_parse(repo_root: &Path, rev: &str) -> Result<String> {
    let out = Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(repo_root)
        .output()
        .with_context(|| format!("git rev-parse {} falló", rev))?;
    if !out.status.success() {
        bail!(
            "git rev-parse {} falló: {}",
            rev,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Parse `git diff --name-status` into [`PathChange`] list.
pub fn git_name_status(repo_root: &Path, base: &str, staged: bool) -> Result<Vec<PathChange>> {
    let output = if staged {
        Command::new("git")
            .args(["diff", "--name-status", "-z", "--cached"])
            .current_dir(repo_root)
            .output()
            .context("git diff --cached --name-status falló")?
    } else {
        Command::new("git")
            .args(["diff", "--name-status", "-z", base, "HEAD"])
            .current_dir(repo_root)
            .output()
            .context("git diff --name-status falló")?
    };
    if !output.status.success() {
        bail!(
            "git diff falló: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    parse_name_status_z(&output.stdout).map(|changes| {
        changes
            .into_iter()
            .filter(|c| should_index_path(&c.path))
            .filter(|c| match &c.kind {
                PathChangeKind::Renamed { from } => should_index_path(from) || should_index_path(&c.path),
                _ => true,
            })
            .collect()
    })
}

fn should_index_path(path: &str) -> bool {
    let p = path.replace('\\', "/");
    if p.starts_with(".xavier/") || p.contains("/.xavier/") {
        return false;
    }
    if p.ends_with(".db") || p.ends_with(".db-wal") || p.ends_with(".db-shm") {
        return false;
    }
    // Skip common non-source noise; Indexer still filters by language extension.
    if p.starts_with("target/") || p.starts_with("target_local/") || p.starts_with("node_modules/") {
        return false;
    }
    true
}

/// Parse NUL-separated `git diff -z --name-status` output.
pub fn parse_name_status_z(raw: &[u8]) -> Result<Vec<PathChange>> {
    let text = String::from_utf8_lossy(raw);
    let parts: Vec<&str> = text.split('\0').filter(|s| !s.is_empty()).collect();
    let mut changes = Vec::new();
    let mut i = 0;
    while i < parts.len() {
        let status = parts[i];
        i += 1;
        if status.is_empty() {
            continue;
        }
        let code = status.chars().next().unwrap_or('M');
        match code {
            'A' | 'C' => {
                let path = parts.get(i).copied().unwrap_or("");
                i += 1;
                if !path.is_empty() {
                    changes.push(PathChange::added(path));
                }
            }
            'M' | 'T' => {
                let path = parts.get(i).copied().unwrap_or("");
                i += 1;
                if !path.is_empty() {
                    changes.push(PathChange::modified(path));
                }
            }
            'D' => {
                let path = parts.get(i).copied().unwrap_or("");
                i += 1;
                if !path.is_empty() {
                    changes.push(PathChange::deleted(path));
                }
            }
            'R' => {
                let from = parts.get(i).copied().unwrap_or("");
                let to = parts.get(i + 1).copied().unwrap_or("");
                i += 2;
                if !from.is_empty() && !to.is_empty() {
                    changes.push(PathChange::renamed(from, to));
                }
            }
            _ => {
                if let Some(path) = parts.get(i).copied() {
                    i += 1;
                    if !path.is_empty() {
                        changes.push(PathChange::modified(path));
                    }
                }
            }
        }
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_name_status_z_basic() {
        let raw = b"A\0src/new.rs\0M\0src/old.rs\0D\0src/gone.rs\0R100\0src/a.rs\0src/b.rs\0";
        let changes = parse_name_status_z(raw).expect("parse");
        assert_eq!(changes.len(), 4);
        assert_eq!(changes[0], PathChange::added("src/new.rs"));
        assert_eq!(changes[1], PathChange::modified("src/old.rs"));
        assert_eq!(changes[2], PathChange::deleted("src/gone.rs"));
        assert_eq!(changes[3], PathChange::renamed("src/a.rs", "src/b.rs"));
    }

    #[test]
    fn skips_xavier_and_db_paths() {
        assert!(!should_index_path(".xavier/codegraph.json"));
        assert!(!should_index_path("data/code_graph.db"));
        assert!(should_index_path("src/main.rs"));
    }
}
