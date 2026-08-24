//! # Maturity Scanner — Deterministic Feature Completeness Measurement
//!
//! Measures Xavier's feature completeness deterministically using:
//! 1. **Code Graph** — static analysis of symbols (structs, fns, traits)
//! 2. **Test Anchors** — tests that validate each subcomponent
//! 3. **Feature Gates** — cfg(feature = "...") presence
//! 4. **Memory Evidence** — sessions, errors, usages (v2 deep-scan)
//! 5. **Conversation Evidence** — issues, PRs, discussions (v2 deep-scan)
//!
//! ## Modes
//!
//! - `scan` (v1, fast): uses grep-based fallback, no evidence layers
//! - `deep-scan` (v2, thorough): uses code graph DB + cargo test + evidence

pub mod anchor;
pub mod cli;
pub mod reporter;
pub mod scanner;
pub mod scorer;

#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::Arc;

use anchor::{AnchorManifest, FeatureAnchor};
use anyhow::Result;
use reporter::Summary;
use scorer::ScoredFeature;
use serde::{Deserialize, Serialize};

/// Result of a complete maturity scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaturityResult {
    pub features: Vec<ScoredFeature>,
    pub summary: Summary,
    pub scanned_at: String,
    pub head_commit: String,
    pub errors: Vec<String>,
    /// Timing info from deep-scan layers (empty when using v1 scan)
    #[serde(default)]
    pub layers: LayerTiming,
    /// Scan mode: "v1" (fast grep) or "v2" (deep analysis)
    #[serde(default)]
    pub scanner_version: String,
}

/// Timing for each scanning layer.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LayerTiming {
    pub static_ms: u64,
    pub dynamic_ms: u64,
    pub memory_ms: u64,
    pub conversations_ms: u64,
    pub total_ms: u64,
}

/// Progress callback type: receives a JSON string of partial results after each layer completes.
pub type ProgressCallback = Box<dyn Fn(&str) + Send + Sync>;

/// Main orchestrator for the maturity scanning pipeline.
pub struct MaturityScanner {
    /// Loaded anchor manifest
    manifest: Arc<AnchorManifest>,
    /// Codebase root path
    codebase_root: String,
    /// Whether to run deep scan (v2) or fast scan (v1)
    deep: bool,
    /// Optional progress callback invoked after each layer completes
    on_progress: Option<ProgressCallback>,
}

impl MaturityScanner {
    /// Create a new scanner with the given anchor manifest file and codebase root.
    pub fn new(anchor_path: &Path, codebase_root: &str) -> Result<Self> {
        let content = if !anchor_path.is_file() {
            let default_anchors = r#"{
  "version": "2.0.0",
  "generated": "2026-08-23T00:00:00Z",
  "features": [
    {
      "id": "memory-rag",
      "name": "Memory RAG",
      "priority": "high",
      "subcomponents": []
    }
  ]
}"#;
            if let Some(parent) = anchor_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::write(anchor_path, default_anchors) {
                Ok(_) => default_anchors.to_string(),
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "Maturity scanner anchors manifest not found at '{}' and auto-generation failed: {}.\n\n\
                        Action Required:\n\
                        Please create a valid anchors manifest file at '{}' with the following base structure:\n\n\
                        {{\n  \
                          \"version\": \"2.0.0\",\n  \
                          \"generated\": \"2026-06-19T02:00:00Z\",\n  \
                          \"features\": [\n    \
                            {{\n      \
                              \"id\": \"memory-rag\",\n      \
                              \"name\": \"Memory RAG\",\n      \
                              \"priority\": \"high\",\n      \
                              \"subcomponents\": []\n    \
                            }}\n  \
                          ]\n\
                        }}\n\n\
                        Ensure that the directory is writable, or specify a different anchors path using the '--anchors' option.",
                        anchor_path.display(),
                        e,
                        anchor_path.display()
                    ));
                }
            }
        } else {
            std::fs::read_to_string(anchor_path).map_err(|e| {
                anyhow::anyhow!(
                    "Cannot read anchor manifest '{}': {}",
                    anchor_path.display(),
                    e
                )
            })?
        };

        let manifest: AnchorManifest = serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Invalid anchor manifest JSON: {}", e))?;
        Ok(Self {
            manifest: Arc::new(manifest),
            codebase_root: codebase_root.to_string(),
            deep: false,
            on_progress: None,
        })
    }

    /// Enable deep scan mode (v2) — runs code graph, test anchors, memory, and conversations scanners.
    pub fn with_deep_scan(mut self) -> Self {
        self.deep = true;
        self
    }

    /// Set a progress callback invoked after each deep-scan layer completes with partial JSON.
    pub fn with_progress(mut self, cb: ProgressCallback) -> Self {
        self.on_progress = Some(cb);
        self
    }

    /// Run the full scanning pipeline.
    pub fn scan(&self) -> MaturityResult {
        if self.deep {
            self.deep_scan()
        } else {
            self.fast_scan()
        }
    }

    /// Fast scan (v1) — grep-based, no evidence layers.
    fn fast_scan(&self) -> MaturityResult {
        let mut errors = Vec::new();
        let features = self.manifest.features.clone();
        let mut scored_features = Vec::new();

        // Scan code graph for all unique symbols across features
        let all_symbols: Vec<String> = features
            .iter()
            .flat_map(|f| f.subcomponents.iter())
            .flat_map(|s| s.static_checks.iter().map(|c| c.symbol.clone()))
            .collect();
        let static_scan = scanner::old_types::scan_code_graph(&self.codebase_root, &all_symbols);

        for feature in &features {
            match self.scan_feature_v1(feature, &static_scan) {
                Ok(scored) => scored_features.push(scored),
                Err(e) => {
                    errors.push(format!("Feature '{}': {}", feature.id, e));
                    scored_features.push(ScoredFeature {
                        id: feature.id.clone(),
                        name: feature.name.clone(),
                        subcomponents: Vec::new(),
                        overall: 0.0,
                        status: "scan_error".to_string(),
                    });
                }
            }
        }

        let total: f64 = scored_features.iter().map(|f| f.overall).sum();
        let overall = if scored_features.is_empty() {
            0.0
        } else {
            (total / scored_features.len() as f64).round()
        };

        let production_ready = scored_features
            .iter()
            .filter(|f| f.status == "production_ready")
            .count();
        let needs_work = scored_features
            .iter()
            .filter(|f| f.status == "needs_work")
            .count();
        let in_progress = scored_features
            .iter()
            .filter(|f| f.status == "in_progress")
            .count();

        let summary = Summary {
            overall_maturity: overall as u8,
            total_features: scored_features.len(),
            production_ready,
            needs_work,
            in_progress,
            scan_errors: errors.len(),
        };

        MaturityResult {
            features: scored_features,
            summary,
            scanned_at: chrono::Utc::now().to_rfc3339(),
            head_commit: self.get_head_commit().unwrap_or_default(),
            errors,
            layers: LayerTiming::default(),
            scanner_version: "v1".to_string(),
        }
    }

    /// Deep scan (v2) — runs all 4 layers sequentially with per-layer timeouts.
    ///
    /// After each layer completes, flushes partial results to disk (when --write is used)
    /// so that even if a later layer fails, earlier results are preserved.
    fn deep_scan(&self) -> MaturityResult {
        let mut errors = Vec::new();
        let features = self.manifest.features.clone();

        // Execute scan_all with per-layer timeouts and progress callbacks
        let evidence = if let Some(ref cb) = self.on_progress {
            scanner::scan_all_with_progress(&self.codebase_root, Some(&|s| cb(s)))
        } else {
            scanner::scan_all(&self.codebase_root)
        };

        // Emit final evidence summary via callback
        if let Some(ref cb) = self.on_progress {
            let progress = serde_json::json!({
                "event": "evidence_complete",
                "layers": {
                    "static_ms": evidence.timing.static_ms,
                    "dynamic_ms": evidence.timing.dynamic_ms,
                    "memory_ms": evidence.timing.memory_ms,
                    "conversations_ms": evidence.timing.conversations_ms,
                    "total_ms": evidence.timing.total_ms,
                },
                "static_features": evidence.static_results.len(),
                "test_features": evidence.test_results.len(),
                "memory_features": evidence.memory_evidence.len(),
                "conversation_features": evidence.conversation_evidence.len(),
                "errors_count": evidence.errors.len(),
            });
            cb(&progress.to_string());
        }

        let mut scored_features = Vec::new();

        for feature in &features {
            let memory_ratio = evidence.memory_evidence.get(&feature.id).map(|m| m.ratio);
            let conv_ratio = evidence
                .conversation_evidence
                .get(&feature.id)
                .map(|c| c.ratio);

            match self.scan_feature_v2(feature, &evidence, memory_ratio, conv_ratio) {
                Ok(scored) => scored_features.push(scored),
                Err(e) => {
                    errors.push(format!("Feature '{}': {}", feature.id, e));
                    scored_features.push(ScoredFeature {
                        id: feature.id.clone(),
                        name: feature.name.clone(),
                        subcomponents: Vec::new(),
                        overall: 0.0,
                        status: "scan_error".to_string(),
                    });
                }
            }
        }

        let total: f64 = scored_features.iter().map(|f| f.overall).sum();
        let overall = if scored_features.is_empty() {
            0.0
        } else {
            (total / scored_features.len() as f64).round()
        };

        let production_ready = scored_features
            .iter()
            .filter(|f| f.status == "production_ready")
            .count();
        let needs_work = scored_features
            .iter()
            .filter(|f| f.status == "needs_work")
            .count();
        let in_progress = scored_features
            .iter()
            .filter(|f| f.status == "in_progress")
            .count();

        let summary = Summary {
            overall_maturity: overall as u8,
            total_features: scored_features.len(),
            production_ready,
            needs_work,
            in_progress,
            scan_errors: errors.len(),
        };

        MaturityResult {
            features: scored_features,
            summary,
            scanned_at: chrono::Utc::now().to_rfc3339(),
            head_commit: self.get_head_commit().unwrap_or_default(),
            errors: evidence.errors,
            layers: LayerTiming {
                static_ms: evidence.timing.static_ms,
                dynamic_ms: evidence.timing.dynamic_ms,
                memory_ms: evidence.timing.memory_ms,
                conversations_ms: evidence.timing.conversations_ms,
                total_ms: evidence.timing.total_ms,
            },
            scanner_version: "v2".to_string(),
        }
    }

    /// v1 scan: uses old grep-based symbol + test scanning.
    fn scan_feature_v1(
        &self,
        feature: &FeatureAnchor,
        static_scan: &scanner::CodeGraphScan,
    ) -> Result<ScoredFeature, String> {
        let mut scored_subs = Vec::new();

        for sub in &feature.subcomponents {
            let symbols: Vec<String> = sub.static_checks.iter().map(|c| c.symbol.clone()).collect();
            let static_found = symbols
                .iter()
                .filter(|s| static_scan.found.contains(*s))
                .count();
            let static_pass_rate = if symbols.is_empty() {
                1.0
            } else {
                static_found as f64 / symbols.len() as f64
            };

            let gate_ok = if let Some(ref gate) = sub.required_feature {
                self.check_feature_gate(gate)
            } else {
                true
            };

            let test_results = self.scan_tests(&sub.test_anchors);
            let test_pass_rate = if sub.test_anchors.is_empty() {
                1.0
            } else {
                let passed = test_results.iter().filter(|t| **t).count();
                passed as f64 / sub.test_anchors.len() as f64
            };

            let static_score = static_pass_rate * sub.weight as f64 * 0.40;
            let test_score = test_pass_rate * sub.weight as f64 * 0.50;
            let gate_score = if gate_ok {
                sub.weight as f64 * 0.10
            } else {
                0.0
            };
            let sub_score = (static_score + test_score + gate_score).round() as u8;

            scored_subs.push(scorer::ScoredSubcomponent {
                name: sub.name.clone(),
                weight: sub.weight,
                maturity: sub_score,
                static_pass_rate: (static_pass_rate * 100.0).round() as u8,
                test_pass_rate: (test_pass_rate * 100.0).round() as u8,
                gate_check: gate_ok,
                tests_passing: test_results.iter().filter(|t| **t).count(),
                tests_total: sub.test_anchors.len(),
                symbols_found: static_found as u8,
                symbols_total: symbols.len() as u8,
                memory_usage: 0,
                issue_health: 0,
                evidence_detail: String::new(),
            });
        }

        let total_weight: u32 = scored_subs.iter().map(|s| s.weight).sum();
        let weighted_sum: f64 = scored_subs
            .iter()
            .map(|s| s.maturity as f64 * s.weight as f64)
            .sum();
        let overall = if total_weight == 0 {
            0.0
        } else {
            (weighted_sum / total_weight as f64).round()
        };
        let status = if overall >= 90.0 {
            "production_ready"
        } else if overall >= 50.0 {
            "needs_work"
        } else {
            "in_progress"
        };

        Ok(ScoredFeature {
            id: feature.id.clone(),
            name: feature.name.clone(),
            subcomponents: scored_subs,
            overall,
            status: status.to_string(),
        })
    }

    /// v2 scan: uses deep scan evidence from all 4 layers.
    fn scan_feature_v2(
        &self,
        feature: &FeatureAnchor,
        evidence: &scanner::DeepScanEvidence,
        memory_ratio: Option<f64>,
        conv_ratio: Option<f64>,
    ) -> Result<ScoredFeature, String> {
        let mut scored_subs = Vec::new();
        let feat_id = &feature.id;

        let static_evidence = evidence.static_results.get(feat_id);
        let test_evidence = evidence.test_results.get(feat_id);

        for sub in &feature.subcomponents {
            let symbols: Vec<String> = sub.static_checks.iter().map(|c| c.symbol.clone()).collect();
            let symbols_total = symbols.len() as u8;

            let (static_found, static_pass_rate) = if let Some((f, t)) = static_evidence {
                // Use the aggregated found/total ratio for this feature
                let proportion = if *t > 0 { *f as f64 / *t as f64 } else { 1.0 };
                let sv = (proportion * symbols_total as f64).round() as usize;
                (sv.min(symbols_total as usize), proportion)
            } else {
                // fallback: grep
                let found = symbols.iter().filter(|s| self.grep_codebase(s)).count();
                let rate = if symbols_total == 0 {
                    1.0
                } else {
                    found as f64 / symbols_total as f64
                };
                (found, rate)
            };

            let gate_ok = if let Some(ref gate) = sub.required_feature {
                self.check_feature_gate(gate)
            } else {
                true
            };

            let (tests_passing, tests_total, test_pass_rate) =
                if let Some((passing, total)) = test_evidence {
                    // Per-subcomponent denominator: the rate must reflect THIS
                    // subcomponent's anchors, not the feature-wide aggregate.
                    // (previously used total.len() which is the feature total and
                    //  produced nonsensical rates when some subs had anchors and others
                    //  didn't). `total` here is the full anchor set for the feature.
                    let _ = total; // feature-wide set available for diagnostics
                    let sub_total = sub.test_anchors.len();
                    let passing_count = passing
                        .iter()
                        .filter(|p| sub.test_anchors.contains(*p))
                        .count();
                    let rate = if sub_total == 0 {
                        1.0
                    } else {
                        passing_count as f64 / sub_total as f64
                    };
                    (passing_count, sub_total, rate)
                } else {
                    let results = self.scan_tests(&sub.test_anchors);
                    let p = results.iter().filter(|t| **t).count();
                    let t2 = sub.test_anchors.len();
                    let rate = if t2 == 0 { 1.0 } else { p as f64 / t2 as f64 };
                    (p, t2, rate)
                };

            // Memory + conversation evidence
            let mem_ratio = memory_ratio.unwrap_or(0.0);
            let issue_ratio = conv_ratio.unwrap_or(0.0);

            // Weighted score with 5 metrics (v2 formula)
            let static_score = static_pass_rate * sub.weight as f64 * 0.35;
            let test_score = test_pass_rate * sub.weight as f64 * 0.35;
            let gate_score = if gate_ok {
                sub.weight as f64 * 0.10
            } else {
                0.0
            };
            let memory_score = mem_ratio * sub.weight as f64 * 0.10;
            let conversation_score = issue_ratio * sub.weight as f64 * 0.10;

            let sub_score = (static_score
                + test_score
                + gate_score
                + memory_score
                + conversation_score)
                .round() as u8;

            let mut detail = format!(
                "static: {}/{} ({}%), tests: {}/{} ({}%)",
                static_found,
                symbols_total,
                (static_pass_rate * 100.0).round(),
                tests_passing,
                tests_total,
                (test_pass_rate * 100.0).round()
            );
            if memory_ratio.is_some() {
                detail.push_str(&format!(" | memory: {:.0}%", mem_ratio * 100.0));
            }
            if conv_ratio.is_some() {
                detail.push_str(&format!(" | issues: {:.0}%", issue_ratio * 100.0));
            }

            scored_subs.push(scorer::ScoredSubcomponent {
                name: sub.name.clone(),
                weight: sub.weight,
                maturity: sub_score,
                static_pass_rate: (static_pass_rate * 100.0).round() as u8,
                test_pass_rate: (test_pass_rate * 100.0).round() as u8,
                gate_check: gate_ok,
                tests_passing,
                tests_total,
                symbols_found: static_found as u8,
                symbols_total,
                memory_usage: (mem_ratio * 100.0).round() as u8,
                issue_health: (issue_ratio * 100.0).round() as u8,
                evidence_detail: detail,
            });
        }

        let total_weight: u32 = scored_subs.iter().map(|s| s.weight).sum();
        let weighted_sum: f64 = scored_subs
            .iter()
            .map(|s| s.maturity as f64 * s.weight as f64)
            .sum();
        let overall = if total_weight == 0 {
            0.0
        } else {
            (weighted_sum / total_weight as f64).round()
        };
        let status = if overall >= 90.0 {
            "production_ready"
        } else if overall >= 50.0 {
            "needs_work"
        } else {
            "in_progress"
        };

        Ok(ScoredFeature {
            id: feature.id.clone(),
            name: feature.name.clone(),
            subcomponents: scored_subs,
            overall,
            status: status.to_string(),
        })
    }

    fn check_feature_gate(&self, gate: &str) -> bool {
        let cargo_path = format!("{}/Cargo.toml", self.codebase_root);
        if let Ok(content) = std::fs::read_to_string(&cargo_path) {
            if content.contains(&format!("{} = [\"dep:", gate))
                || content.contains(&format!("\"{}\"", gate))
                || content.contains(&format!("{} = [", gate))
            {
                return true;
            }
        }
        false
    }

    fn scan_tests(&self, tests: &[String]) -> Vec<bool> {
        if tests.is_empty() {
            return Vec::new();
        }
        tests
            .iter()
            .map(|test_name| self.grep_codebase(&format!("fn {}", test_name)))
            .collect()
    }

    fn grep_codebase(&self, pattern: &str) -> bool {
        let root = &self.codebase_root;

        let tags_path = format!("{}/.xavier/tags", root);
        if let Ok(content) = std::fs::read_to_string(&tags_path) {
            if content.contains(pattern) {
                return true;
            }
        }

        let walker = walkdir::WalkDir::new(root)
            .max_depth(8)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
            .take(200);

        for entry in walker {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if content.contains(pattern) {
                    return true;
                }
            }
        }
        false
    }

    fn get_head_commit(&self) -> Result<String, String> {
        let output = std::process::Command::new("git")
            .args(["-C", &self.codebase_root, "rev-parse", "HEAD"])
            .output()
            .map_err(|e| format!("git error: {}", e))?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}
