//! # Scorer — Maturity Scoring Engine
//!
//! Calculates maturity percentage using a 5-factor weighted formula.

use serde::{Deserialize, Serialize};

/// Scores for each of the 5 maturity factors (0.0 to 1.0).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FeatureScores {
    pub static_code_pct: f64,
    pub tests_pct: f64,
    pub gates_pct: f64,
    pub memory_pct: f64,
    pub issues_pct: f64,
}

/// Weights for the scoring formula.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringWeights {
    pub static_code: f64,
    pub tests: f64,
    pub gates: f64,
    pub memory: f64,
    pub issues: f64,
}

impl Default for ScoringWeights {
    fn default() -> Self {
        Self {
            static_code: 0.35,
            tests: 0.35,
            gates: 0.10,
            memory: 0.10,
            issues: 0.10,
        }
    }
}

/// Computes the weighted maturity score for a feature.
///
/// Formula:
/// static * 0.35 + tests * 0.35 + gates * 0.10 + memory * 0.10 + issues * 0.10
/// Each score is expected to be already multiplied by the component's weight (e.g. 1.0 * weight).
pub fn compute_feature_score(scores: &FeatureScores, weights: &ScoringWeights) -> f64 {
    let score = (scores.static_code_pct * weights.static_code)
        + (scores.tests_pct * weights.tests)
        + (scores.gates_pct * weights.gates)
        + (scores.memory_pct * weights.memory)
        + (scores.issues_pct * weights.issues);

    score.round()
}

/// Computes the overall maturity as the weighted average of feature scores.
pub fn compute_overall(feature_scores: &[f64], weights: &[u32]) -> f64 {
    if feature_scores.is_empty() || feature_scores.len() != weights.len() {
        return 0.0;
    }

    let total_weight: u32 = weights.iter().sum();
    if total_weight == 0 {
        return 0.0;
    }

    let weighted_sum: f64 = feature_scores.iter()
        .zip(weights.iter())
        .map(|(score, weight)| score * *weight as f64)
        .sum();

    (weighted_sum / total_weight as f64).round()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_feature_score_full() {
        // Assuming weight is 100 for this subcomponent
        let scores = FeatureScores {
            static_code_pct: 100.0,
            tests_pct: 100.0,
            gates_pct: 100.0,
            memory_pct: 100.0,
            issues_pct: 100.0,
        };
        let weights = ScoringWeights::default();
        assert_eq!(compute_feature_score(&scores, &weights), 100.0);
    }

    #[test]
    fn test_compute_feature_score_zero() {
        let scores = FeatureScores::default();
        let weights = ScoringWeights::default();
        assert_eq!(compute_feature_score(&scores, &weights), 0.0);
    }

    #[test]
    fn test_compute_feature_score_mixed() {
        // weight = 100
        let scores = FeatureScores {
            static_code_pct: 100.0, // 35.0
            tests_pct: 50.0,       // 17.5
            gates_pct: 100.0,       // 10.0
            memory_pct: 0.0,      // 0.0
            issues_pct: 0.0,      // 0.0
        };
        // 35.0 + 17.5 + 10.0 = 62.5 -> rounded to 63.0
        let weights = ScoringWeights::default();
        assert_eq!(compute_feature_score(&scores, &weights), 63.0);
    }

    #[test]
    fn test_compute_overall_weighted() {
        let feature_scores = vec![100.0, 50.0];
        let weights = vec![10, 10]; // Equal weights -> 75.0
        assert_eq!(compute_overall(&feature_scores, &weights), 75.0);

        let weights2 = vec![30, 10]; // (3000 + 500) / 40 = 3500 / 40 = 87.5 -> 88.0
        assert_eq!(compute_overall(&feature_scores, &weights2), 88.0);

        let empty_scores: Vec<f64> = Vec::new();
        let empty_weights: Vec<u32> = Vec::new();
        assert_eq!(compute_overall(&empty_scores, &empty_weights), 0.0);
    }
}
