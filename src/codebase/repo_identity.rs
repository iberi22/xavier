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
    sanitize_project_id(raw)
}

/// The single sanitizer for any `project_id`, whether it came from a directory
/// name (`derive_project_id`) or was declared by a repo in
/// `<repo>/.xavier/config.toml` (see [`crate::codebase::repo_config`]).
///
/// Extracted so a declared id is sanitized by the exact same rule: no second,
/// subtly different normalizer that could let one id be a file path and the
/// other not.
pub fn sanitize_project_id(raw: &str) -> String {
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
///
/// Walking up is correct for an existing directory: `apps/xavier/src` belongs
/// to the `xavier` checkout, exactly as git resolves it.
///
/// XAV-01's defect was the *non-existent* path. `repo-missing-xav01` did not
/// exist, so nothing said where its root was, and the old code kept walking
/// and returned whatever checkout was above the tempdir — on this machine
/// `~/.hermes`, a real git repo. The caller then reported a missing repo under
/// someone else's identity, which is the cross-repo leak XAV-01 forbids.
///
/// A path that does not exist now anchors at itself. That is a distinct
/// identity, so it degrades for itself instead of answering with another
/// repo's graph.
pub fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let absolute = std::path::absolute(start).unwrap_or_else(|_| start.to_path_buf());

    // A real directory may legitimately sit inside a checkout (`apps/xavier/src`
    // resolves to the `xavier` root), so walking up is correct for it.
    if absolute.is_dir() {
        let mut current = absolute.clone();
        loop {
            if current.join(".git").exists() {
                return Some(current);
            }
            match current.parent() {
                Some(parent) => current = parent.to_path_buf(),
                None => break,
            }
        }
    }

    // A path that does not exist cannot be a checkout, and its ancestors are
    // not evidence about it: borrowing one is how a missing repo inherited a
    // real repo's identity. Anchor at the path itself instead.
    Some(absolute)
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
///
/// When `cwd` is not inside a checkout, the identity is anchored at `cwd`
/// itself and marked `unknown`: it is still a distinct identity, never a
/// borrowed one, so an unindexed directory reports itself as degraded instead
/// of reporting another repo's graph.
pub fn derive_repo_identity(cwd: &Path) -> RepoIdentity {
    let root = find_repo_root(cwd)
        .unwrap_or_else(|| std::path::absolute(cwd).unwrap_or_else(|_| cwd.to_path_buf()));
    let canonical = root.canonicalize().unwrap_or(root);
    // A repo (or a product inside a monorepo) may declare its own `project_id`
    // in `<dir>/.xavier/config.toml`. That declaration wins over the id derived
    // from the directory name, which is what makes two products inside ONE
    // monorepo separable instead of collapsing to a single instance.
    //
    // The search starts at `cwd`, not at the git root: a product living at
    // `apps/duque-mvp` carries its config in ITS OWN directory, and the git
    // root is an ancestor of it — looking only at the root would miss the
    // product's config and hand both products the repo-level id.
    //
    // A missing or unreadable config is not an error: it falls back to the
    // directory-derived id, i.e. exactly the previous behaviour.
    let cwd_abs = std::path::absolute(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let project_id =
        nearest_declared_project_id(&cwd_abs).unwrap_or_else(|| derive_project_id(&canonical));
    RepoIdentity {
        project_id,
        root: canonical.to_string_lossy().into_owned(),
        indexed_commit: read_indexed_commit(&canonical),
    }
}

/// Closest ancestor-or-self of `dir` whose `.xavier/config.toml` exists.
///
/// The single walk over the config hierarchy; both the CLI and
/// [`derive_repo_identity`] resolve products through it, so a product's
/// identity cannot depend on which caller asked.
pub fn nearest_config_ancestor(dir: &Path) -> Option<PathBuf> {
    let mut current = Some(dir);
    while let Some(candidate) = current {
        if crate::codebase::repo_config::repo_config_path(candidate).is_file() {
            return Some(candidate.to_path_buf());
        }
        current = candidate.parent();
    }
    None
}

/// `project_id` declared by the closest config at or above `dir`.
///
/// A config that exists but is malformed, or declares no `project_id`, is
/// skipped in favour of continuing upwards: a broken config at one level must
/// not mask a valid one above it, and must never yield a half-parsed id.
fn nearest_declared_project_id(dir: &Path) -> Option<String> {
    nearest_config_ancestor(dir)
        .and_then(|c| crate::codebase::repo_config::RepoConfig::load(&c).ok())
        .map(|cfg| cfg.project_id)
        .filter(|id| !id.is_empty())
}

/// Canonical per-repo CodeGraph SQLite path: `<root>/.xavier/code_graph.db`.
///
/// Deliberately independent of the daemon's cwd and of the
/// `XAVIER_CODE_GRAPH_DB_PATH` override so that two checkouts can never
/// resolve to the same graph file.
pub fn code_graph_db_path_for_root(root: &Path) -> PathBuf {
    root.join(".xavier").join("code_graph.db")
}

/// Tests for the config-driven `project_id` in [`derive_repo_identity`].
///
/// These cover the wiring itself: a declared `project_id` must win over the
/// directory name, and a repo without a config must behave exactly as before.
#[cfg(test)]
mod repo_config_identity_tests {
    use super::*;

    fn git_repo(root: &Path) {
        std::fs::create_dir_all(root.join(".git")).expect("mkdir .git");
    }

    /// The point of the whole feature: two products inside ONE monorepo get
    /// two identities. Without a declared config they would both derive the
    /// same id from the repo directory name and collapse into one instance.
    #[test]
    fn declared_project_id_wins_over_directory_name() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("monorepo");
        git_repo(&repo);
        let product = repo.join("apps").join("duque-mvp");
        std::fs::create_dir_all(product.join(".xavier")).expect("mkdir product");

        // Precondition: same repo, no config ⇒ one shared id.
        assert_eq!(
            derive_repo_identity(&product).project_id,
            derive_repo_identity(&repo).project_id,
            "without a config both products must collapse to one id"
        );

        std::fs::write(
            product.join(".xavier").join("config.toml"),
            "project_id = \"duque-mvp\"\nname = \"duque-mvp\"\n",
        )
        .expect("write config");

        assert_eq!(derive_repo_identity(&product).project_id, "duque-mvp");
    }

    /// A repo with no config must keep deriving from the directory name: the
    /// wiring must not silently change behaviour for existing checkouts.
    #[test]
    fn absent_config_keeps_directory_derived_id() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("plain-repo");
        git_repo(&repo);
        assert_eq!(
            derive_repo_identity(&repo).project_id,
            derive_project_id(&repo)
        );
    }

    /// A malformed config must not take the identity down with it: the
    /// directory-derived id is the safe fallback.
    #[test]
    fn malformed_config_falls_back_to_directory_id() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("broken-repo");
        git_repo(&repo);
        std::fs::create_dir_all(repo.join(".xavier")).expect("mkdir .xavier");
        std::fs::write(repo.join(".xavier").join("config.toml"), "project_id = [[[")
            .expect("write broken config");
        assert_eq!(
            derive_repo_identity(&repo).project_id,
            derive_project_id(&repo)
        );
    }
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

    /// XAV-01: a path that does not exist must not borrow an ancestor
    /// checkout's identity. `repo-missing-xav01` is the exact shape the
    /// handler tests use; before the fix it resolved to `~/.hermes`, a real
    /// repository, because the walk never stopped.
    #[test]
    fn a_missing_path_does_not_inherit_an_ancestor_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("repo-missing-xav01");
        assert!(!missing.exists(), "precondition: the path must not exist");

        let resolved = find_repo_root(&missing)
            .expect("a missing path still anchors at itself, it is not None");
        assert!(
            resolved.ends_with("repo-missing-xav01"),
            "must anchor at the missing path, got {}",
            resolved.display()
        );

        let id = derive_repo_identity(&missing);
        assert!(
            id.root.contains("repo-missing-xav01"),
            "the identity must name the missing path, got {}",
            id.root
        );
        assert_eq!(
            id.indexed_commit, UNKNOWN_COMMIT,
            "an unidentified repo must never report a real commit"
        );
    }

    /// Two missing directories under the same tempdir stay distinct, so one
    /// missing repo can never read as another.
    #[test]
    fn two_missing_paths_get_distinct_identities() {
        let tmp = tempfile::tempdir().unwrap();
        let a = derive_repo_identity(&tmp.path().join("missing-alpha"));
        let b = derive_repo_identity(&tmp.path().join("missing-beta"));
        assert_ne!(a.root, b.root);
        assert_ne!(a.project_id, b.project_id);
    }

    /// The positive case still works: a real checkout resolves to itself.
    #[test]
    fn a_real_checkout_still_resolves_to_its_own_root() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let nested = repo.join("src").join("deep");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(&nested).unwrap();

        assert_eq!(
            find_repo_root(&nested),
            Some(repo.canonicalize().unwrap_or(repo)),
            "a nested path inside a checkout must resolve to the checkout root"
        );
    }
}
