//! Snapshot Manager — codebase snapshots + precise change descriptors.
//!
//! Snapshot manager for Xavier's CodeGraph. A snapshot captures the indexed
//! state of a repository (files, symbols, hash) so consumers can query it
//! WITHOUT having the source tree. `PreciseChange` describes the EXACT
//! fragment (file + symbol + line range + before/after) an executor must
//! change — avoiding full-file rewrites and saving tokens.

use crate::codebase::codegraph_paths::code_graph_db_path_for;
use anyhow::{Context, Result};
use code_graph::db::CodeGraphDB;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A codebase snapshot for one repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeSnapshot {
    pub repo: String,
    pub snapshot_date: String,
    pub hash: String,
    pub symbols_total: u64,
    pub files_total: u64,
    pub tree: serde_json::Value,
}

/// The EXACT change an executor must apply: file + symbol + line range +
/// before/after snippets. This is the token-saving core: the agent receives
/// only the fragment to change, never the whole file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreciseChange {
    pub repo: String,
    pub file: String,
    pub symbol: String,
    pub start_line: u32,
    pub end_line: u32,
    pub before_snippet: String,
    pub after_snippet: String,
}

/// Manages codebase snapshots persisted under data/snapshots/.
pub struct SnapshotManager {
    snapshots_dir: PathBuf,
}

impl SnapshotManager {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            snapshots_dir: data_dir.join("snapshots"),
        }
    }

    fn snapshot_path(&self, repo: &str) -> PathBuf {
        self.snapshots_dir.join(format!("{}.json", repo))
    }

    /// Create a snapshot from the CodeGraph DB for the given repo root.
    pub fn create_snapshot(&self, repo_root: &Path, repo: &str) -> Result<CodeSnapshot> {
        std::fs::create_dir_all(&self.snapshots_dir)
            .with_context(|| format!("create snapshots dir {:?}", self.snapshots_dir))?;

        let db_path = code_graph_db_path_for(repo_root);
        let db = CodeGraphDB::new(&db_path)
            .with_context(|| format!("open CodeGraph DB at {:?}", db_path))?;
        let stats = db.stats().with_context(|| "read CodeGraph stats")?;

        let hash = snapshot_hash(repo, &stats);

        let snapshot = CodeSnapshot {
            repo: repo.to_string(),
            snapshot_date: chrono::Utc::now().to_rfc3339(),
            hash,
            symbols_total: stats.total_symbols,
            files_total: stats.total_files,
            tree: serde_json::json!({
                "files": stats.total_files,
                "symbols": stats.total_symbols,
                "imports": stats.total_imports,
                "languages": stats
                    .languages
                    .iter()
                    .map(|l| serde_json::json!({"language": format!("{:?}", l.lang), "files": l.count}))
                    .collect::<Vec<_>>(),
            }),
        };

        let path = self.snapshot_path(repo);
        let json = serde_json::to_string_pretty(&snapshot)?;
        std::fs::write(&path, json).with_context(|| format!("write snapshot {:?}", path))?;

        Ok(snapshot)
    }

    pub fn list_snapshots(&self) -> Result<Vec<CodeSnapshot>> {
        let mut out = Vec::new();
        if !self.snapshots_dir.exists() {
            return Ok(out);
        }
        for entry in std::fs::read_dir(&self.snapshots_dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "json") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(s) = serde_json::from_str::<CodeSnapshot>(&content) {
                        out.push(s);
                    }
                }
            }
        }
        out.sort_by(|a, b| b.snapshot_date.cmp(&a.snapshot_date));
        Ok(out)
    }

    pub fn get_snapshot(&self, repo: &str) -> Result<Option<CodeSnapshot>> {
        let path = self.snapshot_path(repo);
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)?;
        Ok(Some(serde_json::from_str(&content)?))
    }

    /// Build a PreciseChange for a symbol in a repo's source file.
    /// `before_snippet` is the exact current fragment; `after_snippet` is the
    /// proposed replacement — the executor applies ONLY this delta.
    pub fn build_precise_change(
        &self,
        repo: &str,
        file: &str,
        symbol: &str,
        start_line: u32,
        end_line: u32,
        source: &str,
        after_snippet: &str,
    ) -> PreciseChange {
        let lines: Vec<&str> = source.lines().collect();
        let start = (start_line.saturating_sub(1)) as usize;
        let end = (end_line as usize).min(lines.len());
        let before = if start < end && start < lines.len() {
            lines[start..end].join("\n")
        } else {
            String::new()
        };
        PreciseChange {
            repo: repo.to_string(),
            file: file.to_string(),
            symbol: symbol.to_string(),
            start_line,
            end_line,
            before_snippet: before,
            after_snippet: after_snippet.to_string(),
        }
    }
}

/// Deterministic snapshot hash over repo + stats.
fn snapshot_hash(repo: &str, stats: &code_graph::types::IndexStats) -> String {
    let mut hasher = Sha256::new();
    hasher.update(repo.as_bytes());
    hasher.update(stats.total_files.to_le_bytes());
    hasher.update(stats.total_symbols.to_le_bytes());
    hasher.update(stats.total_imports.to_le_bytes());
    hex_encode(&hasher.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Discover repository roots under SWAL projects workspace.
///
/// Looks under `base_dir` (or `SWAL_REPOS_DIR` env var, or `~/proyectosSWAL`, or `/home/belal/proyectosSWAL`).
/// Checks direct child directories as well as subcategory directories (`cores/`, `apps/`, `synapse/`, `periferia/`).
/// A directory is considered a repo root if it contains `.git` or `.gitcore`.
pub fn discover_swal_repo_roots(base_dir: Option<&Path>) -> HashMap<String, PathBuf> {
    let mut map = HashMap::new();

    let root_path = if let Some(b) = base_dir {
        b.to_path_buf()
    } else if let Ok(env_path) = std::env::var("SWAL_REPOS_DIR") {
        PathBuf::from(env_path)
    } else {
        let home = std::env::var("HOME").map(PathBuf::from).ok();
        let swal_home = home.as_ref().map(|h| h.join("proyectosSWAL"));
        if let Some(ref p) = swal_home {
            if p.exists() {
                p.clone()
            } else {
                PathBuf::from("proyectosSWAL")
            }
        } else {
            PathBuf::from("proyectosSWAL")
        }
    };

    if !root_path.exists() {
        return map;
    }

    let scan_dir = |dir: &Path, out: &mut HashMap<String, PathBuf>| {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if path.is_dir() {
                    let is_repo = path.join(".git").exists() || path.join(".gitcore").exists();
                    if is_repo {
                        if let Some(repo_name) = path.file_name().and_then(|n| n.to_str()) {
                            out.insert(repo_name.to_string(), path);
                        }
                    }
                }
            }
        }
    };

    // 1. Scan direct subdirectories
    scan_dir(&root_path, &mut map);

    // 2. Scan subcategories (cores, apps, synapse, periferia, etc.)
    if let Ok(entries) = std::fs::read_dir(&root_path) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() && !path.join(".git").exists() && !path.join(".gitcore").exists() {
                scan_dir(&path, &mut map);
            }
        }
    }

    map
}

/// Index a repo name -> snapshot mapping from a directory of repo roots.
pub fn snapshot_all_repos(repo_roots: &HashMap<String, PathBuf>, data_dir: &Path) -> Result<Vec<CodeSnapshot>> {
    let manager = SnapshotManager::new(data_dir);
    let mut out = Vec::new();
    for (repo, root) in repo_roots {
        if let Ok(s) = manager.create_snapshot(root, repo) {
            out.push(s);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_hash_deterministic() {
        let stats = code_graph::types::IndexStats {
            total_files: 10,
            total_symbols: 100,
            total_imports: 50,
            languages: vec![],
            duration_ms: 0,
        };
        let h1 = snapshot_hash("xavier", &stats);
        let h2 = snapshot_hash("xavier", &stats);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_precise_change_builds_before_snippet() {
        let manager = SnapshotManager::new(Path::new("/tmp/snap-test"));
        let source = "line 1\nline 2\nline 3\nline 4\nline 5";
        let change = manager.build_precise_change(
            "xavier",
            "src/main.rs",
            "fn run",
            2,
            4,
            source,
            "line 2'\\nline 3'\\nline 4'",
        );
        assert_eq!(change.start_line, 2);
        assert_eq!(change.end_line, 4);
        assert_eq!(change.before_snippet, "line 2\nline 3\nline 4");
        assert_eq!(change.symbol, "fn run");
    }

    #[test]
    fn test_precise_change_out_of_range() {
        let manager = SnapshotManager::new(Path::new("/tmp/snap-test"));
        let source = "a\nb";
        let change = manager.build_precise_change("r", "f.rs", "sym", 99, 100, source, "x");
        assert!(change.before_snippet.is_empty() || change.start_line >= change.end_line);
    }

    #[test]
    fn test_snapshot_roundtrip_with_tempdir() {
        let dir = std::env::temp_dir().join(format!("snap-rt-{}", std::process::id()));
        let manager = SnapshotManager::new(&dir);
        // No CodeGraph DB in a temp dir: create_snapshot should error gracefully
        // (repo root has no code_graph.db), so we test list/get on empty state.
        let list = manager.list_snapshots().unwrap_or_default();
        assert!(list.is_empty());
        let got = manager.get_snapshot("nonexistent").unwrap_or(None);
        assert!(got.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_build_precise_change_keeps_after_snippet() {
        let manager = SnapshotManager::new(Path::new("/tmp/snap-test"));
        let change = manager.build_precise_change(
            "gestalt",
            "src/chain.rs",
            "Chain::run",
            10,
            12,
            "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm",
            "j'\\nk'\\nl'",
        );
        assert_eq!(change.after_snippet, "j'\\nk'\\nl'");
        assert_eq!(change.repo, "gestalt");
    }

    #[test]
    fn test_hex_encode_padding() {
        let hex = hex_encode(&[0u8, 1u8, 255u8]);
        assert_eq!(hex, "0001ff");
    }

    #[test]
    fn test_discover_swal_repo_roots_with_categories() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path();

        // Direct repo
        let direct = base.join("direct-repo");
        std::fs::create_dir_all(direct.join(".git")).unwrap();

        // Subcategory repo (e.g. cores/core-repo)
        let cores = base.join("cores");
        let core_repo = cores.join("core-repo");
        std::fs::create_dir_all(core_repo.join(".gitcore")).unwrap();

        let discovered = discover_swal_repo_roots(Some(base));
        assert_eq!(discovered.len(), 2);
        assert_eq!(discovered.get("direct-repo"), Some(&direct));
        assert_eq!(discovered.get("core-repo"), Some(&core_repo));
    }

    #[tokio::test]
    async fn test_snapshot_all_repos_multi_language() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path();

        // Repo 1: Rust
        let repo1 = base.join("repo-rust");
        let src1 = repo1.join("src");
        std::fs::create_dir_all(&src1).unwrap();
        std::fs::create_dir_all(repo1.join(".git")).unwrap();
        std::fs::write(src1.join("lib.rs"), "pub fn rust_fn() {}\npub struct RustStruct;\n").unwrap();

        // Repo 2: TypeScript & Python
        let repo2 = base.join("repo-ts-py");
        let src2 = repo2.join("src");
        std::fs::create_dir_all(&src2).unwrap();
        std::fs::create_dir_all(repo2.join(".git")).unwrap();
        std::fs::write(src2.join("app.ts"), "export function tsFunction(): string { return 'hello'; }\n").unwrap();
        std::fs::write(src2.join("script.py"), "def py_function():\n    pass\n").unwrap();

        // Index CodeGraph for both repos
        let db_path1 = crate::codebase::codegraph_paths::code_graph_db_path_for(&repo1);
        let db_path2 = crate::codebase::codegraph_paths::code_graph_db_path_for(&repo2);

        std::fs::create_dir_all(db_path1.parent().unwrap()).unwrap();
        std::fs::create_dir_all(db_path2.parent().unwrap()).unwrap();

        let db1 = std::sync::Arc::new(CodeGraphDB::new(&db_path1).unwrap());
        let db2 = std::sync::Arc::new(CodeGraphDB::new(&db_path2).unwrap());

        let indexer1 = code_graph::indexer::Indexer::new(db1);
        let indexer2 = code_graph::indexer::Indexer::new(db2);

        indexer1.index(&repo1, false).await.unwrap();
        indexer2.index(&repo2, false).await.unwrap();

        let repo_roots = discover_swal_repo_roots(Some(base));
        assert_eq!(repo_roots.len(), 2);

        let data_dir = base.join("data");
        let snapshots = snapshot_all_repos(&repo_roots, &data_dir).unwrap();

        assert_eq!(snapshots.len(), 2);

        let rust_snap = snapshots.iter().find(|s| s.repo == "repo-rust").unwrap();
        assert!(rust_snap.symbols_total > 0);

        let multi_snap = snapshots.iter().find(|s| s.repo == "repo-ts-py").unwrap();
        assert!(multi_snap.symbols_total >= 2);
    }
}
