//! # Test Scanner — Layer 2: Dynamic Test Analysis
//!
//! Loads test_anchors from the anchors manifest, then quickly checks
//! whether each anchor name exists as a string in .rs source files.
//!
//! Performance: constrained to depth=4, max 50 .rs files, hard 30s timeout.
//!
//! Timing target: < 5s.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};

/// Result of scanning tests against known features.
#[derive(Debug, Clone)]
pub struct TestListScanResult {
    /// All discovered test names (fully qualified)
    pub all_tests: Vec<String>,
    /// Per-feature-id: (list of passing test names, list of total anchor names)
    pub feature_tests: HashMap<String, (Vec<String>, Vec<String>)>,
    /// Errors
    pub errors: Vec<String>,
    /// Timing in ms
    pub timing_ms: u64,
}

/// Load feature -> test_anchors mapping from anchors manifest.
fn load_anchored_tests(codebase_root: &str) -> HashMap<String, Vec<String>> {
    let anchor_path = Path::new(codebase_root).join(".xavier/maturity-anchors.json");
    let mut map: HashMap<String, Vec<String>> = HashMap::new();

    if let Ok(content) = std::fs::read_to_string(&anchor_path) {
        if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(features) = manifest.get("features").and_then(|f| f.as_array()) {
                for feat in features {
                    let id = feat.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
                    let mut tests = Vec::new();
                    let mut seen = HashSet::new();
                    if let Some(subs) = feat.get("subcomponents").and_then(|s| s.as_array()) {
                        for sub in subs {
                            if let Some(anchors) = sub.get("test_anchors").and_then(|a| a.as_array()) {
                                for anchor in anchors {
                                    if let Some(name) = anchor.as_str() {
                                        if seen.insert(name.to_string()) {
                                            tests.push(name.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    map.insert(id.to_string(), tests);
                }
            }
        }
    }
    map
}

/// Check if test anchor names exist as substrings in .rs source files.
///
/// Constrained to depth=4 relative to root, max 50 files, 30s hard timeout.
fn check_test_anchors_in_sources(
    root: &str,
    unique_anchors: &[String],
) -> (HashSet<String>, Vec<String>) {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut found: HashSet<String> = HashSet::new();

    // Walk .rs files with depth=4, limit 50 files
    let walker: Vec<_> = walkdir::WalkDir::new(root)
        .max_depth(4)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().map_or(false, |ext| ext == "rs")
                && !e.path().to_string_lossy().contains("target")
        })
        .take(50)
        .collect();

    for entry in &walker {
        // Hard timeout check
        if Instant::now() > deadline {
            break;
        }

        let content = match std::fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Check every anchor against this file's content
        for anchor in unique_anchors {
            if found.contains(anchor) {
                continue;
            }
            // Check for the anchor name as a substring (e.g. "context::bm25::tests::empty_query_or_limit_returns_no_hits")
            if content.contains(anchor.as_str()) {
                found.insert(anchor.clone());
            }
            // Also check for the short name (last component after ::)
            if let Some(short) = anchor.rsplit("::").next() {
                let fn_pattern = format!("fn {}", short);
                if content.contains(&fn_pattern) {
                    found.insert(anchor.clone());
                }
            }
        }

        // Early exit if all anchors found
        if found.len() == unique_anchors.len() {
            break;
        }
    }

    let mut errors = Vec::new();
    if Instant::now() > deadline {
        errors.push("Test scanner: hit 30s timeout while checking anchors".to_string());
    }

    (found, errors)
}

/// Main entry: check test anchors against source files.
///
/// Does NOT run `cargo test --list` (avoids cargo lock contention).
/// Instead loads test_anchors from the anchors manifest and verifies
/// they exist as strings in the codebase (depth=4, max 50 files, 30s timeout).
pub fn list_tests(codebase_root: &str) -> TestListScanResult {
    let start = Instant::now();
    let feature_tests_map = load_anchored_tests(codebase_root);
    let mut errors: Vec<String> = Vec::new();
    // Collect all unique test anchors across all features
    let all_anchors: Vec<String> = feature_tests_map
        .values()
        .flat_map(|v| v.iter().cloned())
        .collect();

    // Remove duplicates
    let unique_anchors: Vec<String> = {
        let mut seen = HashSet::new();
        all_anchors
            .iter()
            .filter(|a| seen.insert(a.to_string()))
            .cloned()
            .collect()
    };

    // Check which anchors exist in the codebase (depth=4, 50 files, 30s timeout)
    let (found_anchors, scan_errors) = check_test_anchors_in_sources(codebase_root, &unique_anchors);
    errors.extend(scan_errors);

    // Build per-feature results
    let mut feature_results: HashMap<String, (Vec<String>, Vec<String>)> = HashMap::new();
    for (feat_id, anchor_names) in &feature_tests_map {
        let (passing, missing): (Vec<_>, Vec<_>) = anchor_names
            .iter()
            .cloned()
            .partition(|anchor| found_anchors.contains(anchor));

        feature_results.insert(feat_id.clone(), (passing, missing));
    }

    let all_tests: Vec<String> = found_anchors.into_iter().collect();
    let timing_ms = start.elapsed().as_millis() as u64;

    TestListScanResult {
        all_tests,
        feature_tests: feature_results,
        errors,
        timing_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_anchored_tests_does_not_panic() {
        let result = load_anchored_tests(".");
        // Should at least return something or empty, but never panic
        let _ = result;
    }
}
