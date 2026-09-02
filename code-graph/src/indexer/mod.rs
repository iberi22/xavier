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
use crate::types::{CodeEdge, EdgeType, IndexStats, Language, Symbol, SymbolEmbedder, SymbolKind};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};

pub mod call_resolution;
use call_resolution::{extract_call_names, CallResolver};

/// Kind of path-level change for [`Indexer::apply_paths`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathChangeKind {
    Added,
    Modified,
    Deleted,
    /// `from` is the previous relative path; [`PathChange::path`] is the new path.
    Renamed {
        from: String,
    },
}

/// Explicit file delta for git-driven (or caller-driven) incremental updates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathChange {
    /// Relative path within the index root (new path for renames).
    pub path: String,
    pub kind: PathChangeKind,
}

impl PathChange {
    pub fn added(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            kind: PathChangeKind::Added,
        }
    }

    pub fn modified(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            kind: PathChangeKind::Modified,
        }
    }

    pub fn deleted(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            kind: PathChangeKind::Deleted,
        }
    }

    pub fn renamed(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            path: to.into(),
            kind: PathChangeKind::Renamed { from: from.into() },
        }
    }
}

pub struct Indexer {
    db: Arc<CodeGraphDB>,
    max_concurrent: usize,
    plugin_host: Arc<PluginHost>,
    embedder: Option<Arc<dyn SymbolEmbedder>>,
}

impl Indexer {
    pub fn new(db: Arc<CodeGraphDB>) -> Self {
        Self {
            db,
            max_concurrent: 8,
            plugin_host: Arc::new(PluginHost::new()),
            embedder: None,
        }
    }

    pub fn with_embedder(db: Arc<CodeGraphDB>, embedder: Arc<dyn SymbolEmbedder>) -> Self {
        Self {
            db,
            max_concurrent: 8,
            plugin_host: Arc::new(PluginHost::new()),
            embedder: Some(embedder),
        }
    }

    pub fn set_embedder(&mut self, embedder: Arc<dyn SymbolEmbedder>) {
        self.embedder = Some(embedder);
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

        let mut stats = self
            .parse_and_persist(root, files_to_index, files_to_mtime, incremental)
            .await?;
        stats.duration_ms = start.elapsed().as_millis() as u64;

        info!(
            "Indexed {} files, {} symbols in {}ms",
            stats.total_files, stats.total_symbols, stats.duration_ms
        );

        Ok(stats)
    }

    /// Apply an explicit path-list delta (add / update / delete / rename).
    ///
    /// Unlike [`Indexer::index`] (mtime walk), this always reindexes the given
    /// paths. Used by `xavier code sync --git`.
    ///
    /// Edge repair:
    /// 1. Collect stable_ids for files about to be deleted/reparsed.
    /// 2. Delete **incident** edges (from or to those ids).
    /// 3. Reparse one-hop caller files that previously pointed at those ids.
    /// 4. After persist, prune dangling edges to missing symbol ids.
    ///
    /// Structural `stable_id` (v2) excludes `start_line`, so intra-file moves
    /// keep identity; renames still change path and therefore the id.
    pub async fn apply_paths(&self, root: &Path, changes: &[PathChange]) -> Result<IndexStats> {
        let start = Instant::now();
        if changes.is_empty() {
            let mut stats = self.db.stats()?;
            stats.duration_ms = start.elapsed().as_millis() as u64;
            return Ok(stats);
        }

        let mut delete_paths: Vec<String> = Vec::new();
        let mut index_rel_paths: Vec<String> = Vec::new();

        for change in changes {
            let path = normalize_rel_path(&change.path);
            match &change.kind {
                PathChangeKind::Added | PathChangeKind::Modified => {
                    delete_paths.push(path.clone());
                    index_rel_paths.push(path);
                }
                PathChangeKind::Deleted => {
                    delete_paths.push(path);
                }
                PathChangeKind::Renamed { from } => {
                    delete_paths.push(normalize_rel_path(from));
                    delete_paths.push(path.clone());
                    index_rel_paths.push(path);
                }
            }
        }

        delete_paths.sort();
        delete_paths.dedup();
        index_rel_paths.sort();
        index_rel_paths.dedup();

        // Incident-edge cleanup + one-hop caller reparsing.
        let old_stable_ids = self.db.stable_ids_for_files(&delete_paths)?;
        let mut caller_files = self.db.files_with_edges_to(&old_stable_ids)?;
        caller_files.retain(|p| !delete_paths.contains(p) && !index_rel_paths.contains(p));
        for caller in &caller_files {
            delete_paths.push(caller.clone());
            index_rel_paths.push(caller.clone());
        }
        delete_paths.sort();
        delete_paths.dedup();
        index_rel_paths.sort();
        index_rel_paths.dedup();

        if !old_stable_ids.is_empty() {
            let removed = self.db.delete_edges_referencing_symbols(&old_stable_ids)?;
            if removed > 0 {
                debug!(
                    "Removed {} incident edges targeting reindexed symbols",
                    removed
                );
            }
        }

        self.db.batch_delete_file_data(&delete_paths)?;

        let mut files_to_index = Vec::new();
        let mut files_to_mtime = HashMap::new();
        for rel in &index_rel_paths {
            let abs = root.join(rel);
            if !abs.is_file() {
                continue;
            }
            let ext = abs.extension().and_then(|e| e.to_str()).unwrap_or("");
            if Language::from_extension_with_plugins(ext, self.plugin_host.discovery())
                == Language::Unknown
            {
                continue;
            }
            let (rel_norm, mtime) = get_file_info(root, &abs);
            files_to_mtime.insert(rel_norm, mtime);
            files_to_index.push(abs);
        }

        info!(
            "apply_paths: {} deletes, {} files to parse ({} caller rebuilds)",
            delete_paths.len(),
            files_to_index.len(),
            caller_files.len()
        );

        if files_to_index.is_empty() {
            let pruned = self.db.prune_dangling_edges().unwrap_or(0);
            if pruned > 0 {
                debug!("Pruned {} dangling edges after path deletes", pruned);
            }
            let mut stats = self.db.stats()?;
            stats.duration_ms = start.elapsed().as_millis() as u64;
            return Ok(stats);
        }

        let mut stats = self
            .parse_and_persist(root, files_to_index, files_to_mtime, true)
            .await?;
        let pruned = self.db.prune_dangling_edges().unwrap_or(0);
        if pruned > 0 {
            debug!("Pruned {} dangling edges after apply_paths", pruned);
        }
        stats.duration_ms = start.elapsed().as_millis() as u64;
        Ok(stats)
    }

    /// Alias for [`Indexer::apply_paths`].
    pub async fn apply_file_delta(
        &self,
        root: &Path,
        changes: &[PathChange],
    ) -> Result<IndexStats> {
        self.apply_paths(root, changes).await
    }

    /// Shared parse → edges → persist pipeline used by full and path-list index.
    async fn parse_and_persist(
        &self,
        root: &Path,
        files_to_index: Vec<PathBuf>,
        files_to_mtime: HashMap<String, i64>,
        incremental: bool,
    ) -> Result<IndexStats> {
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

        let all_symbols = if incremental && !new_symbols.is_empty() {
            let mut all = self.db.get_all_symbols()?;
            all.extend(new_symbols.clone());
            all
        } else {
            new_symbols.clone()
        };

        let edges = build_edges(&new_symbols, &all_symbols, &sources);
        let edges_len = edges.len();
        let new_symbols_len = new_symbols.len();

        // Generate embeddings for new symbols if embedder is configured
        let mut symbol_embeddings = Vec::new();
        if let Some(ref embedder) = self.embedder {
            for symbol in &new_symbols {
                if let Some(ref stable_id) = symbol.stable_id {
                    let text_to_embed = format!(
                        "{} {} {}",
                        symbol.name,
                        symbol.signature.as_deref().unwrap_or(""),
                        symbol.file_path
                    );
                    if let Ok(vec) = embedder.embed(&text_to_embed).await {
                        if !vec.is_empty() {
                            symbol_embeddings.push((stable_id.clone(), vec));
                        }
                    }
                }
            }
        }

        let db = Arc::clone(&self.db);
        let stats = tokio::task::spawn_blocking(move || {
            db.insert_symbols(&new_symbols)?;
            db.insert_edges(&edges)?;
            if !symbol_embeddings.is_empty() {
                let batch_refs: Vec<(&str, &[f32])> = symbol_embeddings
                    .iter()
                    .map(|(id, vec)| (id.as_str(), vec.as_slice()))
                    .collect();
                let _ = db.insert_symbol_embeddings_batch(&batch_refs);
            }
            db.batch_upsert_file_metadata(files_to_mtime)?;
            db.checkpoint_wal()?;
            db.stats()
        })
        .await
        .map_err(|e| GraphError::Io(std::io::Error::other(e)))??;

        info!(
            "Indexed batch: {} symbols, {} edges (db totals: {} files / {} symbols)",
            new_symbols_len, edges_len, stats.total_files, stats.total_symbols
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

    // Only build edges FROM new symbols
    let new_callable_symbols: Vec<&Symbol> = new_symbols
        .iter()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method))
        .collect();

    let source_lines: HashMap<&str, Vec<&str>> = sources
        .iter()
        .map(|(k, v)| (k.as_str(), v.lines().collect()))
        .collect();

    let resolver = CallResolver::new(all_symbols, sources);

    for caller in &new_callable_symbols {
        let Some(lines) = source_lines.get(caller.file_path.as_str()) else {
            continue;
        };
        let caller_id = caller
            .stable_id
            .clone()
            .unwrap_or_else(|| caller.deterministic_id("default"));

        let start = caller.start_line.saturating_sub(1) as usize;
        let end = caller.end_line as usize;
        let body = if start < lines.len() {
            let take_len = end.saturating_sub(start).max(1);
            let end_idx = (start + take_len).min(lines.len());
            lines[start..end_idx].join("\n")
        } else {
            String::new()
        };

        let callee_names = extract_call_names(&body);

        for name in callee_names {
            let resolved = resolver.resolve(&caller.file_path, &name);
            for res in resolved {
                if res.stable_id == caller_id {
                    continue;
                }
                edges.push(CodeEdge {
                    id: None,
                    from_symbol: caller_id.clone(),
                    to_symbol: res.stable_id.clone(),
                    edge_type: EdgeType::Calls,
                    file_path: caller.file_path.clone(),
                    line: caller.start_line,
                    confidence: res.confidence,
                    metadata: Some(serde_json::json!({
                        "callee": name,
                        "strategy": res.strategy
                    })),
                });

                edges.push(CodeEdge {
                    id: None,
                    from_symbol: caller_id.clone(),
                    to_symbol: res.stable_id,
                    edge_type: EdgeType::References,
                    file_path: caller.file_path.clone(),
                    line: caller.start_line,
                    confidence: res.confidence * 0.8,
                    metadata: Some(serde_json::json!({
                        "reference": name,
                        "strategy": res.strategy
                    })),
                });
            }
        }
    }

    edges
}

#[allow(dead_code)]
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

fn normalize_rel_path(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").to_string()
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
    async fn apply_paths_adds_updates_and_deletes() {
        let dir = TempDir::new().expect("temp dir");
        let a = dir.path().join("a.rs");
        let b = dir.path().join("b.rs");
        std::fs::write(&a, "fn alpha() {}\n").expect("write a");
        std::fs::write(&b, "fn beta() {}\n").expect("write b");

        let db = Arc::new(CodeGraphDB::in_memory().expect("db"));
        let indexer = Indexer::new(db.clone());

        let stats = indexer
            .apply_paths(
                dir.path(),
                &[PathChange::added("a.rs"), PathChange::added("b.rs")],
            )
            .await
            .expect("add");
        assert_eq!(stats.total_files, 2);
        assert!(stats.total_symbols >= 2);

        std::fs::write(&a, "fn alpha() {}\nfn alpha_extra() {}\n").expect("update a");
        let stats2 = indexer
            .apply_paths(dir.path(), &[PathChange::modified("a.rs")])
            .await
            .expect("modify");
        assert_eq!(stats2.total_files, 2);
        let symbols = db.get_all_symbols().expect("symbols");
        assert!(
            symbols.iter().any(|s| s.name == "alpha_extra"),
            "expected reparsed symbol alpha_extra"
        );
        assert!(
            symbols.iter().any(|s| s.name == "beta"),
            "untouched file should remain"
        );

        std::fs::remove_file(&b).expect("remove b");
        let stats3 = indexer
            .apply_paths(dir.path(), &[PathChange::deleted("b.rs")])
            .await
            .expect("delete");
        assert_eq!(stats3.total_files, 1);
        let symbols = db.get_all_symbols().expect("symbols");
        assert!(symbols.iter().all(|s| s.file_path == "a.rs"));
    }

    #[tokio::test]
    async fn apply_paths_keeps_structural_id_when_symbol_moves() {
        let dir = TempDir::new().expect("temp dir");
        std::fs::write(dir.path().join("m.rs"), "fn helper() {}\n").expect("write");

        let db = Arc::new(CodeGraphDB::in_memory().expect("db"));
        let indexer = Indexer::new(db.clone());
        indexer
            .apply_paths(dir.path(), &[PathChange::added("m.rs")])
            .await
            .expect("add");
        let before = db.find_by_file("m.rs").expect("syms");
        let helper = before.iter().find(|s| s.name == "helper").expect("helper");
        let id_before = helper.stable_id.clone().expect("id");

        std::fs::write(dir.path().join("m.rs"), "// pad\n// pad\nfn helper() {}\n").expect("move");
        indexer
            .apply_paths(dir.path(), &[PathChange::modified("m.rs")])
            .await
            .expect("modify");
        let after = db.find_by_file("m.rs").expect("syms");
        let helper2 = after
            .iter()
            .find(|s| s.name == "helper")
            .expect("helper after");
        assert_eq!(
            helper2.stable_id.as_deref(),
            Some(id_before.as_str()),
            "structural stable_id must survive line moves"
        );
        assert!(helper2.start_line > 1);
    }

    #[tokio::test]
    async fn apply_paths_handles_rename_and_clears_incoming() {
        let dir = TempDir::new().expect("temp dir");
        std::fs::write(dir.path().join("lib.rs"), "pub fn helper() {}\n").expect("lib");
        std::fs::write(dir.path().join("main.rs"), "fn main() { helper(); }\n").expect("main");

        let db = Arc::new(CodeGraphDB::in_memory().expect("db"));
        let indexer = Indexer::new(db.clone());
        indexer
            .index(dir.path(), false)
            .await
            .expect("initial index");

        let before_ids = db
            .stable_ids_for_files(&["lib.rs".to_string()])
            .expect("ids");
        assert!(!before_ids.is_empty());

        std::fs::rename(dir.path().join("lib.rs"), dir.path().join("util.rs")).expect("rename");
        indexer
            .apply_paths(dir.path(), &[PathChange::renamed("lib.rs", "util.rs")])
            .await
            .expect("rename apply");

        let symbols = db.get_all_symbols().expect("symbols");
        assert!(symbols.iter().any(|s| s.file_path == "util.rs"));
        assert!(!symbols.iter().any(|s| s.file_path == "lib.rs"));

        for id in before_ids {
            let to = db.find_edges_to(&id, None, 10).unwrap_or_default();
            assert!(
                to.is_empty(),
                "stale incoming edges to old stable_id should be cleared"
            );
        }
    }

    #[test]
    fn build_edges_10k_symbols_benchmark() {
        let mut symbols = Vec::with_capacity(10_000);
        let mut sources = HashMap::new();

        for f in 0..1000 {
            let file_path = format!("src/file_{}.rs", f);
            let mut file_source = String::new();
            for s in 0..10 {
                let sym_name = format!("func_{}_{}", f, s);
                let next_sym = format!("func_{}_{}", f, (s + 1) % 10);
                file_source.push_str(&format!(
                    "fn {}() {{\n    {}();\n    extern_call_{}();\n}}\n\n",
                    sym_name, next_sym, s
                ));
                symbols.push(Symbol {
                    name: sym_name.clone(),
                    file_path: file_path.clone(),
                    kind: SymbolKind::Function,
                    lang: Language::Rust,
                    start_line: (s * 5 + 1) as u32,
                    end_line: (s * 5 + 4) as u32,
                    stable_id: Some(format!("sym:{}:{}", file_path, sym_name)),
                    ..Default::default()
                });
            }
            sources.insert(file_path, file_source);
        }

        let start = std::time::Instant::now();
        let edges = build_edges(&symbols, &symbols, &sources);
        let duration = start.elapsed();

        println!(
            "DEBUG BENCHMARK DURATION: {:.4}s, edges count: {}",
            duration.as_secs_f64(),
            edges.len()
        );
        assert!(
            duration.as_secs_f64() < 5.0,
            "build_edges took {:.2}s, expected < 5s (O(n) hash-map; original double-loop ~40s+)",
            duration.as_secs_f64()
        );
    }
}
