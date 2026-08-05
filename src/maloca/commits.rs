//! Maloca Commit Chronicle API (MS-003)
//!
//! Exports the interconnected commit network of the active repository as a
//! graph: commit nodes linked to touched files, and files linked to the code
//! symbols indexed in the CodeGraph DB. Consumed by swal-backoffice
//! CommitGraphPage (`GET /maloca/commits/graph`).
//!
//! Contract:
//!   GET /maloca/commits/graph?limit=50&since_days=30
//!     -> { ok, nodes: [CommitNode|SymbolNode], links: [CommitFileLink|FileSymbolLink] }

use axum::extract::Query;
use axum::response::IntoResponse;
use axum::{Extension, Json};
use chrono::{DateTime, Utc};
use git2::{Repository, Sort};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::workspace::WorkspaceContext;

#[derive(Debug, Deserialize)]
pub struct CommitGraphQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default = "default_since_days")]
    pub since_days: u64,
    /// Repo root override (defaults to the process CWD / active workspace).
    #[serde(default)]
    pub repo: Option<String>,
}

fn default_limit() -> usize {
    50
}
fn default_since_days() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GraphNode {
    Commit {
        id: String,
        short_hash: String,
        hash: String,
        message: String,
        author: String,
        timestamp: String,
        files_changed: usize,
        lines_added: usize,
        lines_deleted: usize,
    },
    Symbol {
        id: String,
        label: String,
        name: String,
        symbol_type: String,
        kind: String,
        file_path: String,
        file: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GraphLink {
    CommitFile {
        source: String,
        target: String,
        insertions: usize,
        deletions: usize,
    },
    FileSymbol {
        source: String,
        target: String,
    },
}

/// Resolve the CodeGraph DB path using the shared helper (same DB the server uses).
fn code_db_path() -> std::path::PathBuf {
    crate::codebase::codegraph_paths::code_graph_db_path_for(Path::new("."))
}

/// Build the commit network graph from the active git repo + CodeGraph DB.
pub async fn commits_graph(
    Extension(_ctx): Extension<WorkspaceContext>,
    Query(q): Query<CommitGraphQuery>,
) -> impl IntoResponse {
    let repo_root = q.repo.clone().unwrap_or_else(|| {
        std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| ".".to_string())
    });

    let since = Utc::now() - chrono::Duration::days(q.since_days as i64);

    let repo = match Repository::open(&repo_root) {
        Ok(r) => r,
        Err(e) => {
            return Json(serde_json::json!({
                "ok": false,
                "error": format!("cannot open git repo at {repo_root}: {e}"),
            }))
        }
    };

    let mut revwalk = match repo.revwalk() {
        Ok(rw) => rw,
        Err(e) => {
            return Json(serde_json::json!({ "ok": false, "error": format!("revwalk: {e}") }))
        }
    };
    revwalk.set_sorting(Sort::TIME).ok();
    if revwalk.push_ref("refs/heads/main").is_err() {
        revwalk.push_head().ok();
    }

    let mut nodes: Vec<GraphNode> = Vec::new();
    let mut links: Vec<GraphLink> = Vec::new();
    let mut commit_ids: Vec<String> = Vec::new();
    let mut file_to_commit: HashMap<String, String> = HashMap::new();

    for id in revwalk {
        let Ok(oid) = id else { continue };
        let Ok(commit) = repo.find_commit(oid) else { continue };
        let commit_time = DateTime::<Utc>::from_timestamp(commit.time().seconds(), 0);
        let Some(commit_time) = commit_time else { continue };
        if commit_time < since {
            continue;
        }
        if commit_ids.len() >= q.limit {
            break;
        }

        let hash = oid.to_string();
        let short_hash: String = hash.chars().take(8).collect();
        let message = commit
            .message()
            .unwrap_or_default()
            .lines()
            .next()
            .unwrap_or_default()
            .to_string();
        let author = commit
            .author()
            .name()
            .unwrap_or("unknown")
            .to_string();
        let ts = commit_time.to_rfc3339();

        let mut files_changed = 0usize;
        let mut insertions = 0usize;
        let mut deletions = 0usize;
        let mut touched: Vec<String> = Vec::new();

        if let Ok(parent) = commit.parent(0) {
            if let (Ok(p_tree), Ok(c_tree)) = (parent.tree(), commit.tree()) {
                if let Ok(diff) = repo.diff_tree_to_tree(Some(&p_tree), Some(&c_tree), None) {
                    for delta in diff.deltas() {
                        if let Some(np) = delta.new_file().path() {
                            if let Some(p) = np.to_str() {
                                touched.push(p.to_string());
                                files_changed += 1;
                                file_to_commit.insert(p.to_string(), hash.clone());
                            }
                        }
                    }
                    if let Ok(stats) = diff.stats() {
                        insertions = stats.insertions();
                        deletions = stats.deletions();
                    }
                }
            }
        }

        nodes.push(GraphNode::Commit {
            id: hash.clone(),
            short_hash: short_hash.clone(),
            hash: hash.clone(),
            message,
            author,
            timestamp: ts,
            files_changed,
            lines_added: insertions,
            lines_deleted: deletions,
        });
        commit_ids.push(short_hash.clone());

        // Link commit -> touched files
        for f in &touched {
            links.push(GraphLink::CommitFile {
                source: hash.clone(),
                target: f.clone(),
                insertions,
                deletions,
            });
        }
    }

    // ── Connect files -> symbols from the CodeGraph DB ──────────────────
    let mut symbol_count = 0usize;
    if let Ok(db) = code_graph::db::CodeGraphDB::new(&code_db_path()) {
        // Files in the graph are stored relative to the scan root (e.g. src/).
        // Git paths are relative to the repo root; try exact match and with src/ prefix stripped.
        let mut files_done: HashMap<String, bool> = HashMap::new();
        for (file, commit_id) in &file_to_commit {
            if files_done.contains_key(file) {
                continue;
            }
            files_done.insert(file.clone(), true);
            let candidates: Vec<String> = vec![
                file.clone(),
                file.trim_start_matches("src/").to_string(),
                format!("src/{}", file.trim_start_matches("src/")),
            ];
            for cand in &candidates {
                if let Ok(symbols) = db.find_by_file(cand) {
                    for sym in symbols {
                        if let Some(stable) = &sym.stable_id {
                            let sym_id = format!("sym:{}", stable);
                            let kind_lc = format!("{:?}", sym.kind).to_lowercase();
                            nodes.push(GraphNode::Symbol {
                                id: sym_id.clone(),
                                label: sym.name.clone(),
                                name: sym.name.clone(),
                                symbol_type: kind_lc.clone(),
                                kind: kind_lc,
                                file_path: sym.file_path.clone(),
                                file: sym.file_path.clone(),
                            });
                            links.push(GraphLink::FileSymbol {
                                source: commit_id.clone(),
                                target: sym_id,
                            });
                            symbol_count += 1;
                            if symbol_count > 500 {
                                break;
                            }
                        }
                    }
                    if symbol_count > 500 {
                        break;
                    }
                }
                if symbol_count > 500 {
                    break;
                }
            }
            if symbol_count > 500 {
                break;
            }
        }
    }

    Json(serde_json::json!({
        "ok": true,
        "repo": repo_root,
        "commits": commit_ids.len(),
        "symbols": symbol_count,
        "nodes": nodes,
        "links": links,
    }))
}
