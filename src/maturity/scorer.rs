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
use crate::maturity::anchor::FeatureAnchor;
use crate::maturity::scanner::{CodeGraphScan, TestListScan};

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
const STATIC_WEIGHT: f64 = 0.35;   // 35% — code symbols exist
const TEST_WEIGHT: f64 = 0.35;     // 35% — tests pass
const GATE_WEIGHT: f64 = 0.10;    // 10% — feature gates configured
const MEMORY_WEIGHT: f64 = 0.10;  // 10% — evidence from sessions/code
const ISSUE_WEIGHT: f64 = 0.10;   // 10% — evidence from discussions

/// Score one feature with data from all 4 scanning layers.
///
/// Takes optional memory and conversation evidence (0.0 when not available).
pub fn score_feature(
    feature: &FeatureAnchor,
    static_scan: &CodeGraphScan,
    test_scan: &TestListScan,
    memory_evidence: Option<f64>,
    conversation_evidence: Option<f64>,
) -> ScoredFeature {
    let mut scored_subs = Vec::new();

    for sub in &feature.subcomponents {
        // --- Static analysis ---
        let symbols: Vec<String> = sub.static_checks.iter().map(|c| c.symbol.clone()).collect();
        let static_found = symbols.iter().filter(|s| static_scan.found.contains(*s)).count();
        let static_pass_rate = if symbols.is_empty() {
            1.0
        } else {
            static_found as f64 / symbols.len() as f64
        };

        // --- Feature gate ---
        let gate_ok = if let Some(ref gate) = sub.required_feature {
            test_scan.all_tests.iter().any(|t| t.contains(gate))
                || static_scan.found.contains(gate)
        } else {
            true
        };

        // --- Tests ---
        let test_count = sub.test_anchors.len();
        let tests_passing = if test_count == 0 {
            0
        } else {
            sub.test_anchors.iter()
                .filter(|t| test_scan.matching.contains(t.as_str()))
                .count()
        };
        let test_pass_rate = if test_count == 0 { 1.0 } else { tests_passing as f64 / test_count as f64 };

        // --- Memory evidence ---
        let mem_ratio = memory_evidence.unwrap_or(0.0);

        // --- Conversation evidence ---
        let conv_ratio = conversation_evidence.unwrap_or(0.0);

        // --- Weighted score calculation ---
        let static_score = static_pass_rate * sub.weight as f64 * STATIC_WEIGHT;
        let test_score = test_pass_rate * sub.weight as f64 * TEST_WEIGHT;
        let gate_score = if gate_ok { sub.weight as f64 * GATE_WEIGHT } else { 0.0 };
        let memory_score = mem_ratio * sub.weight as f64 * MEMORY_WEIGHT;
        let issue_score = conv_ratio * sub.weight as f64 * ISSUE_WEIGHT;

        let sub_score = (static_score + test_score + gate_score + memory_score + issue_score).round() as u8;

        // Build evidence detail string
        let mut detail_parts = Vec::new();
        detail_parts.push(format!("static: {}/{}", static_found, symbols.len()));
        detail_parts.push(format!("tests: {}/{}", tests_passing, test_count));
        if gate_ok && sub.required_feature.is_some() {
            detail_parts.push(format!("gate: {}", sub.required_feature.as_ref().unwrap()));
        }
        if memory_evidence.is_some() {
            detail_parts.push(format!("memory: {:.0}%", mem_ratio * 100.0));
        }
        if conversation_evidence.is_some() {
            detail_parts.push(format!("issues: {:.0}%", conv_ratio * 100.0));
        }

        scored_subs.push(ScoredSubcomponent {
            name: sub.name.clone(),
            weight: sub.weight,
            maturity: sub_score,
            static_pass_rate: (static_pass_rate * 100.0).round() as u8,
            test_pass_rate: (test_pass_rate * 100.0).round() as u8,
            gate_check: gate_ok,
            tests_passing,
            tests_total: test_count,
            symbols_found: static_found as u8,
            symbols_total: symbols.len() as u8,
            memory_usage: (mem_ratio * 100.0).round() as u8,
            issue_health: (conv_ratio * 100.0).round() as u8,
            evidence_detail: detail_parts.join(" | "),
        });
    }

    // Weighted average for overall
    let total_weight: u32 = scored_subs.iter().map(|s| s.weight).sum();
    let weighted_sum: f64 = scored_subs.iter().map(|s| s.maturity as f64 * s.weight as f64).sum();
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

    ScoredFeature {
        id: feature.id.clone(),
        name: feature.name.clone(),
        subcomponents: scored_subs,
        overall,
        status: status.to_string(),
    }
}
