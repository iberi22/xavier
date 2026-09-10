//! Explicit per-repo identity for CodeGraph queries (XAV-01).
//!
//! The `code stats` / `code find` HTTP endpoints used to resolve the graph
//! from the daemon's startup `workspace_dir`, so two different checkouts
//! (e.g. `apps/xavier` vs `cores/edge-mesh`) returned the same totals.
//! The CLI now derives a [`RepoIdentity`] from its own cwd
//! (`project_id` + `root` + `indexed_commit`) and sends it on every
//! stats/find request; the server resolves the graph anchored at that
//! `root` and never falls back to another repo's graph.
//!
//! A missing or empty index for the requested repo is reported as
//! `degraded` **for that repo** (with its own identity echoed back),
//! never by substituting a different repo's data.
//!
//! Indexer file exclusions (`target/`, `node_modules/`, `dist/`, gitignore…)
//! live in `code-graph/src/indexer` and are intentionally NOT duplicated here.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::validate_project_id;

/// Checkpoint file written by `code sync --git` after each successful sync.
/// Its trimmed content is the commit the on-disk index corresponds to.
pub const SYNC_CHECKPOINT_FILE: &str = "codegraph-sync-commit";

/// Sentinel used when no indexed commit is known (no checkpoint file and no
/// git HEAD). Never invent a commit hash.
pub const UNKNOWN_COMMIT: &str = "unknown";

/// Explicit identity of the repository a CodeGraph query targets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoIdentity {
    /// Sanitized repo dir name (`[A-Za-z0-9_-]`, fallback `"default"`).
    pub project_id: String,
    /// Canonical absolute repo root (git root when inside a git checkout).
    pub root: String,
    /// Commit the index corresponds to: `.xavier/codegraph-sync-commit`
    /// content, else current git HEAD, else `"unknown"`.
    pub indexed_commit: String,
}

/// Derive a filesystem-safe `project_id` from a repo root.
///
/// Mirrors `CodebaseDb::open` sanitization: file name with every character
/// outside `[alphanumeric, '_', '-']` replaced by `'_'`, falling back to
/// `"default"` when empty or invalid.
pub fn derive_project_id(root: &Path) -> String {
    let raw = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("default");
    let sanitized: String = raw
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() || validate_project_id(&sanitized).is_err() {
        "default".to_string()
    } else {
        sanitized
    }
}

/// Walk up from `start` looking for a `.git` entry (dir or worktree file).
/// Falls back to the absolute `start` path when no git root is found.
pub fn find_repo_root(start: &Path) -> PathBuf {
    let mut current = std::path::absolute(start).unwrap_or_else(|_| start.to_path_buf());
    loop {
        if current.join(".git").exists() {
            return current;
        }
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => break,
        }
    }
    std::path::absolute(start).unwrap_or_else(|_| start.to_path_buf())
}

/// Current git `HEAD` for `root`, or `None` outside a git checkout / on error.
pub fn git_head(root: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["-C", &root.to_string_lossy(), "rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let head = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if head.is_empty() {
        None
    } else {
        Some(head)
    }
}

/// Commit the on-disk index corresponds to: trimmed
/// `<root>/.xavier/codegraph-sync-commit`, else current git HEAD,
/// else `"unknown"`.
pub fn read_indexed_commit(root: &Path) -> String {
    let checkpoint = root.join(".xavier").join(SYNC_CHECKPOINT_FILE);
    if let Ok(raw) = std::fs::read_to_string(&checkpoint) {
        let trimmed = raw.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    git_head(root).unwrap_or_else(|| UNKNOWN_COMMIT.to_string())
}

/// Derive the [`RepoIdentity`] for the caller's cwd (or any path inside a repo).
pub fn derive_repo_identity(cwd: &Path) -> RepoIdentity {
    let root = find_repo_root(cwd);
    let canonical = root.canonicalize().unwrap_or(root);
    RepoIdentity {
        project_id: derive_project_id(&canonical),
        root: canonical.to_string_lossy().into_owned(),
        indexed_commit: read_indexed_commit(&canonical),
    }
}

/// Canonical per-repo CodeGraph SQLite path: `<root>/.xavier/code_graph.db`.
///
/// Deliberately independent of the daemon's cwd and of the
/// `XAVIER_CODE_GRAPH_DB_PATH` override so that two checkouts can never
/// resolve to the same graph file.
pub fn code_graph_db_path_for_root(root: &Path) -> PathBuf {
    root.join(".xavier").join("code_graph.db")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codebase::connection_manager::ConnectionManager;

    fn test_symbol(name: &str, file_path: &str) -> code_graph::types::Symbol {
        code_graph::types::Symbol {
            id: None,
            stable_id: None,
            name: name.to_string(),
            kind: code_graph::types::SymbolKind::Function,
            lang: code_graph::types::Language::Rust,
            file_path: file_path.to_string(),
            start_line: 1,
            end_line: 2,
            start_col: 0,
            end_col: 0,
            signature: None,
            parent: None,
            complexity: None,
        }
    }

    #[test]
    fn derive_project_id_sanitizes_like_codebase_db() {
        assert_eq!(
            derive_project_id(Path::new("/tmp/my-project")),
            "my-project"
        );
        assert_eq!(
            derive_project_id(Path::new("/tmp/my.project")),
            "my_project"
        );
        assert_eq!(derive_project_id(Path::new("/tmp/bad/name")), "name");
        assert_eq!(derive_project_id(Path::new("/")), "default");
        for pid in [
            derive_project_id(Path::new("/tmp/repo-alpha-xav01")),
            derive_project_id(Path::new("/tmp/repo-beta-xav01")),
        ] {
            assert!(validate_project_id(&pid).is_ok());
        }
    }

    #[test]
    fn derive_repo_identity_reports_root_and_checkpoint() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("repo-gamma-xav01");
        std::fs::create_dir_all(repo.join(".git")).expect("mkdir .git");
        std::fs::create_dir_all(repo.join(".xavier")).expect("mkdir .xavier");
        std::fs::write(
            repo.join(".xavier").join(SYNC_CHECKPOINT_FILE),
            "deadbeef1234\n",
        )
        .expect("write checkpoint");

        let id = derive_repo_identity(&repo);
        assert_eq!(id.project_id, "repo-gamma-xav01");
        assert_eq!(
            PathBuf::from(&id.root),
            repo.canonicalize().unwrap_or(repo.clone())
        );
        assert_eq!(id.indexed_commit, "deadbeef1234");
        assert_eq!(
            code_graph_db_path_for_root(Path::new(&id.root)),
            PathBuf::from(&id.root)
                .join(".xavier")
                .join("code_graph.db")
        );
    }

    /// XAV-01 regression: repos A and B keep isolated graphs.
    ///
    /// Opens both repos, asserts exclusive symbols are only visible in
    /// their own repo with differing stats, evicts B from the shared
    /// per-path cache, reopens it without loss, and reports
    /// `project_id` / `root` / `indexed_commit` per repo.
    #[test]
    fn codegraph_repo_isolation_a_b_evict_reopen() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir_a = tmp.path().join("repo-alpha-xav01");
        let dir_b = tmp.path().join("repo-beta-xav01");
        for d in [&dir_a, &dir_b] {
            std::fs::create_dir_all(d.join(".git")).expect("mkdir .git");
            std::fs::create_dir_all(d.join(".xavier")).expect("mkdir .xavier");
        }
        std::fs::write(dir_a.join(".xavier").join(SYNC_CHECKPOINT_FILE), "aaa111\n")
            .expect("write checkpoint A");
        std::fs::write(dir_b.join(".xavier").join(SYNC_CHECKPOINT_FILE), "bbb222\n")
            .expect("write checkpoint B");

        let id_a = derive_repo_identity(&dir_a);
        let id_b = derive_repo_identity(&dir_b);
        assert_ne!(id_a.project_id, id_b.project_id);
        assert_ne!(id_a.root, id_b.root);
        assert_eq!(id_a.indexed_commit, "aaa111");
        assert_eq!(id_b.indexed_commit, "bbb222");

        // Isolated manager; the underlying CodeGraphDB per-path cache is
        // still shared, which is exactly what evict/reopen exercises.
        let cm = ConnectionManager::new();
        let db_a_path = code_graph_db_path_for_root(Path::new(&id_a.root));
        let db_b_path = code_graph_db_path_for_root(Path::new(&id_b.root));
        assert_ne!(db_a_path, db_b_path);

        let db_a = cm.get_code_graph_db(&db_a_path).expect("open A");
        let db_b = cm.get_code_graph_db(&db_b_path).expect("open B");

        db_a.insert_symbol(&test_symbol("Xav01ExclusiveAlpha", "src/alpha.rs"))
            .expect("insert alpha");
        db_a.insert_symbol(&test_symbol("Xav01SharedNameExtra", "src/extra.rs"))
            .expect("insert extra");
        db_b.insert_symbol(&test_symbol("Xav01ExclusiveBeta", "src/beta.rs"))
            .expect("insert beta");

        // Isolation: exclusive symbols are only visible in their own repo.
        assert_eq!(
            db_a.find_by_name("Xav01ExclusiveAlpha", 10).unwrap().len(),
            1
        );
        assert_eq!(
            db_a.find_by_name("Xav01ExclusiveBeta", 10).unwrap().len(),
            0
        );
        assert_eq!(
            db_b.find_by_name("Xav01ExclusiveBeta", 10).unwrap().len(),
            1
        );
        assert_eq!(
            db_b.find_by_name("Xav01ExclusiveAlpha", 10).unwrap().len(),
            0
        );

        // Different repos report different stats.
        let stats_a = db_a.stats().expect("stats A");
        let stats_b = db_b.stats().expect("stats B");
        assert_eq!(stats_a.total_symbols, 2);
        assert_eq!(stats_b.total_symbols, 1);
        assert_ne!(stats_a.total_symbols, stats_b.total_symbols);

        // Evict B from the per-path cache, reopen without loss.
        assert!(cm.unload_code_graph_db(&db_b_path));
        let db_b2 = cm.get_code_graph_db(&db_b_path).expect("reopen B");
        assert_eq!(
            db_b2.find_by_name("Xav01ExclusiveBeta", 10).unwrap().len(),
            1,
            "evicted repo B must reopen without loss"
        );
        assert_eq!(
            db_b2.find_by_name("Xav01ExclusiveAlpha", 10).unwrap().len(),
            0
        );
        assert_eq!(db_b2.stats().unwrap().total_symbols, 1);

        println!(
            "XAV-01 isolation OK: A project_id={} root={} indexed_commit={} symbols={} | B project_id={} root={} indexed_commit={} symbols={}",
            id_a.project_id,
            id_a.root,
            id_a.indexed_commit,
            stats_a.total_symbols,
            id_b.project_id,
            id_b.root,
            id_b.indexed_commit,
            stats_b.total_symbols,
        );
    }
}
