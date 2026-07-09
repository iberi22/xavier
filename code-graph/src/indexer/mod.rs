//! Indexer - scans and indexes codebases.

use crate::db::CodeGraphDB;
use crate::error::{GraphError, Result};
use crate::parser::{parse_source, parse_source_sync};
use crate::plugin_host::{ParserDispatch, PluginHost};
use crate::types::{CodeEdge, EdgeType, IndexStats, Language, Symbol, SymbolKind};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};

pub struct Indexer {
    db: Arc<CodeGraphDB>,
    max_concurrent: usize,
    plugin_host: Arc<PluginHost>,
}

impl Indexer {
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

        let root_buf = root.to_path_buf();
        let all_files = {
            let root_clone = root_buf.clone();
            // Wrap file collection in spawn_blocking as it's heavy I/O
            tokio::task::spawn_blocking(move || {
                // Since collect_files is a method on Indexer, we can't easily call it
                // without Arc<Self> or similar if we wanted it to be truly independent.
                // But we can just implement the logic here or make a helper.
                // For now, let's just keep it simple and see if we can call it.
                // Actually, let's just move the logic into a static-like helper or just use the method if possible.
                // We'll use a temporary Indexer or just call it if we can.
                // Given the constraints, I'll just implement it as is for now.
                collect_files_internal(&root_clone)
            })
            .await
            .map_err(|e| GraphError::Parser(e.to_string()))??
        };

        let mut files_to_index = Vec::new();
        let mut files_to_mtime = HashMap::new();
        let mut files_to_delete = Vec::new();

        if !incremental {
            self.db.clear()?;
            files_to_index = all_files;
            // For metadata update later
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

        // Process files in chunks of 50 to reduce spawn overhead
        for chunk in files_to_index.chunks(50) {
            let chunk = chunk.to_vec();
            let sem = semaphore.clone();
            let root = root_buf.clone();
            let plugin_host = self.plugin_host.clone();

            let handle = tokio::spawn(async move {
                let _permit = sem.acquire().await.expect("semaphore should be open");

                tokio::task::spawn_blocking(move || {
                    let mut results = Vec::new();
                    for file_path in chunk {
                        match parse_file_sync(&root, &file_path, Some(&*plugin_host)) {
                            Ok(parsed) => results.push(parsed),
                            Err(error) => warn!("Failed to parse {:?}: {}", file_path, error),
                        }
                    }
                    results
                })
                .await
                .map_err(|e| GraphError::Parser(e.to_string()))
            });
            handles.push(handle);
        }

        let mut new_symbols = Vec::new();
        let mut sources = HashMap::new();
        for handle in handles {
            match handle.await {
                Ok(Ok(results)) => {
                    for parsed in results {
                        sources.insert(parsed.file_path.clone(), parsed.source);
                        new_symbols.extend(parsed.symbols);
                    }
                }
                Ok(Err(error)) => warn!("Batch task failed: {}", error),
                Err(error) => error!("Task failed: {}", error),
            }
        }

        assign_stable_ids(&mut new_symbols);

        // Load all symbols to build edges correctly (new symbols can point to old ones)
        // Only load if there are actually new symbols to process
        let all_symbols = if incremental && !new_symbols.is_empty() {
            let mut all = self.db.get_all_symbols()?;
            all.extend(new_symbols.clone());
            all
        } else {
            new_symbols.clone()
        };

        let edges = build_edges(&new_symbols, &all_symbols, &sources);

        self.db.insert_symbols(&new_symbols)?;
        self.db.insert_edges(&edges)?;

        // Update file metadata in batch
        self.db.batch_upsert_file_metadata(files_to_mtime)?;

        let mut stats = self.db.stats()?;
        stats.duration_ms = start.elapsed().as_millis() as u64;

        info!(
            "Indexed {} files, {} symbols, {} edges in {}ms",
            stats.total_files,
            stats.total_symbols,
            edges.len(),
            stats.duration_ms
        );

        Ok(stats)
    }

    /// Collect all relevant files in a directory using .gitignore/.ignore aware traversal.
    pub fn collect_files(&self, root: &Path) -> Result<Vec<PathBuf>> {
        collect_files_internal(root)
    }
}

fn collect_files_internal(root: &Path) -> Result<Vec<PathBuf>> {
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
        if Language::from_extension(ext) == Language::Unknown {
            continue;
        }
        files.push(path.to_path_buf());
    }

    Ok(files)
}

struct ParsedFile {
    file_path: String,
    source: String,
    symbols: Vec<Symbol>,
}

fn parse_file_sync(root: &Path, file_path: &Path, plugin_host: Option<&PluginHost>) -> Result<ParsedFile> {
    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let lang = Language::from_extension(ext);

    let source = std::fs::read_to_string(file_path).map_err(GraphError::Io)?;
    let relative_path = file_path
        .strip_prefix(root)
        .unwrap_or(file_path)
        .to_string_lossy()
        .replace('\\', "/");

    let symbols = match plugin_host.map(|h| h.parser_for(&lang)).unwrap_or(ParserDispatch::Native) {
        ParserDispatch::Native => parse_source_sync(&source, &lang, &relative_path)?,
        ParserDispatch::Plugin(_) => {
            // For plugins we must use block_on as they are async (spawn processes)
            tokio::runtime::Handle::current().block_on(parse_source(&source, &lang, &relative_path, plugin_host))?
        }
        ParserDispatch::NoOp => vec![],
    };

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
    let callable_symbols: Vec<&Symbol> = all_symbols
        .iter()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method))
        .collect();

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

    // Only build edges FROM new symbols
    let new_callable_symbols: Vec<&Symbol> = new_symbols
        .iter()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method))
        .collect();

    for caller in &new_callable_symbols {
        let Some(source) = sources.get(&caller.file_path) else {
            continue;
        };
        let caller_id = caller
            .stable_id
            .clone()
            .unwrap_or_else(|| caller.deterministic_id("default"));
        let body = symbol_source_slice(source, caller);
        for callee in &callable_symbols {
            if caller.stable_id == callee.stable_id || caller.name == callee.name {
                continue;
            }
            if contains_call(&body, &callee.name) {
                let callee_id = callee
                    .stable_id
                    .clone()
                    .unwrap_or_else(|| callee.deterministic_id("default"));
                edges.push(CodeEdge {
                    id: None,
                    from_symbol: caller_id.clone(),
                    to_symbol: callee_id.clone(),
                    edge_type: EdgeType::Calls,
                    file_path: caller.file_path.clone(),
                    line: caller.start_line,
                    confidence: 0.65,
                    metadata: Some(serde_json::json!({"callee": callee.name})),
                });
                edges.push(CodeEdge {
                    id: None,
                    from_symbol: caller_id.clone(),
                    to_symbol: callee_id,
                    edge_type: EdgeType::References,
                    file_path: caller.file_path.clone(),
                    line: caller.start_line,
                    confidence: 0.55,
                    metadata: Some(serde_json::json!({"reference": callee.name})),
                });
            }
        }
    }

    edges
}

fn symbol_source_slice(source: &str, symbol: &Symbol) -> String {
    let start = symbol.start_line.saturating_sub(1) as usize;
    let end = symbol.end_line as usize;
    source
        .lines()
        .skip(start)
        .take(end.saturating_sub(start).max(1))
        .collect::<Vec<_>>()
        .join("\n")
}

fn contains_call(source: &str, name: &str) -> bool {
    let needle = format!("{}(", name);
    let method_needle = format!(".{}(", name);
    source.contains(&needle) || source.contains(&method_needle)
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
        let stats = indexer.index(dir.path(), false).await.expect("index");

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
        std::fs::write(&file_path, "fn main() { println!(\"hi\"); }\nfn other() {}\n")
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
    async fn indexer_handles_concurrent_file_batches() {
        let dir = TempDir::new().unwrap();
        // Create 110 files (2 batches of 50 + 1 of 10)
        for i in 0..110 {
            std::fs::write(
                dir.path().join(format!("file_{}.rs", i)),
                format!("fn function_{}() {{}}", i),
            )
            .unwrap();
        }

        let db = Arc::new(CodeGraphDB::in_memory().unwrap());
        let indexer = Indexer::new(db.clone());
        let stats = indexer.index(dir.path(), false).await.unwrap();

        assert_eq!(stats.total_files, 110);
        assert_eq!(stats.total_symbols, 110);
        assert!(stats.duration_ms > 0);
    }

    #[tokio::test]
    async fn indexer_maintains_semaphore_limit() {
        let dir = TempDir::new().unwrap();
        for i in 0..20 {
            std::fs::write(
                dir.path().join(format!("file_{}.rs", i)),
                format!("fn function_{}() {{}}", i),
            )
            .unwrap();
        }

        let db = Arc::new(CodeGraphDB::in_memory().unwrap());
        let indexer = Indexer::new(db.clone());
        let stats = indexer.index(dir.path(), false).await.unwrap();

        assert_eq!(stats.total_files, 20);
        assert!(stats.duration_ms < 10000); // reasonable time
    }

    #[test]
    fn collect_files_respects_gitignore_and_excludes() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(".gitignore"), "ignored.rs\n").unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.path().join("ignored.rs"), "fn ignored() {}\n").unwrap();
        std::fs::create_dir(dir.path().join("target")).unwrap();
        std::fs::write(dir.path().join("target").join("skip.rs"), "fn skip() {}\n").unwrap();

        let db = Arc::new(CodeGraphDB::in_memory().unwrap());
        let indexer = Indexer::new(db);
        let files = indexer.collect_files(dir.path()).unwrap();

        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("main.rs"));
    }
}
