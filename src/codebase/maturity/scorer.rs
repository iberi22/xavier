//! Deterministic Scoring Engine (v2 - 5 metrics)
//!
//! Produces a deterministic maturity percentage for each feature
//! based on 5 weighted factors.

use serde::{Deserialize, Serialize};

/// Result of scoring one subcomponent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredSubcomponent {
    pub name: String,
    pub weight: f64,
    pub sub_score: f64,
}

/// Result of scoring a feature
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureScore {
    pub overall: f64,
    pub code_coverage: f64,
    pub memory_coverage: f64,
    pub conversation_coverage: f64,
    pub test_coverage: f64,
    pub doc_coverage: f64,
    pub subcomponents: Vec<ScoredSubcomponent>,
}

const STATIC_WEIGHT: f64 = 0.35;
const TEST_WEIGHT: f64 = 0.35;
const GATE_WEIGHT: f64 = 0.10;
const MEMORY_WEIGHT: f64 = 0.10;
const ISSUE_WEIGHT: f64 = 0.10;

/// Score a feature based on evidence
pub fn score_feature(
    _feature_id: &str,
    evidence: &[String],
    memory_ratio: f64,
    conv_ratio: f64,
) -> FeatureScore {
    let has_symbols = if evidence.is_empty() { 0.0 } else { 1.0 };
    let has_tests = if evidence.iter().any(|e| e.contains("test")) { 1.0 } else { 0.0 };
    let has_gate = 1.0; // assume gate exists
    let has_code = if evidence.len() > 2 { 1.0 } else { 0.5 };

    let subcomponents = vec![
        ScoredSubcomponent {
            name: "static_code".into(),
            weight: STATIC_WEIGHT,
            sub_score: has_symbols * STATIC_WEIGHT,
        },
        ScoredSubcomponent {
            name: "tests".into(),
            weight: TEST_WEIGHT,
            sub_score: has_tests * TEST_WEIGHT,
        },
        ScoredSubcomponent {
            name: "feature_gate".into(),
            weight: GATE_WEIGHT,
            sub_score: has_gate * GATE_WEIGHT,
        },
        ScoredSubcomponent {
            name: "memory".into(),
            weight: MEMORY_WEIGHT,
            sub_score: memory_ratio * MEMORY_WEIGHT,
        },
        ScoredSubcomponent {
            name: "issues_discussions".into(),
            weight: ISSUE_WEIGHT,
            sub_score: conv_ratio * ISSUE_WEIGHT,
        },
    ];

    let total_weight: f64 = subcomponents.iter().map(|s| s.weight).sum();
    let total_score: f64 = subcomponents.iter().map(|s| s.sub_score).sum();
    let overall = if total_weight > 0.0 {
        (total_score / total_weight) * 100.0
    } else {
        0.0
    };

    FeatureScore {
        overall,
        code_coverage: has_code,
        memory_coverage: memory_ratio,
        conversation_coverage: conv_ratio,
        test_coverage: has_tests,
        doc_coverage: 0.0,
        subcomponents,
    }
}
