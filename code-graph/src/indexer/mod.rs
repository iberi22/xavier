//! Indexer - scans and indexes codebases.
//!
//! The Indexer holds the deprecated `PluginHost` for backwards compatibility;
//! it delegates to the underlying `PluginManager` when calling `parse_source`.
//! Silencing the deprecation here keeps the build log readable while we migrate
//! callers incrementally.
#![allow(deprecated)]

// pub mod watcher;

use crate::db::CodeGraphDB;
use crate::error::{GraphError, Result};
use crate::parser::parse_source;
use crate::plugin_host::PluginHost;
use crate::types::{CodeEdge, EdgeType, IndexStats, Language, Symbol, SymbolKind};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};

pub mod call_resolution;

#[allow(async_fn_in_trait)]
pub trait IndexEngine: Send + Sync {
    async fn index_project(&self, root: &Path, incremental: bool) -> Result<IndexStats>;
    async fn build_call_graph(&self, root: &Path) -> Result<Vec<CodeEdge>>;
    async fn resolve_dependencies(&self, root: &Path) -> Result<Vec<CodeEdge>>;
}

pub struct RustKernel {
    db: Arc<CodeGraphDB>,
    max_concurrent: usize,
    plugin_host: Arc<PluginHost>,
}

pub type Indexer = RustKernel;

impl RustKernel {
    pub fn new(db: Arc<CodeGraphDB>) -> Self {
        Self {
            db,
            max_concurrent: 8,
            plugin_host: Arc::new(PluginHost::new()),
        }
    }

    /// Index a directory.
    pub async fn index(&self, root: &Path, incremental: bool) -> Result<IndexStats> {
        let start = Instant::now();
        info!(
            "Starting {}indexing of {:?}",
            if incremental { "incremental " } else { "" },
            root
        );

        let all_files = self.collect_files(root)?;
        let mut files_to_index = Vec::new();
        let mut files_to_mtime = HashMap::new();
        let mut files_to_delete = Vec::new();

        if !incremental {
            self.db.clear()?;
            files_to_index = all_files;
            for file_path in &files_to_index {
                let (rel, mtime) = get_file_info(root, file_path);
                files_to_mtime.insert(rel, mtime);
            }
        } else {
            let existing_metadata = self.db.get_all_file_metadata()?;
            let mut current_files = std::collections::HashSet::new();

            for file_path in all_files {
                let (relative_path, mtime) = get_file_info(root, &file_path);
                current_files.insert(relative_path.clone());

                if let Some(&old_mtime) = existing_metadata.get(&relative_path) {
                    if old_mtime != mtime {
                        debug!("File changed: {}", relative_path);
                        files_to_delete.push(relative_path.clone());
                        files_to_index.push(file_path);
                        files_to_mtime.insert(relative_path, mtime);
                    }
                } else {
                    debug!("New file: {}", relative_path);
                    files_to_index.push(file_path);
                    files_to_mtime.insert(relative_path, mtime);
                }
            }

            for path in existing_metadata.keys() {
                if !current_files.contains(path) {
                    info!("File removed: {}", path);
                    files_to_delete.push(path.clone());
                }
            }

            if files_to_index.is_empty() && files_to_delete.is_empty() {
                info!("No changes detected, skipping index update.");
                let mut stats = self.db.stats()?;
                stats.duration_ms = start.elapsed().as_millis() as u64;
                return Ok(stats);
            }

            self.db.batch_delete_file_data(&files_to_delete)?;
        }

        info!("Found {} files to index", files_to_index.len());

        let semaphore = Arc::new(Semaphore::new(self.max_concurrent));
        let mut handles = Vec::new();

        for file_path in files_to_index {
            let sem = semaphore.clone();
            let root = root.to_path_buf();
            let plugin_host = self.plugin_host.clone();
            let handle = tokio::spawn(async move {
                let _permit = sem.acquire().await.expect("semaphore should be open");
                parse_file(&root, &file_path, Some(&*plugin_host)).await
            });
            handles.push(handle);
        }

        let mut new_symbols = Vec::new();
        let mut sources = HashMap::new();
        for handle in handles {
            match handle.await {
                Ok(Ok(parsed)) => {
                    sources.insert(parsed.file_path.clone(), parsed.source);
                    new_symbols.extend(parsed.symbols);
                }
                Ok(Err(error)) => warn!("Failed to parse file: {}", error),
                Err(error) => error!("Task failed: {}", error),
            }
        }

        assign_stable_ids(&mut new_symbols);

        // Load all symbols to build edges correctly (new symbols can point to old ones)
        let all_symbols = if incremental && !new_symbols.is_empty() {
            let mut all = self.db.get_all_symbols()?;
            all.extend(new_symbols.clone());
            all
        } else {
            new_symbols.clone()
        };

        let edges = build_edges(&new_symbols, &all_symbols, &sources);
        let edges_len = edges.len();

        let db = Arc::clone(&self.db);
        let mut stats = tokio::task::spawn_blocking(move || {
            db.insert_symbols(&new_symbols)?;
            db.insert_edges(&edges)?;
            db.batch_upsert_file_metadata(files_to_mtime)?;
            db.stats()
        })
        .await
        .map_err(|e| GraphError::Io(std::io::Error::other(e)))??;
        stats.duration_ms = start.elapsed().as_millis() as u64;

        info!(
            "Indexed {} files, {} symbols, {} edges in {}ms",
            stats.total_files, stats.total_symbols, edges_len, stats.duration_ms
        );

        Ok(stats)
    }

    /// Collect all relevant files in a directory using .gitignore/.ignore aware traversal.
    fn collect_files(&self, root: &Path) -> Result<Vec<PathBuf>> {
        let excludes = build_excludes(&[
            "**/target/**",
            "**/.git/**",
            "**/node_modules/**",
            "**/dist/**",
            "**/build/**",
            "**/.next/**",
            "**/.nuxt/**",
            "**/coverage/**",
            "**/__pycache__/**",
            "**/.pytest_cache/**",
            "**/.codegraph/**",
        ]);

        let mut files = Vec::new();
        let walker = WalkBuilder::new(root)
            .hidden(false)
            .git_ignore(true)
            .git_exclude(true)
            .ignore(true)
            .require_git(false)
            .build();

        for entry in walker {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    warn!("Error walking directory: {}", error);
                    continue;
                }
            };
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if excludes.as_ref().is_some_and(|set| set.is_match(path)) {
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if Language::from_extension_with_plugins(ext, self.plugin_host.discovery())
                == Language::Unknown
            {
                continue;
            }
            files.push(path.to_path_buf());
        }

        Ok(files)
    }

    pub async fn analyze_project(&self, root: &Path) -> Result<(Vec<Symbol>, HashMap<String, String>)> {
        let files = self.collect_files(root)?;
        let mut symbols = Vec::new();
        let mut sources = HashMap::new();
        for file in files {
            if let Ok(parsed) = parse_file(root, &file, Some(&*self.plugin_host)).await {
                sources.insert(parsed.file_path.clone(), parsed.source);
                symbols.extend(parsed.symbols);
            }
        }
        assign_stable_ids(&mut symbols);
        Ok((symbols, sources))
    }
}

impl IndexEngine for RustKernel {
    async fn index_project(&self, root: &Path, incremental: bool) -> Result<IndexStats> {
        self.index(root, incremental).await
    }

    async fn build_call_graph(&self, root: &Path) -> Result<Vec<CodeEdge>> {
        let (symbols, sources) = self.analyze_project(root).await?;
        Ok(call_resolution::build_call_graph_edges(&symbols, &symbols, &sources))
    }

    async fn resolve_dependencies(&self, root: &Path) -> Result<Vec<CodeEdge>> {
        let (symbols, _sources) = self.analyze_project(root).await?;
        let mut edges = Vec::new();
        for symbol in &symbols {
            let file_node = format!("file:{}", symbol.file_path);
            if symbol.kind == SymbolKind::Import {
                edges.push(CodeEdge {
                    id: None,
                    from_symbol: file_node,
                    to_symbol: format!("module:{}", symbol.name),
                    edge_type: EdgeType::Imports,
                    file_path: symbol.file_path.clone(),
                    line: symbol.start_line,
                    confidence: 0.8,
                    metadata: None,
                });
            }
        }
        Ok(edges)
    }
}

pub struct CodeGraphCKernel {
    binary_path: PathBuf,
}

impl CodeGraphCKernel {
    pub fn new(binary_path: PathBuf) -> Self {
        Self { binary_path }
    }
}

impl Default for CodeGraphCKernel {
    fn default() -> Self {
        Self {
            binary_path: PathBuf::from("codegraph"),
        }
    }
}

impl IndexEngine for CodeGraphCKernel {
    async fn index_project(&self, root: &Path, incremental: bool) -> Result<IndexStats> {
        let mut cmd = tokio::process::Command::new(&self.binary_path);
        cmd.arg("index")
            .arg(root);
        if incremental {
            cmd.arg("--incremental");
        }

        let output = cmd.output().await.map_err(|e| {
            GraphError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Failed to execute codegraph binary: {}", e),
            ))
        })?;

        if !output.status.success() {
            return Err(GraphError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "codegraph binary failed with status {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                ),
            )));
        }

        let stats: IndexStats = serde_json::from_slice(&output.stdout).map_err(|e| {
            GraphError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to parse codegraph index output: {}", e),
            ))
        })?;

        Ok(stats)
    }

    async fn build_call_graph(&self, root: &Path) -> Result<Vec<CodeEdge>> {
        let mut cmd = tokio::process::Command::new(&self.binary_path);
        cmd.arg("call-graph")
            .arg(root);

        let output = cmd.output().await.map_err(|e| {
            GraphError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Failed to execute codegraph binary: {}", e),
            ))
        })?;

        if !output.status.success() {
            return Err(GraphError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "codegraph binary failed with status {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                ),
            )));
        }

        let edges: Vec<CodeEdge> = serde_json::from_slice(&output.stdout).map_err(|e| {
            GraphError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to parse codegraph call-graph output: {}", e),
            ))
        })?;

        Ok(edges)
    }

    async fn resolve_dependencies(&self, root: &Path) -> Result<Vec<CodeEdge>> {
        let mut cmd = tokio::process::Command::new(&self.binary_path);
        cmd.arg("dependencies")
            .arg(root);

        let output = cmd.output().await.map_err(|e| {
            GraphError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Failed to execute codegraph binary: {}", e),
            ))
        })?;

        if !output.status.success() {
            return Err(GraphError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "codegraph binary failed with status {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                ),
            )));
        }

        let edges: Vec<CodeEdge> = serde_json::from_slice(&output.stdout).map_err(|e| {
            GraphError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to parse codegraph dependencies output: {}", e),
            ))
        })?;

        Ok(edges)
    }
}

struct ParsedFile {
    file_path: String,
    source: String,
    symbols: Vec<Symbol>,
}

async fn parse_file(
    root: &Path,
    file_path: &Path,
    plugin_host: Option<&PluginHost>,
) -> Result<ParsedFile> {
    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let lang = if let Some(host) = plugin_host {
        Language::from_extension_with_plugins(ext, host.discovery())
    } else {
        Language::from_extension(ext)
    };

    // We try to parse even if lang is Unknown because a plugin might handle it by extension
    // But for now, we follow the discovery logic in PluginHost::parser_for which might return NoOp

    let source = std::fs::read_to_string(file_path).map_err(GraphError::Io)?;
    let relative_path = file_path
        .strip_prefix(root)
        .unwrap_or(file_path)
        .to_string_lossy()
        .replace('\\', "/");
    // The deprecated PluginHost wraps a PluginManager; unwrap it so parse_source
    // receives the fallback-chain-aware manager. When no host is supplied we
    // fall back to the built-in native parsers.
    let manager = plugin_host.map(|host| host.manager());
    let symbols = parse_source(&source, &lang, &relative_path, manager).await?;
    if !symbols.is_empty() {
        debug!("Extracted {} symbols from {}", symbols.len(), relative_path);
    }

    Ok(ParsedFile {
        file_path: relative_path,
        source,
        symbols,
    })
}

fn assign_stable_ids(symbols: &mut [Symbol]) {
    for symbol in symbols {
        if symbol.stable_id.is_none() {
            symbol.stable_id = Some(symbol.deterministic_id("default"));
        }
    }
}

fn build_edges(
    new_symbols: &[Symbol],
    all_symbols: &[Symbol],
    sources: &HashMap<String, String>,
) -> Vec<CodeEdge> {
    let mut edges = Vec::new();

    for symbol in new_symbols {
        let symbol_id = symbol
            .stable_id
            .clone()
            .unwrap_or_else(|| symbol.deterministic_id("default"));
        let file_node = format!("file:{}", symbol.file_path);

        edges.push(CodeEdge {
            id: None,
            from_symbol: file_node.clone(),
            to_symbol: symbol_id.clone(),
            edge_type: EdgeType::Contains,
            file_path: symbol.file_path.clone(),
            line: symbol.start_line,
            confidence: 1.0,
            metadata: None,
        });

        edges.push(CodeEdge {
            id: None,
            from_symbol: file_node.clone(),
            to_symbol: symbol_id.clone(),
            edge_type: EdgeType::Defines,
            file_path: symbol.file_path.clone(),
            line: symbol.start_line,
            confidence: 1.0,
            metadata: None,
        });

        if symbol.kind == SymbolKind::Import {
            edges.push(CodeEdge {
                id: None,
                from_symbol: file_node,
                to_symbol: format!("module:{}", symbol.name),
                edge_type: EdgeType::Imports,
                file_path: symbol.file_path.clone(),
                line: symbol.start_line,
                confidence: 0.8,
                metadata: None,
            });
        }
    }

    let call_edges = call_resolution::build_call_graph_edges(new_symbols, all_symbols, sources);
    edges.extend(call_edges);

    edges
}

/// Heuristic check for whether `source` (a symbol body) contains a call to a
/// callable named `name`.
///
/// Uses identifier word-boundary checks instead of a raw substring test so that
/// a callee named `init` no longer matches `initialize(`, and a callee `run`
/// no longer matches `// we run(x)` inside a comment block as aggressively. The
/// call still needs to be followed by `(` (direct call) or be a method call
/// (`.name(`).
///
/// This is a heuristic improvement over the previous `source.contains("name(")`
/// approach; a fully correct call graph requires tree-sitter call-expression
/// extraction (see `build_edges` doc).
///
/// ⚠️ DEPRECATED: Use `CallResolver` with 6-strategy cascade instead.
#[allow(dead_code)]
#[deprecated(note = "Use CallResolver with 6-strategy cascade instead")]
fn contains_call(source: &str, name: &str) -> bool {
    if name.is_empty() || source.is_empty() {
        return false;
    }
    // Find every occurrence of `name(` and check the character immediately
    // before is a word boundary (non-identifier char or start of line).
    let call_needle = format!("{}(", name);
    let method_needle = format!(".{}(", name);
    if source.contains(&method_needle) {
        return true;
    }
    let bytes = source.as_bytes();
    let needle_bytes = call_needle.as_bytes();
    let _name_first = name.as_bytes()[0];
    let mut from = 0;
    while let Some(idx) = source[from..].find(&call_needle) {
        let abs = from + idx;
        let prev_ok = if abs == 0 {
            true
        } else {
            let prev = bytes[abs - 1];
            // Word boundary: previous char must not be an identifier continuation
            // (letter, digit, underscore) so `xinit(` won't match callee `init`.
            !(prev.is_ascii_alphanumeric() || prev == b'_')
        };
        // Also ensure the char before `name` isn't `.` (that's the method-call
        // branch handled separately) — avoid double counting.
        if prev_ok {
            let is_method = abs > 0 && bytes[abs - 1] == b'.';
            if !is_method {
                return true;
            }
        }
        // Avoid matching `init` as a prefix of `initialize(`: ensure the match
        // we found is the full `name(` (it is, because the needle includes `(`),
        // so no extra suffix check is needed here.
        from = abs + needle_bytes.len();
    }
    false
}

fn get_file_info(root: &Path, file_path: &Path) -> (String, i64) {
    let relative_path = file_path
        .strip_prefix(root)
        .unwrap_or(file_path)
        .to_string_lossy()
        .replace('\\', "/");

    let mtime = std::fs::metadata(file_path)
        .and_then(|m| m.modified())
        .map(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64
        })
        .unwrap_or(0);

    (relative_path, mtime)
}

fn build_excludes(patterns: &[&str]) -> Option<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    let mut added = false;
    for pattern in patterns {
        match Glob::new(pattern) {
            Ok(glob) => {
                builder.add(glob);
                added = true;
            }
            Err(error) => warn!("Invalid glob pattern '{}': {}", pattern, error),
        }
    }
    added.then(|| builder.build().ok()).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn indexes_multiple_languages_and_edges() {
        let dir = TempDir::new().expect("temp dir");
        std::fs::write(
            dir.path().join("main.rs"),
            "fn helper() {}\nfn main() { helper(); }\n",
        )
        .expect("write rust");
        std::fs::write(
            dir.path().join("app.py"),
            "import os\nclass Service:\n    def run(self):\n        return os.getcwd()\n",
        )
        .expect("write python");

        let db = Arc::new(CodeGraphDB::in_memory().expect("db"));
        let indexer = Indexer::new(db.clone());
        let parsed_python = parse_file(dir.path(), &dir.path().join("app.py"), None).await;
        println!("DEBUG PARSED PYTHON SUCCESS: {:?}", parsed_python.is_ok());
        if let Ok(ref p) = parsed_python {
            println!("DEBUG PARSED PYTHON SYMBOLS LEN: {}", p.symbols.len());
        } else {
            println!("DEBUG PARSED PYTHON ERR: {:?}", parsed_python.err());
        }
        let parsed_rust = parse_file(dir.path(), &dir.path().join("main.rs"), None).await;
        println!("DEBUG PARSED RUST SUCCESS: {:?}", parsed_rust.is_ok());
        if let Ok(ref r) = parsed_rust {
            println!("DEBUG PARSED RUST SYMBOLS LEN: {}", r.symbols.len());
        }

        let stats = indexer.index(dir.path(), false).await.expect("index");
        println!("DEBUG STATS: {:?}", stats);
        let collected_files = indexer.collect_files(dir.path()).expect("collect_files");
        println!("DEBUG COLLECTED FILES: {:?}", collected_files);

        assert_eq!(stats.total_files, 2);
        assert!(stats.total_symbols >= 5);
        assert!(stats.total_imports >= 1);
        assert!(!db.hub_nodes(1, 10).expect("hubs").is_empty());
    }

    #[test]
    fn collector_respects_gitignore_and_common_excludes() {
        let dir = TempDir::new().expect("temp dir");
        std::fs::write(dir.path().join(".gitignore"), "ignored.rs\n").expect("gitignore");
        std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").expect("main");
        std::fs::write(dir.path().join("ignored.rs"), "fn ignored() {}\n").expect("ignored");
        std::fs::create_dir(dir.path().join("target")).expect("target");
        std::fs::write(dir.path().join("target").join("skip.rs"), "fn skip() {}\n").expect("skip");

        let db = Arc::new(CodeGraphDB::in_memory().expect("db"));
        let indexer = Indexer::new(db);
        let files = indexer.collect_files(dir.path()).expect("collect");

        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("main.rs"));
    }

    #[tokio::test]
    async fn incremental_indexing_skips_unchanged_files() {
        let dir = TempDir::new().expect("temp dir");
        let file_path = dir.path().join("main.rs");
        std::fs::write(&file_path, "fn main() {}\n").expect("write rust");

        let db = Arc::new(CodeGraphDB::in_memory().expect("db"));
        let indexer = Indexer::new(db.clone());

        // First index
        let stats1 = indexer.index(dir.path(), true).await.expect("first index");
        assert_eq!(stats1.total_files, 1);
        let symbols1 = db.get_all_symbols().expect("get symbols");
        assert_eq!(symbols1.len(), 1);

        // Second index (no changes)
        let stats2 = indexer.index(dir.path(), true).await.expect("second index");
        assert_eq!(stats2.total_files, 1);
        // Duration should be low, but more importantly, we can check if it re-indexed
        // In our current implementation, new_symbols will be empty if nothing changed
        // We can check if it still has the same symbols
        let symbols2 = db.get_all_symbols().expect("get symbols");
        assert_eq!(symbols2.len(), 1);
        assert_eq!(symbols1[0].stable_id, symbols2[0].stable_id);

        // Modify file
        // Since we can't easily set mtime in a cross-platform way without extra crates,
        // and our implementation uses std::fs::metadata, let's just write and hope mtime changes
        // or just wait a bit.
        std::thread::sleep(std::time::Duration::from_secs(1));
        std::fs::write(
            &file_path,
            "fn main() { println!(\"hi\"); }\nfn other() {}\n",
        )
        .expect("write rust");

        let stats3 = indexer.index(dir.path(), true).await.expect("third index");
        assert_eq!(stats3.total_files, 1);
        let symbols3 = db.get_all_symbols().expect("get symbols");
        assert_eq!(symbols3.len(), 2); // main and other
    }

    #[tokio::test]
    async fn incremental_indexing_removes_deleted_files() {
        let dir = TempDir::new().expect("temp dir");
        let file1 = dir.path().join("main.rs");
        let file2 = dir.path().join("other.rs");
        std::fs::write(&file1, "fn main() {}\n").expect("write file1");
        std::fs::write(&file2, "fn other() {}\n").expect("write file2");

        let db = Arc::new(CodeGraphDB::in_memory().expect("db"));
        let indexer = Indexer::new(db.clone());

        indexer.index(dir.path(), true).await.expect("first index");
        assert_eq!(db.stats().expect("stats").total_files, 2);

        std::fs::remove_file(file2).expect("remove file2");
        indexer.index(dir.path(), true).await.expect("second index");

        assert_eq!(db.stats().expect("stats").total_files, 1);
        let symbols = db.get_all_symbols().expect("get symbols");
        assert!(symbols.iter().all(|s| s.file_path == "main.rs"));
    }

    #[tokio::test]
    async fn indexer_picks_up_plugin_backed_extension() {
        let dir = TempDir::new().expect("temp dir");
        std::fs::write(dir.path().join("script.rb"), "def hello; end\n").expect("write ruby");

        let db = Arc::new(CodeGraphDB::in_memory().expect("db"));
        let indexer = Indexer::new(db.clone());

        // Register a mock plugin for Ruby
        let ruby_lang = Language::Other("ruby".to_string());
        indexer
            .plugin_host
            .manager()
            .register(crate::plugin::types::PluginDescriptor {
                name: "parser-ruby".to_string(),
                version: "1.0.0".to_string(),
                command: "ruby-parser".to_string(), // Won't be called if we use a mock engine
                languages: vec![ruby_lang.clone()],
                extensions: vec!["rb".to_string()],
                capabilities: vec!["parse".to_string()],
            });

        // Use a mock engine to avoid executing a real subprocess
        // PluginManager::new uses ProcessEngine by default.
        // We can't easily swap the engine in an existing Indexer/PluginHost,
        // but for collect_files test we don't even need the engine.

        let files = indexer.collect_files(dir.path()).expect("collect");
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("script.rb"));

        // To test full indexing, we'd need to mock the PluginEngine.
        // Since PluginManager stores engine as Arc<dyn PluginEngine>, we could theoretically
        // provide a mock one at construction.
    }

    #[tokio::test]
    async fn indexer_skips_unknown_extension_with_no_plugin() {
        let dir = TempDir::new().expect("temp dir");
        std::fs::write(dir.path().join("readme.md"), "# Hello\n").expect("write md");

        let db = Arc::new(CodeGraphDB::in_memory().expect("db"));
        let indexer = Indexer::new(db.clone());

        let files = indexer.collect_files(dir.path()).expect("collect");
        assert_eq!(files.len(), 0);
    }

    #[tokio::test]
    async fn indexer_builtin_languages_unchanged() {
        let dir = TempDir::new().expect("temp dir");
        std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").expect("write rust");

        let db = Arc::new(CodeGraphDB::in_memory().expect("db"));
        let indexer = Indexer::new(db.clone());
        let stats = indexer.index(dir.path(), false).await.expect("index");

        assert_eq!(stats.total_files, 1);
        assert!(stats.total_symbols >= 1);
        assert_eq!(stats.languages[0].lang, Language::Rust);
    }

    #[tokio::test]
    async fn build_edges_uses_resolver_instead_of_contains_call() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("main.rs"),
            "mod processor;\nfn main() { processor::process_data(); }",
        )
        .unwrap();
        std::fs::write(dir.path().join("processor.rs"), "pub fn process_data() {}").unwrap();

        let db = Arc::new(CodeGraphDB::in_memory().unwrap());
        let indexer = Indexer::new(db.clone());
        indexer.index(dir.path(), false).await.unwrap();

        let edges = db.get_all_edges().unwrap();
        let call_edges: Vec<_> = edges
            .iter()
            .filter(|e| e.edge_type == EdgeType::Calls)
            .collect();

        assert!(!call_edges.is_empty(), "Should have call edges");
        // Verify at least one edge has metadata.strategy
        assert!(
            call_edges.iter().any(|e| {
                e.metadata
                    .as_ref()
                    .and_then(|m| m.get("strategy"))
                    .is_some()
            }),
            "Call edges should include strategy metadata"
        );
    }

    #[tokio::test]
    async fn rust_kernel_build_call_graph_and_dependencies() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("main.rs"),
            "mod processor;\nfn main() { processor::process_data(); }",
        )
        .unwrap();
        std::fs::write(dir.path().join("processor.rs"), "pub fn process_data() {}").unwrap();

        let db = Arc::new(CodeGraphDB::in_memory().unwrap());
        let kernel = RustKernel::new(db.clone());

        // Test index_project
        let stats = kernel.index_project(dir.path(), false).await.unwrap();
        assert_eq!(stats.total_files, 2);

        // Test build_call_graph
        let call_graph = kernel.build_call_graph(dir.path()).await.unwrap();
        assert!(!call_graph.is_empty(), "Should build a call graph with edges");
        assert!(
            call_graph.iter().any(|e| e.edge_type == EdgeType::Calls),
            "Should contain Calls edges"
        );

        // Test resolve_dependencies
        std::fs::write(
            dir.path().join("app.py"),
            "import os\n",
        )
        .unwrap();
        let deps = kernel.resolve_dependencies(dir.path()).await.unwrap();
        assert!(
            deps.iter().any(|e| e.edge_type == EdgeType::Imports),
            "Should find Imports dependency edges"
        );
    }

    #[tokio::test]
    async fn c_kernel_error_when_binary_missing() {
        let kernel = CodeGraphCKernel::new(PathBuf::from("nonexistent_codegraph_binary"));
        let result = kernel.build_call_graph(Path::new(".")).await;
        assert!(result.is_err());
        let err_str = result.err().unwrap().to_string();
        assert!(err_str.contains("Failed to execute codegraph") || err_str.contains("entity not found") || err_str.contains("No such file or directory"));
    }
}
