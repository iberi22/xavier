//! Context Regeneration — RRF weight tuner.
//!
//! Consumes `RetrievalMetrics` (from `eval.rs`) and the feedback signal from
//! `AdaptiveZoneBooster` to propose better RRF weights (k, keyword/vector balance,
//! layer weights). The tuner explores a small grid of candidates and picks the
//! configuration that maximizes a composite score (recall-weighted, with an MRR
//! tiebreaker), so retrieval is continuously optimized toward production query
//! patterns.
//!
//! This is the tuning half of context regeneration; it never applies changes
//! directly — it returns a `TuningProposal` that the caller (scheduler / CLI)
//! persists into `XavierSettings` and re-measures.

use super::config::{
    DEFAULT_EPISODIC_WEIGHT, DEFAULT_RRF_K, DEFAULT_SEMANTIC_WEIGHT, DEFAULT_WORKING_WEIGHT,
};
use super::eval::{EvalDataset, RetrievalMetrics};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A candidate configuration produced by the tuner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalConfig {
    pub rrf_k: u32,
    pub keyword_weight: f32,
    pub vector_weight: f32,
    pub working_weight: f32,
    pub episodic_weight: f32,
    pub semantic_weight: f32,
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            rrf_k: DEFAULT_RRF_K,
            keyword_weight: 0.5,
            vector_weight: 0.5,
            working_weight: DEFAULT_WORKING_WEIGHT,
            episodic_weight: DEFAULT_EPISODIC_WEIGHT,
            semantic_weight: DEFAULT_SEMANTIC_WEIGHT,
        }
    }
}

/// The outcome of a tuning pass: the best config found plus the measured gain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuningProposal {
    /// The recommended configuration to apply.
    pub config: RetrievalConfig,
    /// Composite score (recall + 0.3·MRR) of the recommended config.
    pub score: f64,
    /// Baseline score before tuning, for delta reporting.
    pub baseline_score: f64,
    /// Improvement in score: score - baseline_score.
    pub delta: f64,
    /// Number of candidates evaluated.
    pub candidates_evaluated: usize,
}

impl TuningProposal {
    /// True if the proposal improves on the baseline by a non-trivial margin.
    pub fn is_beneficial(&self) -> bool {
        self.delta > 0.005
    }
}

/// Recall threshold below which a drift alert is raised. A drop below this signals
/// that retrieval quality has regressed enough to warrant operator attention.
pub const RECALL_DRIFT_THRESHOLD: f64 = 0.65;

/// Detect recall drift between a baseline and a fresh measurement, raising a
/// system alert when recall has regressed past the threshold.
///
/// Returns `Some(regression_pct)` when a drift alert was raised (caller may log or
/// persist it), or `None` when recall is healthy or improved.
///
/// Drift is also flagged on an absolute drop even within threshold when the
/// regression exceeds 10 points (0.10) — a sudden step-down is always notable.
pub fn detect_recall_drift(baseline: &RetrievalMetrics, current: &RetrievalMetrics) -> Option<f64> {
    let prev = baseline.recall_at_k;
    let now = current.recall_at_k;

    // No baseline to compare against — nothing to do.
    if prev <= 0.0 {
        return None;
    }

    let regression = prev - now;

    // Trigger conditions: crossed the absolute floor, or a large relative drop.
    let crossed_floor = now < RECALL_DRIFT_THRESHOLD && prev >= RECALL_DRIFT_THRESHOLD;
    let large_drop = regression > 0.10;

    if !crossed_floor && !large_drop {
        return None;
    }

    let regression_pct = (regression / prev) * 100.0;
    let level = if now < 0.5 { "ERROR" } else { "WARN" };
    let message = format!(
        "Retrieval recall drifted: {:.0}% → {:.0}% ({:+.1} pts). Consider re-running the RRF tuner.",
        prev * 100.0,
        now * 100.0,
        -regression * 100.0
    );

    // Fire the alert through the shared system alert store. This is a best-effort
    // signal: the store is process-global, so it surfaces in /health and the
    // notification system when the server is running. In CLI-only contexts the
    // call is a harmless no-op on an unused store.
    crate::server::alerts::SYSTEM_ALERTS.push_alert(level, &message, "retrieval");
    // Always log so drift is visible even without the alert store wired.
    if level == "ERROR" {
        tracing::error!(component = "retrieval", "{message}");
    } else {
        tracing::warn!(component = "retrieval", "{message}");
    }

    Some(regression_pct)
}

/// A measured (config → metrics) pair, used internally during grid search.
#[derive(Debug, Clone)]
struct ScoredCandidate {
    config: RetrievalConfig,
    score: f64,
}

/// Composite score: recall is primary, MRR breaks near-ties.
///
/// recall@k ∈ [0,1], mrr ∈ [0,1] → score ∈ [0, 1.3].
fn composite_score(metrics: &RetrievalMetrics) -> f64 {
    metrics.recall_at_k + 0.3 * metrics.mrr
}

/// Run a grid search over RRF tuning knobs and return the best proposal.
///
/// `evaluate` is a closure that takes a candidate config, runs the retrieval
/// benchmark with those weights, and returns the resulting metrics. The search
/// space is deliberately small (≈ k × keyword balance × layer emphasis) to keep
/// each tuning pass cheap; deeper optimization is the auto-improvement loop's job.
pub fn tune<F>(baseline: &RetrievalConfig, evaluate: F) -> TuningProposal
where
    F: Fn(&RetrievalConfig) -> RetrievalMetrics,
{
    // Search grid. The values are chosen to bracket the defaults without
    // exploding the combinatorial space.
    const RRF_K_GRID: [u32; 3] = [40, 60, 80];
    const KV_BALANCE_GRID: [(f32, f32); 3] = [(0.4, 0.6), (0.5, 0.5), (0.6, 0.4)];
    // Layer emphasis: tilt toward semantic (memory) vs working (recent).
    const LAYER_GRID: [(f32, f32, f32); 3] = [
        (0.3, 0.3, 0.4), // default
        (0.2, 0.3, 0.5), // semantic-emphasis
        (0.4, 0.3, 0.3), // working-emphasis
    ];

    let baseline_metrics = evaluate(baseline);
    let baseline_score = composite_score(&baseline_metrics);

    let mut best = ScoredCandidate {
        config: baseline.clone(),
        score: baseline_score,
    };
    let mut count = 0usize;

    for &rrf_k in &RRF_K_GRID {
        for &(kw, vw) in &KV_BALANCE_GRID {
            for &(w, e, s) in &LAYER_GRID {
                let candidate = RetrievalConfig {
                    rrf_k,
                    keyword_weight: kw,
                    vector_weight: vw,
                    working_weight: w,
                    episodic_weight: e,
                    semantic_weight: s,
                };
                count += 1;
                let metrics = evaluate(&candidate);
                let score = composite_score(&metrics);
                if score > best.score {
                    best = ScoredCandidate {
                        config: candidate,
                        score,
                    };
                }
            }
        }
    }

    TuningProposal {
        config: best.config,
        score: best.score,
        baseline_score,
        delta: best.score - baseline_score,
        candidates_evaluated: count,
    }
}

/// Tune RRF weights independently per category in the dataset.
///
/// Cases are grouped by their `category` field. Groups with fewer than 3 cases
/// are skipped. For each qualifying group, `tune()` is run with an evaluate
/// closure scoped to that group. Returns a map from category name to the best
/// `TuningProposal` found for that group.
pub fn tune_by_category<F>(
    baseline: &RetrievalConfig,
    dataset: &EvalDataset,
    evaluate: F,
) -> HashMap<String, TuningProposal>
where
    F: Fn(&RetrievalConfig, &str, &[usize]) -> RetrievalMetrics,
{
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, case) in dataset.cases.iter().enumerate() {
        groups.entry(case.category.clone()).or_default().push(i);
    }

    let mut proposals = HashMap::new();
    for (category, indices) in groups {
        if indices.len() < 3 {
            continue;
        }
        let indices_slice: Vec<usize> = indices;
        let proposal = tune(baseline, |cfg| evaluate(cfg, &category, &indices_slice));
        proposals.insert(category, proposal);
    }
    proposals
}

/// Pick the proposal with the highest delta from a per-category map.
///
/// Returns a copy of the best proposal, or a fallback proposal with zero delta
/// if the map is empty.
pub fn best_overall(proposals: &HashMap<String, TuningProposal>) -> TuningProposal {
    proposals
        .values()
        .max_by(|a, b| {
            a.delta
                .partial_cmp(&b.delta)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .cloned()
        .unwrap_or(TuningProposal {
            config: RetrievalConfig::default(),
            score: 0.0,
            baseline_score: 0.0,
            delta: 0.0,
            candidates_evaluated: 0,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retrieval::eval::{CaseResult, RetrievalMetrics};

    fn metrics_for(recall: f64, mrr: f64) -> RetrievalMetrics {
        let num = 4usize;
        let hits = (recall * num as f64).round() as usize;
        let mut results = Vec::new();
        for i in 0..num {
            let hit = i < hits;
            results.push(CaseResult {
                case_id: format!("c{i}"),
                hit,
                first_hit_rank: if hit { Some(1) } else { None },
            });
        }
        let mut m = RetrievalMetrics::from_results("test", &results, 5);
        m.mrr = mrr;
        m
    }

    #[test]
    fn test_composite_score_weights_recall() {
        // Use num=10 so 0.8 recall is exact (8 hits / 10 cases).
        let num = 10usize;
        let mut results = Vec::new();
        for i in 0..num {
            let hit = i < 8;
            results.push(CaseResult {
                case_id: format!("c{i}"),
                hit,
                first_hit_rank: if hit { Some(1) } else { None },
            });
        }
        let mut m = RetrievalMetrics::from_results("test", &results, 5);
        m.mrr = 0.5;
        let s = composite_score(&m);
        assert!((s - (0.8 + 0.3 * 0.5)).abs() < 1e-9);
    }

    #[test]
    fn test_tune_picks_best_candidate() {
        // Simulate an evaluator where higher rrf_k yields better recall.
        let baseline = RetrievalConfig::default();
        let proposal = tune(&baseline, |cfg| {
            let recall = if cfg.rrf_k >= 80 {
                0.9
            } else if cfg.rrf_k >= 60 {
                0.7
            } else {
                0.5
            };
            metrics_for(recall, 0.6)
        });
        // The best config must use rrf_k=80.
        assert_eq!(proposal.config.rrf_k, 80);
        assert!(proposal.is_beneficial());
        assert_eq!(proposal.candidates_evaluated, 27); // 3 × 3 × 3
    }

    #[test]
    fn test_tune_returns_baseline_when_no_improvement() {
        let baseline = RetrievalConfig::default();
        // Evaluator always returns identical metrics regardless of config.
        let proposal = tune(&baseline, |_| metrics_for(0.5, 0.5));
        // No candidate beats the (identical) baseline score.
        assert!(!proposal.is_beneficial());
        assert!(proposal.delta.abs() < 1e-9);
    }

    #[test]
    fn test_default_config_matches_retrieval_defaults() {
        let c = RetrievalConfig::default();
        assert_eq!(c.rrf_k, DEFAULT_RRF_K);
        assert_eq!(c.working_weight, DEFAULT_WORKING_WEIGHT);
    }

    #[test]
    fn test_detect_recall_drift_floor_crossing() {
        // Recall drops from above to below the threshold -> drift flagged.
        let baseline = RetrievalMetrics {
            dataset: "t".into(),
            num_cases: 10,
            recall_at_k: 0.70,
            mrr: 0.5,
            hit_rate: 0.70,
            k: 5,
            precision_at_k: 0.14,
            sigma: 0.0,
            category: None,
        };
        let current = RetrievalMetrics {
            recall_at_k: 0.60,
            ..baseline.clone()
        };
        let drift = detect_recall_drift(&baseline, &current);
        assert!(drift.is_some(), "floor crossing must flag drift");
    }

    #[test]
    fn test_detect_recall_drift_large_drop() {
        // A >10pt drop flags even when staying above the floor.
        let baseline = RetrievalMetrics {
            dataset: "t".into(),
            num_cases: 10,
            recall_at_k: 0.90,
            mrr: 0.5,
            hit_rate: 0.90,
            k: 5,
            precision_at_k: 0.18,
            sigma: 0.0,
            category: None,
        };
        let current = RetrievalMetrics {
            recall_at_k: 0.78,
            ..baseline.clone()
        };
        let drift = detect_recall_drift(&baseline, &current);
        assert!(drift.is_some(), "large drop must flag drift");
    }

    #[test]
    fn test_detect_recall_drift_healthy_no_alert() {
        // Improvement or stable recall -> no drift.
        let baseline = RetrievalMetrics {
            dataset: "t".into(),
            num_cases: 10,
            recall_at_k: 0.80,
            mrr: 0.5,
            hit_rate: 0.80,
            k: 5,
            precision_at_k: 0.16,
            sigma: 0.0,
            category: None,
        };
        let improved = RetrievalMetrics {
            recall_at_k: 0.85,
            ..baseline.clone()
        };
        assert!(detect_recall_drift(&baseline, &improved).is_none());

        // Small fluctuation within tolerance -> no drift.
        let small_drop = RetrievalMetrics {
            recall_at_k: 0.77,
            ..baseline.clone()
        };
        assert!(detect_recall_drift(&baseline, &small_drop).is_none());
    }

    #[test]
    fn test_detect_recall_drift_no_baseline() {
        // Zero baseline -> nothing to compare, returns None.
        let baseline = RetrievalMetrics {
            dataset: "t".into(),
            num_cases: 0,
            recall_at_k: 0.0,
            mrr: 0.0,
            hit_rate: 0.0,
            k: 5,
            precision_at_k: 0.0,
            sigma: 0.0,
            category: None,
        };
        let current = RetrievalMetrics {
            recall_at_k: 0.5,
            ..baseline.clone()
        };
        assert!(detect_recall_drift(&baseline, &current).is_none());
    }

    #[test]
    fn test_tune_by_category_single_category_falls_through() {
        // All cases in "general" -> tune() is called once for that group.
        let ds = EvalDataset::from_pairs("test", &[("q1", "e1"), ("q2", "e2"), ("q3", "e3")]);
        let baseline = RetrievalConfig::default();
        let proposals =
            tune_by_category(&baseline, &ds, |_cfg, _cat, _indices| metrics_for(0.7, 0.5));
        assert_eq!(proposals.len(), 1);
        assert!(proposals.contains_key("general"));
    }

    #[test]
    fn test_tune_by_category_multi_category() {
        // Cases split across categories with >= 3 each.
        let mut ds = EvalDataset::from_pairs("test", &[]);
        for i in 0..3 {
            ds.cases.push(crate::retrieval::eval::EvalCase {
                id: format!("a-{i}"),
                query: format!("q{i}"),
                expected_path: format!("e{i}"),
                category: "alpha".to_string(),
            });
        }
        for i in 0..4 {
            ds.cases.push(crate::retrieval::eval::EvalCase {
                id: format!("b-{i}"),
                query: format!("q{i}"),
                expected_path: format!("e{i}"),
                category: "beta".to_string(),
            });
        }
        let baseline = RetrievalConfig::default();
        let proposals =
            tune_by_category(&baseline, &ds, |_cfg, _cat, _indices| metrics_for(0.8, 0.6));
        assert_eq!(proposals.len(), 2);
        assert!(proposals.contains_key("alpha"));
        assert!(proposals.contains_key("beta"));
    }

    #[test]
    fn test_tune_by_category_empty_dataset() {
        let ds = EvalDataset::from_pairs("empty", &[]);
        let baseline = RetrievalConfig::default();
        let proposals =
            tune_by_category(&baseline, &ds, |_cfg, _cat, _indices| metrics_for(0.5, 0.5));
        assert!(proposals.is_empty());
    }

    #[test]
    fn test_best_overall_picks_highest_delta() {
        let mut proposals = HashMap::new();
        proposals.insert(
            "low".to_string(),
            TuningProposal {
                config: RetrievalConfig::default(),
                score: 0.6,
                baseline_score: 0.55,
                delta: 0.05,
                candidates_evaluated: 27,
            },
        );
        proposals.insert(
            "high".to_string(),
            TuningProposal {
                config: RetrievalConfig::default(),
                score: 0.9,
                baseline_score: 0.7,
                delta: 0.2,
                candidates_evaluated: 27,
            },
        );
        let best = best_overall(&proposals);
        assert!((best.delta - 0.2).abs() < 1e-9);
    }
}
