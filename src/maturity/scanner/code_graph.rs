//! # Code Graph Scanner — Layer 1: Static Code Analysis
//!
//! Uses Xavier's code-graph database (JSON dump in .xavier/) to quickly
//! resolve symbol existence. Falls back to file grep if code-graph is
//! unavailable (graceful degradation).
//!
//! Timing target: < 100ms with code graph, < 5s without.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use code_graph::db::CodeGraphDB;

/// Helper to resolve the `.xavier/code_graph.db` path.
pub fn resolve_code_graph_db_path(codebase_root: &str) -> PathBuf {
    crate::codebase::codegraph_paths::code_graph_db_path_for(Path::new(codebase_root))
}

/// Attempt to open the CodeGraphDB if the file exists.
/// Resolves paths in order: canonical db path helper, codebase_root/.xavier/code_graph.db, codebase_root/data/code_graph.db
pub fn try_open_code_graph_db(codebase_root: &str) -> Option<CodeGraphDB> {
    let db_path = resolve_code_graph_db_path(codebase_root);
    if db_path.exists() {
        if let Ok(db) = CodeGraphDB::new(&db_path) {
            return Some(db);
        }
    }

    let xavier_db_path = Path::new(codebase_root).join(".xavier").join("code_graph.db");
    if xavier_db_path.exists() {
        if let Ok(db) = CodeGraphDB::new(&xavier_db_path) {
            return Some(db);
        }
    }

    let data_db_path = Path::new(codebase_root).join("data").join("code_graph.db");
    if data_db_path.exists() {
        if let Ok(db) = CodeGraphDB::new(&data_db_path) {
            return Some(db);
        }
    }

    None
}

/// Results for one feature's static symbols.
#[derive(Debug, Clone)]
pub struct SymbolScan {
    pub found: HashSet<String>,
    pub missing: HashSet<String>,
}

/// Full code graph scan result across all known feature symbols.
#[derive(Debug, Clone)]
pub struct CodeGraphScanResult {
    /// Per-feature-id scan results
    pub feature_scans: HashMap<String, SymbolScan>,
    /// Total symbols checked
    pub total_symbols: usize,
    /// Total found
    pub total_found: usize,
    /// Errors during scanning
    pub errors: Vec<String>,
    /// Timing in ms
    pub timing_ms: u64,
}

/// Known features and their required symbols (loaded from anchors).
static ANCHORED_SYMBOLS: OnceLock<HashMap<String, Vec<String>>> = OnceLock::new();

/// Initialize anchored symbols from the manifests JSON file.
/// Returns the map of feature_id -> list of required symbols.
fn load_anchored_symbols(codebase_root: &str) -> HashMap<String, Vec<String>> {
    if let Some(cached) = ANCHORED_SYMBOLS.get() {
        return cached.clone();
    }

    let anchor_path = Path::new(codebase_root).join(".xavier/maturity-anchors.json");
    let mut map: HashMap<String, Vec<String>> = HashMap::new();

    if let Ok(content) = std::fs::read_to_string(&anchor_path) {
        if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(features) = manifest.get("features").and_then(|f| f.as_array()) {
                for feat in features {
                    let id = feat.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
                    let mut symbols = Vec::new();
                    if let Some(subs) = feat.get("subcomponents").and_then(|s| s.as_array()) {
                        for sub in subs {
                            if let Some(checks) =
                                sub.get("static_checks").and_then(|c| c.as_array())
                            {
                                for check in checks {
                                    if let Some(sym) = check.get("symbol").and_then(|v| v.as_str())
                                    {
                                        symbols.push(sym.to_string());
                                    }
                                }
                            }
                        }
                    }
                    map.insert(id.to_string(), symbols);
                }
            }
        }
    }

    // Cache it (ignore poison error since content is the same)
    let _ = ANCHORED_SYMBOLS.set(map.clone());
    map
}

/// Try to load code graph DB and scan for symbols.
/// Falls back through:
/// 1. SQLite database (if usable, i.e., file exists and total_symbols > 0)
/// 2. Portable JSON dump (if codegraph.json exists and parsed symbols > 0)
///
/// If both fail, returns None (caller will then fallback to grep_fallback).
fn try_code_graph_db(
    root: &str,
    feature_symbols: &HashMap<String, Vec<String>>,
) -> Option<CodeGraphScanResult> {
    // Stage 1: Try the real SQLite database directly
    let db_start = Instant::now();
    if let Some(db) = try_open_code_graph_db(root) {
        if let Ok(stats) = db.stats() {
            if stats.total_symbols > 0 {
                let mut feature_scans: HashMap<String, SymbolScan> = HashMap::new();
                let mut errors = Vec::new();

                for (feat_id, symbols) in feature_symbols {
                    let mut found = HashSet::new();
                    let mut missing = HashSet::new();

                    for sym in symbols {
                        match db.find_symbols(sym, 5) {
                            Ok(results) => {
                                let exists = results.symbols.iter().any(|s| s.name == *sym);
                                if exists {
                                    found.insert(sym.clone());
                                } else {
                                    missing.insert(sym.clone());
                                }
                            }
                            Err(e) => {
                                errors.push(format!("Database error querying {sym}: {e}"));
                                missing.insert(sym.clone());
                            }
                        }
                    }

                    feature_scans.insert(feat_id.clone(), SymbolScan { found, missing });
                }

                let total_symbols: usize = feature_symbols.values().map(|v| v.len()).sum();
                let total_found: usize = feature_scans.values().map(|s| s.found.len()).sum();
                let timing_ms = db_start.elapsed().as_millis() as u64;

                return Some(CodeGraphScanResult {
                    feature_scans,
                    total_symbols,
                    total_found,
                    errors,
                    timing_ms,
                });
            }
        }
    }

    // Stage 2: Fallback to JSON dump if SQLite is missing or empty
    let json_start = Instant::now();
    let json_path = Path::new(root).join(".xavier/codegraph.json");

    let content = std::fs::read_to_string(&json_path)
        .map_err(|_| -> std::io::Error {
            // Code dump would recurse — skip
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no code graph",
            )
        })
        .ok()?;

    // Guard: if the codegraph dump has no real symbols (e.g. a stale test fixture or
    // an empty dump from a server whose code-graph DB was never indexed), substring
    // matching against it reports every symbol as missing and tanks the static score.
    // Fall through to None so the grep fallback (which reads source directly) runs.
    let parsed = serde_json::from_str::<serde_json::Value>(&content).ok();
    let real_symbols = parsed
        .as_ref()
        .and_then(|v| v.get("symbols"))
        .and_then(|s| s.as_array())
        .map_or(0, |s| s.len());
    if real_symbols == 0 {
        return None;
    }

    let mut feature_scans: HashMap<String, SymbolScan> = HashMap::new();

    for (feat_id, symbols) in feature_symbols {
        let mut found = HashSet::new();
        let mut missing = HashSet::new();

        for sym in symbols {
            if content.contains(sym.as_str()) {
                found.insert(sym.clone());
            } else {
                missing.insert(sym.clone());
            }
        }

        feature_scans.insert(feat_id.clone(), SymbolScan { found, missing });
    }

    let total_symbols: usize = feature_symbols.values().map(|v| v.len()).sum();
    let total_found: usize = feature_scans.values().map(|s| s.found.len()).sum();
    let timing_ms = json_start.elapsed().as_millis() as u64;

    Some(CodeGraphScanResult {
        feature_scans,
        total_symbols,
        total_found,
        errors: Vec::new(),
        timing_ms,
    })
}

/// Fallback grep-based scanner — reads .rs files looking for symbols.
fn grep_fallback(
    root: &str,
    feature_symbols: &HashMap<String, Vec<String>>,
) -> CodeGraphScanResult {
    let start = Instant::now();
    let mut errors = Vec::new();

    // Collect all unique symbols
    let all_symbols: HashSet<&str> = feature_symbols
        .values()
        .flat_map(|v| v.iter().map(|s| s.as_str()))
        .collect();

    // Scan files once — track which symbols are found
    let mut found_symbols: HashSet<String> = HashSet::new();

    let walker = walkdir::WalkDir::new(root)
        .max_depth(8)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().is_some_and(|ext| ext == "rs")
                && !e.path().to_string_lossy().contains("target")
        })
        .take(300); // Safety limit

    for entry in walker {
        if found_symbols.len() >= all_symbols.len() {
            break; // All symbols found, stop early
        }
        match std::fs::read_to_string(entry.path()) {
            Ok(content) => {
                for sym in &all_symbols {
                    if !found_symbols.contains(*sym) && content.contains(sym) {
                        found_symbols.insert(sym.to_string());
                    }
                }
            }
            Err(e) => {
                errors.push(format!("Cannot read {}: {}", entry.path().display(), e));
            }
        }
    }

    // Build per-feature results
    let mut feature_scans: HashMap<String, SymbolScan> = HashMap::new();
    for (feat_id, symbols) in feature_symbols {
        let mut found = HashSet::new();
        let mut missing = HashSet::new();
        for sym in symbols {
            if found_symbols.contains(sym) {
                found.insert(sym.clone());
            } else {
                missing.insert(sym.clone());
            }
        }
        feature_scans.insert(feat_id.clone(), SymbolScan { found, missing });
    }

    let total_symbols: usize = feature_symbols.values().map(|v| v.len()).sum();
    let total_found: usize = feature_scans.values().map(|s| s.found.len()).sum();
    let timing_ms = start.elapsed().as_millis() as u64;

    CodeGraphScanResult {
        feature_scans,
        total_symbols,
        total_found,
        errors,
        timing_ms,
    }
}

/// Main entry: scan code graph for all known feature symbols.
/// Tries code graph JSON first, falls back to grep.
pub fn scan_code_graph(codebase_root: &str) -> CodeGraphScanResult {
    let feature_symbols = load_anchored_symbols(codebase_root);

    if feature_symbols.is_empty() {
        return CodeGraphScanResult {
            feature_scans: HashMap::new(),
            total_symbols: 0,
            total_found: 0,
            errors: vec!["No anchors found in .xavier/maturity-anchors.json".to_string()],
            timing_ms: 0,
        };
    }

    // Try code graph DB first
    if let Some(result) = try_code_graph_db(codebase_root, &feature_symbols) {
        return result;
    }

    // Fallback to grep
    grep_fallback(codebase_root, &feature_symbols)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_code_graph_db_path() {
        let path = resolve_code_graph_db_path(".");
        assert!(!path.as_os_str().is_empty());
    }

    #[test]
    fn test_try_open_code_graph_db_nonexistent() {
        let db = try_open_code_graph_db("/nonexistent/directory/path");
        assert!(db.is_none());
    }

    #[test]
    fn test_try_open_code_graph_db_with_mock_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join(".xavier");
        std::fs::create_dir_all(&db_path).unwrap();

        // Before creating, opening should return None
        let db = try_open_code_graph_db(&temp_dir.path().to_string_lossy());
        assert!(db.is_none());

        // Create a real database
        let real_db_path = db_path.join("code_graph.db");
        let _ = CodeGraphDB::create_new(&real_db_path).unwrap();

        // After creating, opening should return Some
        let db = try_open_code_graph_db(&temp_dir.path().to_string_lossy());
        assert!(db.is_some());
    }

    #[test]
    fn test_try_code_graph_db_with_sqlite() {
        let temp_dir = tempfile::tempdir().unwrap();
        let xavier_path = temp_dir.path().join(".xavier");
        std::fs::create_dir_all(&xavier_path).unwrap();

        // Create a real database
        let db_file_path = xavier_path.join("code_graph.db");
        let db = CodeGraphDB::create_new(&db_file_path).unwrap();

        let mut feature_symbols = HashMap::new();
        feature_symbols.insert("feat-test".to_string(), vec!["test_symbol".to_string()]);

        // Scenario 1: Empty database must not find any symbols and should return None
        // since there's no JSON fallback either.
        let result = try_code_graph_db(&temp_dir.path().to_string_lossy(), &feature_symbols);
        assert!(result.is_none());

        // Scenario 2: DB with symbols should scan and find/miss symbols correctly.
        let symbol = code_graph::Symbol {
            id: None,
            stable_id: None,
            name: "test_symbol".to_string(),
            kind: code_graph::SymbolKind::Function,
            lang: code_graph::Language::Rust,
            file_path: "src/test.rs".to_string(),
            start_line: 1,
            end_line: 5,
            start_col: 0,
            end_col: 10,
            signature: Some("fn test_symbol()".to_string()),
            parent: None,
            complexity: Some(1.0),
        };
        db.insert_symbol(&symbol).unwrap();

        let result = try_code_code_graph_db_with_sqlite_helper(&temp_dir.path().to_string_lossy(), &feature_symbols);
        assert!(result.is_some());
        let scan_res = result.unwrap();
        assert_eq!(scan_res.total_symbols, 1);
        assert_eq!(scan_res.total_found, 1);

        let feat_scan = scan_res.feature_scans.get("feat-test").unwrap();
        assert!(feat_scan.found.contains("test_symbol"));
        assert!(feat_scan.missing.is_empty());
    }

    #[test]
    fn test_try_code_graph_db_fallback_to_json_dump() {
        let temp_dir = tempfile::tempdir().unwrap();
        let xavier_path = temp_dir.path().join(".xavier");
        std::fs::create_dir_all(&xavier_path).unwrap();

        // No SQLite DB exists, but let's create a non-empty JSON dump
        let json_path = xavier_path.join("codegraph.json");
        let dump_data = serde_json::json!({
            "symbols": [
                {
                    "name": "test_symbol_json",
                    "kind": "Function"
                }
            ],
            "edges": [],
            "hotspots": [],
            "hubs": []
        });
        std::fs::write(&json_path, serde_json::to_string(&dump_data).unwrap()).unwrap();

        let mut feature_symbols = HashMap::new();
        feature_symbols.insert("feat-test".to_string(), vec!["test_symbol_json".to_string()]);

        let result = try_code_graph_db(&temp_dir.path().to_string_lossy(), &feature_symbols);
        assert!(result.is_some());
        let scan_res = result.unwrap();
        assert_eq!(scan_res.total_symbols, 1);
        assert_eq!(scan_res.total_found, 1);
        assert!(scan_res.feature_scans.get("feat-test").unwrap().found.contains("test_symbol_json"));
    }

    #[test]
    fn test_try_code_graph_db_empty_json_falls_back() {
        let temp_dir = tempfile::tempdir().unwrap();
        let xavier_path = temp_dir.path().join(".xavier");
        std::fs::create_dir_all(&xavier_path).unwrap();

        // Create an empty JSON dump (0 symbols)
        let json_path = xavier_path.join("codegraph.json");
        let dump_data = serde_json::json!({
            "symbols": [],
            "edges": [],
            "hotspots": [],
            "hubs": []
        });
        std::fs::write(&json_path, serde_json::to_string(&dump_data).unwrap()).unwrap();

        let mut feature_symbols = HashMap::new();
        feature_symbols.insert("feat-test".to_string(), vec!["test_symbol_json".to_string()]);

        let result = try_code_graph_db(&temp_dir.path().to_string_lossy(), &feature_symbols);
        // It must return None so that the caller can fall back to grep
        assert!(result.is_none());
    }

    #[test]
    fn test_scan_code_graph_missing_all_graceful_grep() {
        let temp_dir = tempfile::tempdir().unwrap();
        // Create an anchor file so that we have feature symbols to look for
        let xavier_path = temp_dir.path().join(".xavier");
        std::fs::create_dir_all(&xavier_path).unwrap();

        let anchor_path = xavier_path.join("maturity-anchors.json");
        let anchor_data = serde_json::json!({
            "features": [
                {
                    "id": "feat-cool",
                    "subcomponents": [
                        {
                            "static_checks": [
                                {
                                    "symbol": "cool_fn_symbol"
                                }
                            ]
                        }
                    ]
                }
            ]
        });
        std::fs::write(&anchor_path, serde_json::to_string(&anchor_data).unwrap()).unwrap();

        // Now create a Rust source file containing that symbol under temp_dir
        let src_dir = temp_dir.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        let rust_file = src_dir.join("main.rs");
        std::fs::write(&rust_file, "fn cool_fn_symbol() {}").unwrap();

        // Scan code graph with completely missing DB and dump
        let scan_res = scan_code_graph(&temp_dir.path().to_string_lossy());
        // Since DB and dump are missing, it should gracefully fall back to grep and find the symbol
        assert_eq!(scan_res.total_symbols, 1);
        assert_eq!(scan_res.total_found, 1);
        assert!(scan_res.feature_scans.get("feat-cool").unwrap().found.contains("cool_fn_symbol"));
    }

    #[test]
    fn test_scan_code_graph_empty_dump_falls_back_to_grep() {
        let temp_dir = tempfile::tempdir().unwrap();
        let xavier_path = temp_dir.path().join(".xavier");
        std::fs::create_dir_all(&xavier_path).unwrap();

        let anchor_path = xavier_path.join("maturity-anchors.json");
        let anchor_data = serde_json::json!({
            "features": [
                {
                    "id": "feat-cool",
                    "subcomponents": [
                        {
                            "static_checks": [
                                {
                                    "symbol": "cool_fn_symbol"
                                }
                            ]
                        }
                    ]
                }
            ]
        });
        std::fs::write(&anchor_path, serde_json::to_string(&anchor_data).unwrap()).unwrap();

        // Create an empty json dump
        let json_path = xavier_path.join("codegraph.json");
        let dump_data = serde_json::json!({
            "symbols": [],
            "edges": [],
            "hotspots": [],
            "hubs": []
        });
        std::fs::write(&json_path, serde_json::to_string(&dump_data).unwrap()).unwrap();

        // Now create a Rust source file containing that symbol under temp_dir
        let src_dir = temp_dir.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        let rust_file = src_dir.join("main.rs");
        std::fs::write(&rust_file, "fn cool_fn_symbol() {}").unwrap();

        // Scan code graph with empty dump
        let scan_res = scan_code_graph(&temp_dir.path().to_string_lossy());
        // Since DB is missing and dump is empty, it should gracefully fall back to grep and find the symbol
        assert_eq!(scan_res.total_symbols, 1);
        assert_eq!(scan_res.total_found, 1);
        assert!(scan_res
            .feature_scans
            .get("feat-cool")
            .unwrap()
            .found
            .contains("cool_fn_symbol"));
    }

    #[test]
    fn test_scan_code_graph_empty_sqlite_falls_back_to_grep() {
        let temp_dir = tempfile::tempdir().unwrap();
        let xavier_path = temp_dir.path().join(".xavier");
        std::fs::create_dir_all(&xavier_path).unwrap();

        let anchor_path = xavier_path.join("maturity-anchors.json");
        let anchor_data = serde_json::json!({
            "features": [
                {
                    "id": "feat-cool",
                    "subcomponents": [
                        {
                            "static_checks": [
                                {
                                    "symbol": "cool_fn_symbol"
                                }
                            ]
                        }
                    ]
                }
            ]
        });
        std::fs::write(&anchor_path, serde_json::to_string(&anchor_data).unwrap()).unwrap();

        // Create an empty SQLite DB
        let db_file_path = xavier_path.join("code_graph.db");
        let _db = CodeGraphDB::create_new(&db_file_path).unwrap();

        // Ensure codegraph.json is missing

        // Now create a Rust source file containing that symbol under temp_dir
        let src_dir = temp_dir.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        let rust_file = src_dir.join("main.rs");
        std::fs::write(&rust_file, "fn cool_fn_symbol() {}").unwrap();

        // Scan code graph with empty SQLite DB
        let scan_res = scan_code_graph(&temp_dir.path().to_string_lossy());
        // Since DB is empty and dump is missing, it should gracefully fall back to grep and find the symbol
        assert_eq!(scan_res.total_symbols, 1);
        assert_eq!(scan_res.total_found, 1);
        assert!(scan_res
            .feature_scans
            .get("feat-cool")
            .unwrap()
            .found
            .contains("cool_fn_symbol"));
    }
}

// Helper to avoid test name colliding/referencing itself incorrectly
fn try_code_code_graph_db_with_sqlite_helper(
    root: &str,
    feature_symbols: &HashMap<String, Vec<String>>,
) -> Option<CodeGraphScanResult> {
    try_code_graph_db(root, feature_symbols)
}
