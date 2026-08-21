//! Context Regeneration Loop and Dynamic RRF Weight Tuning
//!
//! Provides non-blocking background context consolidation and Reciprocal Rank
//! Fusion (RRF) dynamic weight tuning based on live search hit metrics.
//!
//! Heavy vector-keyword similarity recalculations are offloaded to
//! `tokio::task::spawn_blocking` to avoid blocking the async runtime.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use super::config::{DEFAULT_KEYWORD_WEIGHT, DEFAULT_VECTOR_WEIGHT};
use crate::search::rrf::ScoredResult;

/// Configuration for the background context regenerator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextRegeneratorConfig {
    /// Interval in seconds between background regeneration passes.
    pub interval_secs: u64,
    /// Learning rate for dynamic weight adjustments (0.0 to 1.0).
    pub learning_rate: f32,
    /// Number of top-k search results to re-rank during context consolidation.
    pub target_top_k: usize,
    /// Delta threshold below which weight tuning is considered converged.
    pub convergence_threshold: f32,
    /// Minimum total query sample size required before adjusting weights.
    pub min_hit_sample: u32,
}

impl Default for ContextRegeneratorConfig {
    fn default() -> Self {
        Self {
            interval_secs: 60,
            learning_rate: 0.1,
            target_top_k: 10,
            convergence_threshold: 0.005,
            min_hit_sample: 5,
        }
    }
}

/// Result metrics produced by a single regeneration pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegenerationResult {
    /// Baseline composite score before adjustment.
    pub baseline_score: f64,
    /// Re-calculated score after dynamic weight tuning.
    pub regenerated_score: f64,
    /// Newly assigned keyword/BM25 weight.
    pub keyword_weight: f32,
    /// Newly assigned vector/dense weight.
    pub vector_weight: f32,
    /// Mean absolute score shift across top-k items.
    pub top_k_score_shift: f64,
    /// Whether weight changes fell below the convergence threshold.
    pub converged: bool,
    /// Total similarity pairs/candidates processed.
    pub candidates_processed: usize,
    /// Processing duration in milliseconds.
    pub duration_ms: u64,
}

/// Context Regenerator - cognitive optimizer for scheduled memory consolidation.
#[derive(Debug)]
pub struct ContextRegenerator {
    config: ContextRegeneratorConfig,
    keyword_hits: AtomicU32,
    vector_hits: AtomicU32,
    total_queries: AtomicU32,
    weights: RwLock<(f32, f32)>,
}

impl ContextRegenerator {
    /// Create a new `ContextRegenerator` with the given configuration.
    pub fn new(config: ContextRegeneratorConfig) -> Self {
        Self {
            config,
            keyword_hits: AtomicU32::new(0),
            vector_hits: AtomicU32::new(0),
            total_queries: AtomicU32::new(0),
            weights: RwLock::new((DEFAULT_KEYWORD_WEIGHT, DEFAULT_VECTOR_WEIGHT)),
        }
    }

    /// Create a `ContextRegenerator` with default settings.
    pub fn with_defaults() -> Self {
        Self::new(ContextRegeneratorConfig::default())
    }

    /// Record feedback for a query hit from keyword search and/or vector search.
    pub fn record_hit(&self, keyword_hit: bool, vector_hit: bool) {
        self.total_queries.fetch_add(1, Ordering::Relaxed);
        if keyword_hit {
            self.keyword_hits.fetch_add(1, Ordering::Relaxed);
        }
        if vector_hit {
            self.vector_hits.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Get current (keyword_weight, vector_weight) pair.
    pub async fn current_weights(&self) -> (f32, f32) {
        *self.weights.read().await
    }

    /// Calculate updated RRF weights based on hit metrics and learning rate.
    ///
    /// Weights are normalized such that `keyword_weight + vector_weight == 1.0`.
    pub fn calculate_rrf_weights(
        &self,
        keyword_hits: u32,
        vector_hits: u32,
        total_queries: u32,
        current_kw: f32,
        current_vw: f32,
    ) -> (f32, f32) {
        if total_queries < self.config.min_hit_sample {
            return (current_kw, current_vw);
        }

        let total_hits = (keyword_hits + vector_hits) as f32;
        if total_hits <= 0.0 {
            return (current_kw, current_vw);
        }

        let kw_ratio = keyword_hits as f32 / total_hits;
        let vw_ratio = vector_hits as f32 / total_hits;

        let alpha = self.config.learning_rate.clamp(0.01, 1.0);
        let new_kw = current_kw * (1.0 - alpha) + kw_ratio * alpha;
        let new_vw = current_vw * (1.0 - alpha) + vw_ratio * alpha;

        let sum = new_kw + new_vw;
        if sum > 0.0 {
            (new_kw / sum, new_vw / sum)
        } else {
            (0.5, 0.5)
        }
    }

    /// Adjust scores of top-k results according to the newly tuned RRF weights.
    ///
    /// Returns the average absolute score shift across top-k results.
    pub fn adjust_top_k_scores(
        &self,
        results: &mut [ScoredResult],
        keyword_weight: f32,
        vector_weight: f32,
    ) -> f64 {
        if results.is_empty() {
            return 0.0;
        }

        let target_len = results.len().min(self.config.target_top_k);
        let mut total_shift = 0.0f64;

        for item in results.iter_mut().take(target_len) {
            let old_score = item.score;
            // Scale score according to source channel or balanced weight multiplier
            let multiplier = if item.source.contains("working") || item.source.contains("keyword") {
                keyword_weight * 2.0
            } else if item.source.contains("semantic") || item.source.contains("vector") {
                vector_weight * 2.0
            } else {
                keyword_weight + vector_weight
            };

            let new_score = (old_score * multiplier).clamp(0.0, 1.0);
            item.score = new_score;
            total_shift += (new_score - old_score).abs() as f64;
        }

        total_shift / (target_len as f64)
    }

    /// Non-blocking batch cosine similarity recalculation.
    ///
    /// Offloads heavy floating point vector/keyword similarity recalculations to
    /// `tokio::task::spawn_blocking` to avoid stalling the async reactor.
    pub async fn recalculate_similarity_batch(
        &self,
        candidates: Vec<(Vec<f32>, Vec<f32>)>,
        keyword_weight: f32,
        vector_weight: f32,
    ) -> Result<Vec<f32>, String> {
        tokio::task::spawn_blocking(move || {
            candidates
                .into_iter()
                .map(|(v1, v2)| {
                    let dot: f32 = v1.iter().zip(v2.iter()).map(|(a, b)| a * b).sum();
                    let norm1: f32 = v1.iter().map(|a| a * a).sum::<f32>().sqrt();
                    let norm2: f32 = v2.iter().map(|b| b * b).sum::<f32>().sqrt();

                    let cos_sim = if norm1 > 0.0 && norm2 > 0.0 {
                        dot / (norm1 * norm2)
                    } else {
                        0.0
                    };

                    // Blended score weighting vector similarity with keyword ratio
                    (cos_sim * vector_weight + (1.0 - cos_sim.abs()) * keyword_weight * 0.5)
                        .clamp(0.0, 1.0)
                })
                .collect()
        })
        .await
        .map_err(|e| format!("Task execution error in spawn_blocking: {e}"))
    }

    /// Run a single context regeneration pass.
    pub async fn run_regeneration_pass(
        &self,
        candidate_pairs: Vec<(Vec<f32>, Vec<f32>)>,
        mut top_k_results: Vec<ScoredResult>,
    ) -> Result<RegenerationResult, String> {
        let start = Instant::now();

        let kw_hits = self.keyword_hits.load(Ordering::Relaxed);
        let vw_hits = self.vector_hits.load(Ordering::Relaxed);
        let total_q = self.total_queries.load(Ordering::Relaxed);

        let (cur_kw, cur_vw) = self.current_weights().await;
        let (new_kw, new_vw) =
            self.calculate_rrf_weights(kw_hits, vw_hits, total_q, cur_kw, cur_vw);

        let weight_delta = (new_kw - cur_kw).abs() + (new_vw - cur_vw).abs();
        let converged = weight_delta < self.config.convergence_threshold;

        // Save updated weights
        {
            let mut w = self.weights.write().await;
            *w = (new_kw, new_vw);
        }

        let num_candidates = candidate_pairs.len();
        // Heavy similarity recalculation in spawn_blocking
        let recalculated_scores = self
            .recalculate_similarity_batch(candidate_pairs, new_kw, new_vw)
            .await?;

        let baseline_score: f64 = if !recalculated_scores.is_empty() {
            recalculated_scores.iter().sum::<f32>() as f64 / recalculated_scores.len() as f64
        } else {
            0.0
        };

        let top_k_shift = self.adjust_top_k_scores(&mut top_k_results, new_kw, new_vw);

        let regenerated_score: f64 = if !top_k_results.is_empty() {
            let target_len = top_k_results.len().min(self.config.target_top_k);
            top_k_results
                .iter()
                .take(target_len)
                .map(|r| r.score as f64)
                .sum::<f64>()
                / target_len as f64
        } else {
            baseline_score
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(RegenerationResult {
            baseline_score,
            regenerated_score,
            keyword_weight: new_kw,
            vector_weight: new_vw,
            top_k_score_shift: top_k_shift,
            converged,
            candidates_processed: num_candidates,
            duration_ms,
        })
    }

    /// Spawn background scheduled regeneration loop.
    pub fn start_regeneration_loop(
        self: Arc<Self>,
        mut shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(self.config.interval_secs));

            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        info!("ContextRegenerator background loop received shutdown signal.");
                        break;
                    }
                    _ = interval.tick() => {
                        debug!("Executing scheduled context regeneration pass...");
                        let sample_pair = vec![(vec![1.0, 0.0], vec![0.8, 0.2])];
                        let sample_results = vec![
                            ScoredResult {
                                id: "sample1".to_string(),
                                score: 0.8,
                                source: "working".to_string(),
                                path: "working/1".to_string(),
                                content: "sample content".to_string(),
                                updated_at: None,
                                zone: None,
                            }
                        ];

                        match self.run_regeneration_pass(sample_pair, sample_results).await {
                            Ok(res) => {
                                info!(
                                    "Regeneration pass completed in {}ms: converged={}, kw={:.3}, vw={:.3}",
                                    res.duration_ms, res.converged, res.keyword_weight, res.vector_weight
                                );
                            }
                            Err(e) => {
                                warn!("Error in scheduled regeneration pass: {}", e);
                            }
                        }
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_rrf_weights_equal_hits() {
        let regenerator = ContextRegenerator::with_defaults();
        let (kw, vw) = regenerator.calculate_rrf_weights(10, 10, 20, 0.5, 0.5);
        assert!((kw - 0.5).abs() < 0.001);
        assert!((vw - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_calculate_rrf_weights_vector_heavy() {
        let regenerator = ContextRegenerator::with_defaults();
        // 10 keyword hits vs 90 vector hits -> vector weight increases
        let (kw, vw) = regenerator.calculate_rrf_weights(10, 90, 100, 0.5, 0.5);
        assert!(
            vw > kw,
            "Vector weight ({vw}) should exceed keyword weight ({kw})"
        );
        assert!((kw + vw - 1.0).abs() < 0.001, "Weights should sum to 1.0");
    }

    #[test]
    fn test_calculate_rrf_weights_insufficient_samples() {
        let regenerator = ContextRegenerator::with_defaults();
        // 2 samples < min_hit_sample (5) -> keeps current weights
        let (kw, vw) = regenerator.calculate_rrf_weights(2, 0, 2, 0.4, 0.6);
        assert_eq!(kw, 0.4);
        assert_eq!(vw, 0.6);
    }

    #[test]
    fn test_adjust_top_k_scores() {
        let regenerator = ContextRegenerator::with_defaults();
        let mut results = vec![
            ScoredResult {
                id: "1".to_string(),
                score: 0.5,
                source: "working".to_string(),
                path: "".to_string(),
                content: "".to_string(),
                updated_at: None,
                zone: None,
            },
            ScoredResult {
                id: "2".to_string(),
                score: 0.5,
                source: "semantic".to_string(),
                path: "".to_string(),
                content: "".to_string(),
                updated_at: None,
                zone: None,
            },
        ];

        let shift = regenerator.adjust_top_k_scores(&mut results, 0.6, 0.4);
        assert!(shift > 0.0, "Score shift should be non-zero");
        // working gets multiplier kw * 2.0 = 1.2 -> score 0.5 * 1.2 = 0.6
        assert!((results[0].score - 0.6).abs() < 0.001);
        // semantic gets multiplier vw * 2.0 = 0.8 -> score 0.5 * 0.8 = 0.4
        assert!((results[1].score - 0.4).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_recalculate_similarity_batch_spawn_blocking() {
        let regenerator = ContextRegenerator::with_defaults();
        let candidates = vec![
            (vec![1.0, 0.0, 0.0], vec![1.0, 0.0, 0.0]),
            (vec![1.0, 0.0, 0.0], vec![0.0, 1.0, 0.0]),
        ];

        let scores = regenerator
            .recalculate_similarity_batch(candidates, 0.5, 0.5)
            .await
            .unwrap();

        assert_eq!(scores.len(), 2);
        assert!(
            scores[0] > scores[1],
            "Identical vectors must score higher than orthogonal vectors"
        );
    }

    #[tokio::test]
    async fn test_run_regeneration_pass_convergence() {
        let regenerator = ContextRegenerator::with_defaults();
        let candidates = vec![(vec![1.0, 0.0], vec![0.9, 0.1])];
        let results = vec![ScoredResult {
            id: "1".to_string(),
            score: 0.5,
            source: "working".to_string(),
            path: "".to_string(),
            content: "".to_string(),
            updated_at: None,
            zone: None,
        }];

        let res = regenerator
            .run_regeneration_pass(candidates, results)
            .await
            .unwrap();

        assert_eq!(res.candidates_processed, 1);
        assert!(
            res.converged,
            "With 0 query hits recorded, weights remain unchanged and pass converges"
        );
    }
}
