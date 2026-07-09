//! Indexer - scans and indexes codebases.

pub mod watcher;

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
    pub async fn index(&self, root: &Path) -> Result<IndexStats> {
        let start = Instant::now();
        info!("Starting indexing of {:?}", root);

        let files = self.collect_files(root)?;
        info!("Found {} files to index", files.len());

        self.db.clear()?;

        let semaphore = Arc::new(Semaphore::new(self.max_concurrent));
        let mut handles = Vec::new();

        for file_path in files {
            let sem = semaphore.clone();
            let root = root.to_path_buf();
            let plugin_host = self.plugin_host.clone();
            let handle = tokio::spawn(async move {
                let _permit = sem.acquire().await.expect("semaphore should be open");
                parse_file(&root, &file_path, Some(&*plugin_host)).await
            });
            handles.push(handle);
        }

        let mut symbols = Vec::new();
        let mut sources = HashMap::new();
        for handle in handles {
            match handle.await {
                Ok(Ok(parsed)) => {
                    sources.insert(parsed.file_path.clone(), parsed.source);
                    symbols.extend(parsed.symbols);
                }
                Ok(Err(error)) => warn!("Failed to parse file: {}", error),
                Err(error) => error!("Task failed: {}", error),
            }
        }

        assign_stable_ids(&mut symbols);
        let edges = build_edges(&symbols, &sources);

        self.db.insert_symbols(&symbols)?;
        self.db.insert_edges(&edges)?;

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
            if Language::from_extension(ext) == Language::Unknown {
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

async fn parse_file(root: &Path, file_path: &Path, plugin_host: Option<&PluginHost>) -> Result<ParsedFile> {
    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let lang = Language::from_extension(ext);

    // We try to parse even if lang is Unknown because a plugin might handle it by extension
    // But for now, we follow the discovery logic in PluginHost::parser_for which might return NoOp

    let source = std::fs::read_to_string(file_path).map_err(GraphError::Io)?;
    let relative_path = file_path
        .strip_prefix(root)
        .unwrap_or(file_path)
        .to_string_lossy()
        .replace('\\', "/");
    let symbols = parse_source(&source, &lang, &relative_path, plugin_host).await?;
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

fn build_edges(symbols: &[Symbol], sources: &HashMap<String, String>) -> Vec<CodeEdge> {
    let mut edges = Vec::new();
    let callable_symbols: Vec<&Symbol> = symbols
        .iter()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method))
        .collect();

    for symbol in symbols {
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

    for caller in &callable_symbols {
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
    let name_first = name.as_bytes()[0];
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
        let stats = indexer.index(dir.path()).await.expect("index");

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
}
