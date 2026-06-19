//! # Scorer — Deterministic Scoring Engine
//!
//! Applies the scoring formula to produce a deterministic maturity percentage
//! for each feature.

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

/// Scoring weights.
const STATIC_WEIGHT: f64 = 0.40; // 40% — code symbols exist
const TEST_WEIGHT: f64 = 0.50;   // 50% — tests pass
const GATE_WEIGHT: f64 = 0.10;   // 10% — feature gates configured

/// Score one feature based on code graph scan and test list scan.
///
/// Formula:
/// ```text
/// subcomponent_score = static_pass_rate × weight × 0.4
///                    + test_pass_rate × weight × 0.5
///                    + gate_ok × weight × 0.1
///
/// feature_score = Σ(subcomponent_score) / Σ(weight) × 100
/// ```
pub fn score_feature(
    feature: &FeatureAnchor,
    code_scan: &CodeGraphScan,
    test_scan: &TestListScan,
) -> ScoredFeature {
    let mut scored_subs = Vec::new();

    for sub in &feature.subcomponents {
        // Static analysis
        let symbols: Vec<String> = sub.static_checks.iter().map(|c| c.symbol.clone()).collect();
        let static_found = symbols.iter().filter(|s| code_scan.found.contains(*s)).count();
        let static_pass_rate = if symbols.is_empty() {
            1.0
        } else {
            static_found as f64 / symbols.len() as f64
        };

        // Feature gate
        let gate_ok = if let Some(ref gate) = sub.required_feature {
            test_scan.all_tests.iter().any(|t| t.contains(gate))
                || code_scan.found.contains(gate)
        } else {
            true
        };

        // Tests
        let test_count = sub.test_anchors.len();
        let tests_passing = if test_count == 0 {
            0
        } else {
            sub.test_anchors.iter()
                .filter(|t| test_scan.matching.contains(t.as_str()))
                .count()
        };
        let test_pass_rate = if test_count == 0 { 1.0 } else { tests_passing as f64 / test_count as f64 };

        // Weighted score calculation
        let static_score = static_pass_rate * sub.weight as f64 * STATIC_WEIGHT;
        let test_score = test_pass_rate * sub.weight as f64 * TEST_WEIGHT;
        let gate_score = if gate_ok { sub.weight as f64 * GATE_WEIGHT } else { 0.0 };

        let sub_score = (static_score + test_score + gate_score).round() as u8;

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
