//! # Code Graph Scanner — Layer 1: Static Code Analysis
//!
//! Uses Xavier's code-graph database (JSON dump in .xavier/) to quickly
//! resolve symbol existence. Falls back to file grep if code-graph is
//! unavailable (graceful degradation).
//!
//! Timing target: < 100ms with code graph, < 5s without.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;
use std::time::Instant;

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
fn try_code_graph_db(
    root: &str,
    feature_symbols: &HashMap<String, Vec<String>>,
) -> Option<CodeGraphScanResult> {
    let start = Instant::now();
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
    let timing_ms = start.elapsed().as_millis() as u64;

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
