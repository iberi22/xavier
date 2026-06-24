//! # Fallback CodeGraph Adapter — Minimal Built-in Scanner
//!
//! This adapter implements `CodeGraphPort` using the built-in maturity
//! scanner (grep-based, no tree-sitter). It is the **fallback** when the
//! external `codegraph-plugin` is not available.
//!
//! ## Design
//!
//! - **Minimal**: only the essential logic for symbol verification
//! - **Self-contained**: uses only stdlib + serde_json + walkdir
//! - **Fast-enough**: < 5s for most codebases (capped at 300 files)
//! - **Graceful degradation**: works without any external database
//!
//! ## When to Use
//!
//! - Plugin binary not installed
//! - Plugin server not responding
//! - Offline mode
//! - Quick local scans where tree-sitter precision isn't critical

use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;
use std::time::Instant;

use crate::ports::code_graph::{CodeGraphPort, DesignGap, DesignStatus, DesignVerification, FeatureScan, PluginStatus, ScanResult};

/// The built-in fallback code intelligence adapter.
///
/// Uses a simple grep-based approach to verify symbol existence
/// by scanning `.rs` files in the codebase.
pub struct FallbackCodeGraphAdapter {
    /// Max files to scan (safety limit)
    max_files: usize,
}

impl Default for FallbackCodeGraphAdapter {
    fn default() -> Self {
        Self { max_files: 300 }
    }
}

impl FallbackCodeGraphAdapter {
    pub fn new(max_files: usize) -> Self {
        Self { max_files }
    }
}

// ── Static symbol cache ──────────────────────────────────

static ANCHOR_SYMBOLS: OnceLock<HashMap<String, Vec<String>>> = OnceLock::new();

/// Load symbols from features.json / maturity-anchors.json.
fn load_feature_symbols(features_path: &Path) -> HashMap<String, Vec<String>> {
    if let Some(cached) = ANCHOR_SYMBOLS.get() {
        return cached.clone();
    }

    let mut map: HashMap<String, Vec<String>> = HashMap::new();

    if let Ok(content) = std::fs::read_to_string(features_path) {
        if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(features) = manifest.get("features").and_then(|f| f.as_array()) {
                for feat in features {
                    let id = feat.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
                    let mut symbols = Vec::new();
                    if let Some(subs) = feat.get("subcomponents").and_then(|s| s.as_array()) {
                        for sub in subs {
                            if let Some(checks) = sub.get("static_checks").and_then(|c| c.as_array()) {
                                for check in checks {
                                    if let Some(sym) = check.get("symbol").and_then(|v| v.as_str()) {
                                        symbols.push(sym.to_string());
                                    }
                                }
                            }
                            // Also check for top-level "symbol" field
                            if let Some(sym) = sub.get("symbol").and_then(|v| v.as_str()) {
                                symbols.push(sym.to_string());
                            }
                        }
                    }
                    map.insert(id.to_string(), symbols);
                }
            }
        }
    }

    let _ = ANCHOR_SYMBOLS.set(map.clone());
    map
}

/// Scan .rs files in the codebase looking for symbols.
fn grep_codebase(root: &Path, all_symbols: &HashSet<&str>, max_files: usize) -> (HashSet<String>, Vec<String>) {
    let mut found: HashSet<String> = HashSet::new();
    let mut errors: Vec<String> = Vec::new();

    let walker = walkdir::WalkDir::new(root)
        .max_depth(10)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().map_or(false, |ext| ext == "rs")
                && !e.path().to_string_lossy().contains("target")
                && !e.path().to_string_lossy().contains("node_modules")
        });

    for (i, entry) in walker.enumerate() {
        if i >= max_files {
            break;
        }
        if found.len() >= all_symbols.len() {
            break; // All symbols found, early exit
        }
        match std::fs::read_to_string(entry.path()) {
            Ok(content) => {
                for sym in all_symbols {
                    if !found.contains(*sym) && content.contains(sym) {
                        found.insert(sym.to_string());
                    }
                }
            }
            Err(e) => errors.push(format!("Cannot read {}: {}", entry.path().display(), e)),
        }
    }

    (found, errors)
}

/// Try to load code-graph JSON dump for fast symbol lookup.
fn try_json_dump(root: &Path, feature_symbols: &HashMap<String, Vec<String>>) -> Option<ScanResult> {
    let start = Instant::now();
    let json_path = root.join(".xavier/codegraph.json");

    let content = std::fs::read_to_string(&json_path).ok()?;

    let mut feature_scans = Vec::new();
    let total_checks: usize = feature_symbols.values().map(|v| v.len()).sum();
    let mut total_found = 0;

    for (feat_id, symbols) in feature_symbols {
        let (found, missing): (Vec<_>, Vec<_>) = symbols
            .iter()
            .partition(|s| content.contains(s.as_str()));

        total_found += found.len();
        let pct = if symbols.is_empty() {
            100.0
        } else {
            (found.len() as f64 / symbols.len() as f64) * 100.0
        };

        let found_count = found.len();
        let missing_count = missing.len();
        feature_scans.push(FeatureScan {
            feature_id: feat_id.clone(),
            found: found.into_iter().map(|s| s.clone()).collect(),
            missing: missing.into_iter().map(|s| s.clone()).collect(),
            maturity_pct: pct,
            detail: format!("{}/{} symbols verified via code-graph JSON dump", found_count, missing_count + found_count),
        });
    }

    let timing_ms = start.elapsed().as_millis() as u64;
    Some(ScanResult {
        feature_scans,
        total_checks,
        total_found,
        errors: Vec::new(),
        timing_ms,
    })
}

// ── Implementation ────────────────────────────────────────

#[async_trait]
impl CodeGraphPort for FallbackCodeGraphAdapter {
    async fn scan_maturity(
        &self,
        features_path: &Path,
        codebase_root: &Path,
    ) -> Result<ScanResult, String> {
        let start = Instant::now();
        let feature_symbols = load_feature_symbols(features_path);

        if feature_symbols.is_empty() {
            return Err(format!(
                "No features found in '{}'. Ensure the file has a 'features' array with 'id' and 'subcomponents'.",
                features_path.display()
            ));
        }

        // Try JSON dump first (fast path)
        if let Some(result) = try_json_dump(codebase_root, &feature_symbols) {
            return Ok(result);
        }

        // Fallback: grep codebase
        let all_symbols: HashSet<&str> = feature_symbols
            .values()
            .flat_map(|v| v.iter().map(|s| s.as_str()))
            .collect();

        let (found_symbols, errors) = grep_codebase(codebase_root, &all_symbols, self.max_files);

        let mut feature_scans = Vec::new();
        let total_checks: usize = feature_symbols.values().map(|v| v.len()).sum();
        let mut total_found = 0;

        for (feat_id, symbols) in &feature_symbols {
            let (found, missing): (Vec<_>, Vec<_>) = symbols
                .iter()
                .partition(|s| found_symbols.contains(*s));

            total_found += found.len();
            let pct = if symbols.is_empty() {
                100.0
            } else {
                (found.len() as f64 / symbols.len() as f64) * 100.0
            };

            let found_count = found.len();
            let missing_count = missing.len();
            feature_scans.push(FeatureScan {
                feature_id: feat_id.clone(),
                found: found.into_iter().map(|s| s.clone()).collect(),
                missing: missing.into_iter().map(|s| s.clone()).collect(),
                maturity_pct: pct,
                detail: format!("{}/{} symbols verified via grep (fallback)", found_count, missing_count + found_count),
            });
        }

        let timing_ms = start.elapsed().as_millis() as u64;
        Ok(ScanResult {
            feature_scans,
            total_checks,
            total_found,
            errors,
            timing_ms,
        })
    }

    async fn verify_design(
        &self,
        features_path: &Path,
        codebase_root: &Path,
        feature_id: Option<&str>,
    ) -> Result<DesignVerification, String> {
        let result = self.scan_maturity(features_path, codebase_root).await?;

        let (scans, target) = if let Some(fid) = feature_id {
            let filtered: Vec<_> = result.feature_scans.iter()
                .filter(|s| s.feature_id == fid)
                .cloned()
                .collect();
            if filtered.is_empty() {
                return Err(format!("Feature '{}' not found in features file", fid));
            }
            (filtered, fid.to_string())
        } else {
            (result.feature_scans.clone(), "all features".to_string())
        };

        let total: usize = scans.iter().map(|s| s.found.len() + s.missing.len()).sum();
        let verified: usize = scans.iter().map(|s| s.found.len()).sum();
        let gaps: Vec<String> = scans.iter()
            .flat_map(|s| s.missing.iter().cloned())
            .collect();

        let status = if total == 0 {
            DesignStatus::Unknown
        } else if verified == total {
            DesignStatus::Verified
        } else if verified as f64 / total as f64 >= 0.5 {
            DesignStatus::Partial
        } else {
            DesignStatus::Failed
        };

        Ok(DesignVerification {
            target,
            total_symbols: total,
            verified,
            gaps,
            status,
        })
    }

    async fn design_gaps(
        &self,
        features_path: &Path,
        codebase_root: &Path,
    ) -> Result<Vec<DesignGap>, String> {
        let result = self.scan_maturity(features_path, codebase_root).await?;

        let gaps: Vec<DesignGap> = result.feature_scans
            .iter()
            .flat_map(|fs| {
                fs.missing.iter().map(|sym| DesignGap {
                    symbol: sym.clone(),
                    context: format!("feature '{}'", fs.feature_id),
                    suggestion: Some(format!(
                        "Implement or document '{}' module/struct in the codebase",
                        sym
                    )),
                })
            })
            .collect();

        Ok(gaps)
    }

    async fn health(&self) -> PluginStatus {
        PluginStatus {
            available: true, // Fallback is always available
            version: Some("fallback-v0.12.0 (built-in grep scanner)".to_string()),
            uptime_secs: None,
            features: vec![
                "maturity-scan".to_string(),
                "verify-design".to_string(),
                "design-gaps".to_string(),
            ],
        }
    }
}

