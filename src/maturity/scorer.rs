// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! # Scorer — Deterministic Scoring Engine
//!
//! Applies the scoring formula to produce a deterministic maturity percentage
//! for each feature.
//!
//! ## Formula (v2 — 5 metrics)
//!
//! ```text
//! sub_score = static_pass_rate × weight × STATIC_WEIGHT (0.35)  // symbols exist
//!           + test_pass_rate × weight × TEST_WEIGHT (0.35)      // tests pass
//!           + gate_ok × weight × GATE_WEIGHT (0.10)             // feature gate configured
//!           + memory_evidence × weight × MEMORY_WEIGHT (0.10)   // evidence from sessions/code
//!           + issue_health × weight × ISSUE_WEIGHT (0.10)       // evidence from discussions
//!
//! feature_score = Σ(sub_score) / Σ(weight) × 100
//! ```

use serde::{Deserialize, Serialize};

/// Result of scoring one subcomponent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredSubcomponent {
    pub name: String,
    pub weight: u32,
    pub maturity: u8,
    pub static_pass_rate: u8,
    pub test_pass_rate: u8,
    pub gate_check: bool,
    pub tests_passing: usize,
    pub tests_total: usize,
    pub symbols_found: u8,
    pub symbols_total: u8,
    /// Evidence from agent memory / session analysis (Layer 3), 0-100%
    #[serde(default)]
    pub memory_usage: u8,
    /// Evidence from conversations / issues analysis (Layer 4), 0-100%
    #[serde(default)]
    pub issue_health: u8,
    /// Human-readable detail about the evidence
    #[serde(default)]
    pub evidence_detail: String,
}

/// Result of scoring one feature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredFeature {
    pub id: String,
    pub name: String,
    pub subcomponents: Vec<ScoredSubcomponent>,
    pub overall: f64,
    pub status: String,
}

/// Scoring weights — v2: 5 metrics with more balanced distribution.
pub const STATIC_WEIGHT: f64 = 0.35; // 35% — code symbols exist
pub const TEST_WEIGHT: f64 = 0.35; // 35% — tests pass
pub const GATE_WEIGHT: f64 = 0.10; // 10% — feature gates configured
pub const MEMORY_WEIGHT: f64 = 0.10; // 10% — evidence from sessions/code
pub const ISSUE_WEIGHT: f64 = 0.10; // 10% — evidence from discussions

/// Determine the maturity status bucket from an overall score.
pub fn status_for_score(overall: f64) -> &'static str {
    if overall >= 90.0 {
        "production_ready"
    } else if overall >= 50.0 {
        "needs_work"
    } else {
        "in_progress"
    }
}

// NOTE: The legacy `score_feature` function that lived here was dead code — it was
// never called (deep-scan uses `scan_feature_v2` in mod.rs, which builds
// `ScoredSubcomponent` directly) and referenced a non-existent `test_scan.matching`
// field, so it could not have compiled if invoked. It has been removed. The scoring
// is now performed inline in `scan_feature_v1`/`scan_feature_v2` (src/maturity/mod.rs).
