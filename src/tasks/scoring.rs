// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Deterministic Xavier Rating Engine
//!
//! Implements a scoring engine for data quality, task difficulty, context usefulness,
//! and reward eligibility. All scoring functions are deterministic: same inputs → same outputs.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐     ┌──────────────┐     ┌──────────────┐
//! │ Raw Inputs  │ ──→ │ Normalizer   │ ──→ │ Score Fn     │
//! └─────────────┘     └──────────────┘     └──────────────┘
//!                                                │
//!                                                ↓
//! ┌─────────────┐     ┌──────────────┐     ┌──────────────┐
//! │ Signed Audit│ ←── │ Evidence     │ ←── │ Score Record │
//! │ Record      │     │ Formatter    │     │              │
//! └─────────────┘     └──────────────┘     └──────────────┘
//! ```
//!
//! # Determinism Guarantee
//! - All score functions are `pure` — no I/O, no randomness
//! - Parameters are versioned and stored alongside scores
//! - Regression fixtures test that identical inputs produce identical outputs

use serde::{Deserialize, Serialize};

/// Current scoring algorithm version
pub const SCORING_VERSION: &str = "1.0.0";

// ═══════════════════════════════════════════════
// Input Types
// ═══════════════════════════════════════════════

/// Input data for scoring a contribution/data quality
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContributionInput {
    /// Size of the contributed data in bytes
    pub byte_size: u64,
    /// Number of unique tokens/terms after dedup
    pub unique_terms: usize,
    /// Whether the contribution has associated test coverage
    pub has_tests: bool,
    /// Whether the contribution includes documentation
    pub has_docs: bool,
    /// Boolean features about the contribution
    pub features: ContributionFeatures,
}

/// Boolean feature flags for a contribution
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContributionFeatures {
    pub has_code: bool,
    pub has_markdown: bool,
    pub has_json: bool,
    pub has_config: bool,
}

/// Input data for scoring task difficulty
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDifficultyInput {
    /// Estimated lines of code needed
    pub estimated_loc: u64,
    /// Number of modules/files touched
    pub modules_touched: u32,
    /// Number of dependencies/modules affected
    pub dependencies_count: u32,
    /// Whether crypto/security logic is involved
    pub involves_crypto: bool,
    /// Whether the task requires new external dependencies
    pub requires_new_deps: bool,
    /// Whether the task touches mesh/network code
    pub involves_mesh: bool,
    /// Subjective complexity from issue labels (1-5)
    pub label_complexity: u8,
}

/// Input for scoring context usefulness (recall@k quality)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextUsefulnessInput {
    /// recall@k value (0.0 to 1.0)
    pub recall_at_k: f64,
    /// Mean Reciprocal Rank (0.0 to 1.0)
    pub mrr: f64,
    /// Number of queries tested
    pub query_count: u64,
    /// Average latency in ms
    pub avg_latency_ms: f64,
    /// Whether results crossed relevance threshold
    pub relevance_crossed: bool,
}

// ═══════════════════════════════════════════════
// Output Types
// ═══════════════════════════════════════════════

/// A scored record with full provenance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreRecord {
    /// Schema version for forward compatibility
    pub version: String,
    /// Timestamp (seconds since epoch, set externally for determinism)
    pub timestamp_secs: u64,
    /// Raw score (0.0 to 1.0)
    pub score: f64,
    /// Human-readable explanation
    pub explanation: String,
    /// All input evidence used for scoring
    pub evidence: EvidenceBundle,
}

/// Bundled evidence that produced a score
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvidenceBundle {
    pub contribution: Option<ContributionInput>,
    pub task_difficulty: Option<TaskDifficultyInput>,
    pub context_usefulness: Option<ContextUsefulnessInput>,
    pub additional_notes: Vec<String>,
}

// ═══════════════════════════════════════════════
// Score Parameters (versioned)
// ═══════════════════════════════════════════════

/// Versioned scoring parameters — changing these changes scores
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringParams {
    pub version: String,
    /// Weight for byte size normalization
    pub byte_weight: f64,
    /// Weight for unique terms
    pub terms_weight: f64,
    /// Bonus for having tests
    pub test_bonus: f64,
    /// Bonus for having docs
    pub doc_bonus: f64,
    /// Contribution score cap
    pub contribution_max: f64,
    /// Difficulty: base multiplier per LOC
    pub loc_multiplier: f64,
    /// Difficulty: bonus per module touched
    pub module_bonus: f64,
    /// Difficulty: crypto penalty (adds complexity)
    pub crypto_penalty: f64,
    /// Context: recall weight
    pub recall_weight: f64,
    /// Context: MRR weight
    pub mrr_weight: f64,
    /// Context: latency penalty per ms over threshold
    pub latency_penalty_per_ms: f64,
    /// Context: latency threshold in ms before penalty
    pub latency_threshold_ms: f64,
}

impl Default for ScoringParams {
    fn default() -> Self {
        Self {
            version: SCORING_VERSION.to_string(),
            byte_weight: 0.3,
            terms_weight: 0.2,
            test_bonus: 0.15,
            doc_bonus: 0.1,
            contribution_max: 1.0,
            loc_multiplier: 0.01,
            module_bonus: 0.05,
            crypto_penalty: 0.2,
            recall_weight: 0.5,
            mrr_weight: 0.3,
            latency_penalty_per_ms: 0.001,
            latency_threshold_ms: 100.0,
        }
    }
}

// ═══════════════════════════════════════════════
// Score Functions (PURE — deterministic)
// ═══════════════════════════════════════════════

/// Score a data contribution.
/// Pure function: no I/O, no randomness, no side effects.
pub fn score_contribution(input: &ContributionInput, params: &ScoringParams) -> f64 {
    let mut score = 0.0;

    // Byte size contribution (normalized to 100KB = 0.5)
    let byte_norm = (input.byte_size as f64 / 102_400.0).min(1.0);
    score += byte_norm * params.byte_weight;

    // Unique terms contribution (capped at 500)
    let terms_norm = (input.unique_terms as f64 / 500.0).min(1.0);
    score += terms_norm * params.terms_weight;

    // Boolean bonuses
    if input.has_tests {
        score += params.test_bonus;
    }
    if input.has_docs {
        score += params.doc_bonus;
    }

    // Feature diversity (having more feature types is better)
    let feature_count = input.features.has_code as u8 as f64
        + input.features.has_markdown as u8 as f64
        + input.features.has_json as u8 as f64
        + input.features.has_config as u8 as f64;
    let diversity_bonus = (feature_count / 4.0) * 0.1;
    score += diversity_bonus;

    score.min(params.contribution_max)
}

/// Score task difficulty.
/// Pure function: no I/O, no randomness, no side effects.
pub fn score_task_difficulty(input: &TaskDifficultyInput, params: &ScoringParams) -> f64 {
    let mut score = 0.0;

    // LOC contribution
    score += (input.estimated_loc as f64) * params.loc_multiplier;

    // Module touch bonus
    score += (input.modules_touched as f64) * params.module_bonus;

    // Dependency modifier
    let dep_modifier = 1.0 + (input.dependencies_count as f64 * 0.02);
    score *= dep_modifier;

    // Complexity multipliers
    if input.involves_crypto {
        score += params.crypto_penalty;
    }
    if input.requires_new_deps {
        score += 0.1;
    }
    if input.involves_mesh {
        score += 0.15;
    }

    // Label complexity (1-5 map to 0.0-0.5)
    let label_score = ((input.label_complexity as f64 - 1.0) / 4.0) * 0.5;
    score += label_score;

    // Normalize to 0-1 range
    (score / 5.0).min(1.0)
}

/// Score context usefulness based on retrieval metrics.
/// Pure function: no I/O, no randomness, no side effects.
pub fn score_context_usefulness(input: &ContextUsefulnessInput, params: &ScoringParams) -> f64 {
    let mut score = 0.0;

    // recall@k component
    score += input.recall_at_k * params.recall_weight;

    // MRR component
    score += input.mrr * params.mrr_weight;

    // Latency penalty
    if input.avg_latency_ms > params.latency_threshold_ms {
        let excess = input.avg_latency_ms - params.latency_threshold_ms;
        let penalty = excess * params.latency_penalty_per_ms;
        score = (score - penalty).max(0.0);
    }

    // Relevance bonus
    if input.relevance_crossed {
        score += 0.1;
    }

    score.min(1.0)
}

/// Compute a composite score from multiple evidence sources.
/// Pure function.
pub fn score_composite(evidence: &EvidenceBundle, params: &ScoringParams) -> (f64, Vec<String>) {
    let mut scores = Vec::new();
    let mut explanations = Vec::new();

    if let Some(ref c) = evidence.contribution {
        let s = score_contribution(c, params);
        scores.push(s);
        explanations.push(format!("contribution: {:.3}", s));
    }

    if let Some(ref t) = evidence.task_difficulty {
        let s = score_task_difficulty(t, params);
        scores.push(s);
        explanations.push(format!("task_difficulty: {:.3}", s));
    }

    if let Some(ref u) = evidence.context_usefulness {
        let s = score_context_usefulness(u, params);
        scores.push(s);
        explanations.push(format!("context_usefulness: {:.3}", s));
    }

    // Combine by averaging non-zero scores
    // (more evidence = more reliable score, but mean prevents gaming)
    let count = scores.len().max(1);
    let total: f64 = scores.iter().sum();
    let composite = (total / count as f64).min(1.0);

    (composite, explanations)
}

// ═══════════════════════════════════════════════
// Adversarial checks (spam, low-value, collusion)
// ═══════════════════════════════════════════════

/// Reasons a submission might be penalized or rejected
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PenaltyReason {
    Spam,
    LowValue,
    DuplicateContent,
    SuspiciousTiming,
    MissingEvidence,
}

/// Apply penalty modifiers to a raw score.
/// Pure function.
pub fn apply_penalties(raw_score: f64, reasons: &[PenaltyReason], params: &ScoringParams) -> f64 {
    let _ = params; // Reserved for future parameterized penalties
    let penalty: f64 = reasons.iter().fold(0.0, |acc, r| {
        acc + match r {
            PenaltyReason::Spam => 0.5,
            PenaltyReason::LowValue => 0.3,
            PenaltyReason::DuplicateContent => 0.4,
            PenaltyReason::SuspiciousTiming => 0.2,
            PenaltyReason::MissingEvidence => 0.6,
        }
    });
    (raw_score - penalty).max(0.0)
}

/// Check if a submission looks like spam based on repeated patterns.
/// Pure function on the input data.
pub fn detect_spam_patterns(
    _input: &ContributionInput,
    recent_inputs: &[ContributionInput],
) -> Vec<PenaltyReason> {
    let mut reasons = Vec::new();

    // If we have exactly same byte_size and feature flags many times
    if recent_inputs.len() >= 3 {
        let identical_count = recent_inputs
            .iter()
            .filter(|r| r.byte_size == _input.byte_size && r.unique_terms < 5)
            .count();

        if identical_count >= 3 {
            reasons.push(PenaltyReason::Spam);
        }

        // Check for unusually small contributions
        if _input.byte_size < 50 && _input.unique_terms < 3 {
            reasons.push(PenaltyReason::LowValue);
        }
    }

    reasons
}

impl Default for TaskDifficultyInput {
    fn default() -> Self {
        Self {
            estimated_loc: 0,
            modules_touched: 0,
            dependencies_count: 0,
            involves_crypto: false,
            requires_new_deps: false,
            involves_mesh: false,
            label_complexity: 1,
        }
    }
}

impl Default for ContextUsefulnessInput {
    fn default() -> Self {
        Self {
            recall_at_k: 0.0,
            mrr: 0.0,
            query_count: 0,
            avg_latency_ms: 0.0,
            relevance_crossed: false,
        }
    }
}



// ═══════════════════════════════════════════════
// Tests — Regression fixtures
// ═══════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanity: default params are valid
    #[test]
    fn test_default_params_are_valid() {
        let p = ScoringParams::default();
        assert_eq!(p.version, SCORING_VERSION);
    }

    /// Sanity: empty contribution scores 0
    #[test]
    fn test_empty_contribution_scores_zero() {
        let input = ContributionInput {
            byte_size: 0,
            unique_terms: 0,
            has_tests: false,
            has_docs: false,
            features: ContributionFeatures {
                has_code: false,
                has_markdown: false,
                has_json: false,
                has_config: false,
            },
        };
        let p = ScoringParams::default();
        let score = score_contribution(&input, &p);
        assert!((score - 0.0).abs() < f64::EPSILON);
    }

    /// REGRESSION FIXTURE: high-quality contribution must score > 0.5
    #[test]
    fn test_high_quality_contribution() {
        let input = ContributionInput {
            byte_size: 102_400, // 100KB
            unique_terms: 400,
            has_tests: true,
            has_docs: true,
            features: ContributionFeatures {
                has_code: true,
                has_markdown: true,
                has_json: true,
                has_config: true,
            },
        };
        let p = ScoringParams::default();
        let score = score_contribution(&input, &p);
        assert!(
            score > 0.5,
            "high quality should score > 0.5, got {:.3}",
            score
        );
        assert!(score <= 1.0, "score should be capped at 1.0");
    }

    /// REGRESSION FIXTURE: task difficulty must be deterministic
    #[test]
    fn test_task_difficulty_reproducible() {
        let input = TaskDifficultyInput {
            estimated_loc: 500,
            modules_touched: 5,
            dependencies_count: 3,
            involves_crypto: true,
            requires_new_deps: false,
            involves_mesh: true,
            label_complexity: 4,
        };
        let p = ScoringParams::default();

        // Run 3 times — must produce same result
        let r1 = score_task_difficulty(&input, &p);
        let r2 = score_task_difficulty(&input, &p);
        let r3 = score_task_difficulty(&input, &p);

        assert!((r1 - r2).abs() < f64::EPSILON, "not deterministic");
        assert!((r2 - r3).abs() < f64::EPSILON, "not deterministic");
    }

    /// REGRESSION FIXTURE: context usefulness with perfect metrics
    #[test]
    fn test_perfect_context_scores_high() {
        let input = ContextUsefulnessInput {
            recall_at_k: 1.0,
            mrr: 1.0,
            query_count: 100,
            avg_latency_ms: 50.0, // well under threshold
            relevance_crossed: true,
        };
        let p = ScoringParams::default();
        let score = score_context_usefulness(&input, &p);
        assert!(
            score > 0.8,
            "perfect context should score > 0.8, got {:.3}",
            score
        );
    }

    /// REGRESSION FIXTURE: composite score
    #[test]
    fn test_composite_score() {
        let evidence = EvidenceBundle {
            contribution: Some(ContributionInput {
                byte_size: 50_000,
                unique_terms: 100,
                has_tests: true,
                has_docs: false,
                features: ContributionFeatures {
                    has_code: true,
                    has_markdown: true,
                    has_json: false,
                    has_config: false,
                },
            }),
            task_difficulty: Some(TaskDifficultyInput {
                estimated_loc: 200,
                modules_touched: 3,
                dependencies_count: 2,
                involves_crypto: false,
                requires_new_deps: false,
                involves_mesh: false,
                label_complexity: 3,
            }),
            context_usefulness: None,
            additional_notes: vec![],
        };
        let p = ScoringParams::default();
        let (score, explanations) = score_composite(&evidence, &p);
        assert!(score > 0.0);
        assert_eq!(explanations.len(), 2);
    }

    /// REGRESSION FIXTURE: spam penalty reduces score
    #[test]
    fn test_spam_penalty() {
        let score = 0.8;
        let penalties = vec![PenaltyReason::Spam];
        let p = ScoringParams::default();
        let final_score = apply_penalties(score, &penalties, &p);
        assert!(
            (final_score - 0.3).abs() < f64::EPSILON,
            "expected 0.3, got {:.3}",
            final_score
        );
    }

    /// REGRESSION FIXTURE: multiple penalties don't go below 0
    #[test]
    fn test_penalties_capped_at_zero() {
        let score = 0.2;
        let penalties = vec![PenaltyReason::Spam, PenaltyReason::MissingEvidence];
        let p = ScoringParams::default();
        let final_score = apply_penalties(score, &penalties, &p);
        assert_eq!(final_score, 0.0);
    }

    /// REGRESSION FIXTURE: spam pattern detection
    #[test]
    fn test_detect_spam_patterns() {
        let input = ContributionInput {
            byte_size: 30,
            unique_terms: 2,
            has_tests: false,
            has_docs: false,
            features: ContributionFeatures {
                has_code: true,
                has_markdown: false,
                has_json: false,
                has_config: false,
            },
        };

        let recent = vec![
            ContributionInput {
                byte_size: 30,
                unique_terms: 2,
                has_tests: false,
                has_docs: false,
                features: ContributionFeatures {
                    has_code: true,
                    has_markdown: false,
                    has_json: false,
                    has_config: false,
                },
            },
            ContributionInput::default(),
            ContributionInput::default(),
        ];

        let reasons = detect_spam_patterns(&input, &recent);
        assert!(!reasons.is_empty());
    }

    /// REGRESSION FIXTURE: low-value content detected
    #[test]
    fn test_low_value_detection() {
        let input = ContributionInput {
            byte_size: 10,
            unique_terms: 1,
            has_tests: false,
            has_docs: false,
            features: ContributionFeatures {
                has_code: false,
                has_markdown: false,
                has_json: false,
                has_config: false,
            },
        };

        let reasons = detect_spam_patterns(&input, &[input.clone(), input.clone(), input.clone()]);
        // Should find both LowValue and possibly Spam
        assert!(!reasons.is_empty());
    }
}
