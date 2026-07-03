//! Navigation Policy - Learned retrieval layer and traversal weight management
//!
//! Provides the data structures and logic for maintaining and updating
//! the weights used for multi-layer memory retrieval and graph traversal.

use serde::{Deserialize, Serialize};
use super::gating::LayerWeights;
use super::tuner::RetrievalConfig;

/// Weights for graph traversal signals
///
/// These weights are used by the `NavigationPolicy` to score and rank
/// potential transitions (edges) during graph-based memory traversal.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TraversalWeights {
    /// Importance of text/semantic match between query and target node.
    pub semantic_similarity: f32,
    /// Importance of the edge's inherent confidence score (from indexer).
    pub confidence: f32,
    /// Importance of the edge weight/strength signal.
    pub edge_weight: f32,
    /// Importance of temporal recency (newer edges favored).
    pub recency: f32,
    /// Bonus for transitions that cross memory layers (e.g., Working to Semantic).
    pub cross_layer: f32,
    /// Bonus/penalty for crossing directory boundaries in the codebase.
    pub cross_dir: f32,
    /// Bonus for navigating towards high-degree "hub" nodes.
    pub peripheral_hub: f32,
}

impl Default for TraversalWeights {
    fn default() -> Self {
        Self {
            semantic_similarity: 0.5,
            confidence: 0.1,
            edge_weight: 0.1,
            recency: 0.1,
            cross_layer: 0.05,
            cross_dir: 0.1,
            peripheral_hub: 0.05,
        }
    }
}

/// Decomposed score components for a navigation transition
///
/// Represents the raw signals calculated for a single graph edge
/// before being combined via `TraversalWeights`.
#[derive(Debug, Clone, Copy, Default)]
pub struct NavigationScore {
    /// Raw semantic match score.
    pub semantic_similarity: f32,
    /// Raw edge confidence.
    pub confidence: f32,
    /// Raw edge weight.
    pub edge_weight: f32,
    /// Calculated recency score (usually via sigmoid decay).
    pub recency: f32,
    /// Whether this transition crosses memory layers.
    pub cross_layer: f32,
    /// Whether this transition crosses directory boundaries.
    pub cross_dir: f32,
    /// Degree-based boost component.
    pub peripheral_hub: f32,
}

/// Learned navigation policy for retrieval and traversal
///
/// The `NavigationPolicy` manages the weights used to gate different memory
/// layers (Working, Episodic, Semantic) and the weights used to navigate
/// the belief graph. It can be updated online via reinforcement learning
/// (e.g., HORMER's GRPO implementation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavigationPolicy {
    /// Current base weights for each memory layer.
    pub layer_weights: LayerWeights,
    /// Weights for graph traversal signals.
    pub traversal_weights: TraversalWeights,
    /// Learning rate for weight updates (default 0.01).
    pub learning_rate: f32,
    /// Number of updates applied to this policy.
    pub update_count: u64,
    /// Last reward received (0.0-1.0) during an update.
    pub last_reward: f32,
    /// Historical average reward (running average).
    pub avg_reward: f32,
}

impl Default for NavigationPolicy {
    fn default() -> Self {
        Self {
            layer_weights: LayerWeights::default(),
            traversal_weights: TraversalWeights::default(),
            learning_rate: 0.01,
            update_count: 0,
            last_reward: 0.0,
            avg_reward: 0.0,
        }
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[test]
    fn test_score_for_prefetch() {
        let policy = NavigationPolicy::default();
        // Default weights: working=0.3, episodic=0.3, semantic=0.4

        let score_w = policy.score_for_prefetch("working", 0.8);
        assert!((score_w - 0.24).abs() < 0.001);

        let score_s = policy.score_for_prefetch("semantic", 0.8);
        assert!((score_s - 0.32).abs() < 0.001);

        let score_unknown = policy.score_for_prefetch("unknown", 0.8);
        assert_eq!(score_unknown, 0.0);
    }

    #[test]
    fn test_apply_retrieval_config_changes_layer_weights() {
        // A semantic-emphasis config must shift layer weights toward semantic.
        let mut policy = NavigationPolicy::default();
        let before = (
            policy.layer_weights.working,
            policy.layer_weights.episodic,
            policy.layer_weights.semantic,
        );
        let config = super::RetrievalConfig {
            rrf_k: 60,
            keyword_weight: 0.5,
            vector_weight: 0.5,
            working_weight: 0.2,
            episodic_weight: 0.2,
            semantic_weight: 0.6,
        };
        policy.apply_retrieval_config(&config);
        let after = (
            policy.layer_weights.working,
            policy.layer_weights.episodic,
            policy.layer_weights.semantic,
        );
        assert_ne!(before, after, "layer weights must change after apply");
        // Semantic should now be the dominant layer.
        assert!(
            after.2 > after.0 && after.2 > after.1,
            "semantic must dominate after applying semantic-emphasis config"
        );
        // Weights must remain a valid distribution.
        let sum = after.0 + after.1 + after.2;
        assert!((sum - 1.0).abs() < 1e-4, "layer weights must sum to 1.0, got {sum}");
        // update_count must bump to record the tuner-derived change.
        assert!(policy.update_count >= 1);
    }

    #[test]
    fn test_apply_retrieval_config_vector_emphasis_amplifies_semantic_traversal() {
        // A vector-heavy config should not reduce semantic_similarity below default.
        let mut policy = NavigationPolicy::default();
        let semantic_before = policy.traversal_weights.semantic_similarity;
        let config = super::RetrievalConfig {
            rrf_k: 80,
            keyword_weight: 0.2,
            vector_weight: 0.8,
            ..super::RetrievalConfig::default()
        };
        policy.apply_retrieval_config(&config);
        // Vector share = 0.8/1.0 = 0.8 -> multiplier = 1.6, so semantic grows.
        assert!(
            policy.traversal_weights.semantic_similarity > semantic_before,
            "vector-emphasis should amplify semantic_similarity traversal weight"
        );
        // Traversal weights remain normalized.
        let tw = policy.traversal_weights;
        let sum = tw.semantic_similarity + tw.confidence + tw.edge_weight + tw.recency
            + tw.cross_layer + tw.cross_dir + tw.peripheral_hub;
        assert!((sum - 1.0).abs() < 1e-4, "traversal weights must sum to 1.0, got {sum}");
    }

    #[test]
    fn test_apply_retrieval_config_keyword_emphasis_damps_semantic_traversal() {
        // A keyword-heavy config should damp semantic_similarity traversal signal.
        let mut policy = NavigationPolicy::default();
        let semantic_before = policy.traversal_weights.semantic_similarity;
        let config = super::RetrievalConfig {
            rrf_k: 40,
            keyword_weight: 0.8,
            vector_weight: 0.2,
            ..super::RetrievalConfig::default()
        };
        policy.apply_retrieval_config(&config);
        assert!(
            policy.traversal_weights.semantic_similarity < semantic_before,
            "keyword-emphasis should damp semantic_similarity traversal weight"
        );
    }

    #[test]
    fn test_apply_retrieval_config_degenerate_falls_back_to_default() {
        // All-zero layer weights must not leave the policy in a degenerate state.
        let mut policy = NavigationPolicy::default();
        let config = super::RetrievalConfig {
            working_weight: 0.0,
            episodic_weight: 0.0,
            semantic_weight: 0.0,
            ..super::RetrievalConfig::default()
        };
        policy.apply_retrieval_config(&config);
        // normalize_layers restores the LayerWeights::default() distribution on a
        // zero sum, so the policy stays usable.
        let lw = policy.layer_weights;
        let sum = lw.working + lw.episodic + lw.semantic;
        assert!(
            sum > 0.0 && (sum - 1.0).abs() < 1e-4,
            "degenerate config must normalize back to a valid distribution, sum={sum}"
        );
    }
}

impl NavigationPolicy {
    /// Creates a new policy with default weights and parameters.
    pub fn with_defaults() -> Self {
        Self::default()
    }

    /// Creates a new policy with explicit weights and learning rate.
    pub fn new(layer_weights: LayerWeights, traversal_weights: TraversalWeights, learning_rate: f32) -> Self {
        Self {
            layer_weights,
            traversal_weights,
            learning_rate,
            update_count: 0,
            last_reward: 0.0,
            avg_reward: 0.0,
        }
    }

    /// Update weights based on gradients (deltas).
    ///
    /// Deltas are multiplied by the `learning_rate` and added to current weights.
    /// Weights are then normalized to ensure they sum to 1.0.
    pub fn update(&mut self, layer_deltas: LayerWeights, traversal_deltas: TraversalWeights) {
        // Update layer weights
        self.layer_weights.working = (self.layer_weights.working + self.learning_rate * layer_deltas.working).max(0.0);
        self.layer_weights.episodic = (self.layer_weights.episodic + self.learning_rate * layer_deltas.episodic).max(0.0);
        self.layer_weights.semantic = (self.layer_weights.semantic + self.learning_rate * layer_deltas.semantic).max(0.0);
        self.normalize_layers();

        // Update traversal weights
        self.traversal_weights.semantic_similarity = (self.traversal_weights.semantic_similarity + self.learning_rate * traversal_deltas.semantic_similarity).max(0.0);
        self.traversal_weights.confidence = (self.traversal_weights.confidence + self.learning_rate * traversal_deltas.confidence).max(0.0);
        self.traversal_weights.edge_weight = (self.traversal_weights.edge_weight + self.learning_rate * traversal_deltas.edge_weight).max(0.0);
        self.traversal_weights.recency = (self.traversal_weights.recency + self.learning_rate * traversal_deltas.recency).max(0.0);
        self.traversal_weights.cross_layer = (self.traversal_weights.cross_layer + self.learning_rate * traversal_deltas.cross_layer).max(0.0);
        self.traversal_weights.cross_dir = (self.traversal_weights.cross_dir + self.learning_rate * traversal_deltas.cross_dir).max(0.0);
        self.traversal_weights.peripheral_hub = (self.traversal_weights.peripheral_hub + self.learning_rate * traversal_deltas.peripheral_hub).max(0.0);
        self.normalize_traversal();

        self.update_count += 1;
    }

    /// Score a layer for prefetching based on its weight and base relevance
    pub fn score_for_prefetch(&self, layer: &str, base_relevance: f32) -> f32 {
        let weight = match layer {
            "working" => self.layer_weights.working,
            "episodic" => self.layer_weights.episodic,
            "semantic" => self.layer_weights.semantic,
            _ => 0.0,
        };
        base_relevance * weight
    }

    /// Apply a tuned `RetrievalConfig` to the navigation policy.
    ///
    /// This is the bridge from the RRF tuner (`retrieval::tuner`) into HORMER's
    /// navigation policy: it copies the recommended layer weights (working /
    /// episodic / semantic) directly into `layer_weights`, normalizing them so
    /// they remain a valid distribution, and nudges the traversal weights so that
    /// the semantic-similarity signal tracks the tuner's vector/keyword balance.
    ///
    /// Concretely:
    /// - `layer_weights` are overwritten from the config (then renormalized),
    ///   so retrieval gating and graph traversal both reflect the tuned emphasis
    ///   (e.g. semantic-emphasis from the tuner tilts navigation toward memory).
    /// - `traversal_weights.semantic_similarity` is scaled toward the config's
    ///   `vector_weight`: a more vector-heavy config amplifies semantic match,
    ///   a more keyword-heavy config damps it. The traversal weights are then
    ///   renormalized.
    /// - `update_count` is bumped so callers can observe that a tuner-derived
    ///   change has been applied (distinct from the online RL `update` path).
    ///
    /// Weights that fall to zero (or a degenerate all-zero config) are clamped to
    /// the defaults rather than left empty, mirroring the behavior of
    /// `normalize_layers` / `normalize_traversal`.
    pub fn apply_retrieval_config(&mut self, config: &RetrievalConfig) {
        // 1. Layer weights come straight from the tuner's recommended config.
        self.layer_weights.working = config.working_weight.max(0.0);
        self.layer_weights.episodic = config.episodic_weight.max(0.0);
        self.layer_weights.semantic = config.semantic_weight.max(0.0);
        self.normalize_layers();

        // 2. Tilt traversal semantic similarity toward the vector/keyword balance.
        //    vector_weight ∈ [0,1]; rescale the existing semantic weight by the
        //    ratio of vector to keyword emphasis, guarding against division by zero.
        let total = config.keyword_weight + config.vector_weight;
        if total > 0.0 {
            let vector_share = config.vector_weight / total; // 0.5 at the default 50/50
            // At the default 50/50 split, vector_share == 1.0 so the weight is unchanged.
            // A purely vector config doubles semantic similarity; a purely keyword
            // config zeroes it (then normalization restores the rest of the signals).
            self.traversal_weights.semantic_similarity =
                (self.traversal_weights.semantic_similarity * vector_share * 2.0).max(0.0);
            self.normalize_traversal();
        }

        self.update_count += 1;
    }

    /// Normalize layer weights so they sum to 1.0
    fn normalize_layers(&mut self) {
        let sum = self.layer_weights.working + self.layer_weights.episodic + self.layer_weights.semantic;
        if sum > 0.0 {
            self.layer_weights.working /= sum;
            self.layer_weights.episodic /= sum;
            self.layer_weights.semantic /= sum;
        } else {
            self.layer_weights = LayerWeights::default();
        }
    }

    /// Normalize traversal weights so they sum to 1.0
    fn normalize_traversal(&mut self) {
        let sum = self.traversal_weights.semantic_similarity +
                  self.traversal_weights.confidence +
                  self.traversal_weights.edge_weight +
                  self.traversal_weights.recency +
                  self.traversal_weights.cross_layer +
                  self.traversal_weights.cross_dir +
                  self.traversal_weights.peripheral_hub;

        if sum > 0.0 {
            self.traversal_weights.semantic_similarity /= sum;
            self.traversal_weights.confidence /= sum;
            self.traversal_weights.edge_weight /= sum;
            self.traversal_weights.recency /= sum;
            self.traversal_weights.cross_layer /= sum;
            self.traversal_weights.cross_dir /= sum;
            self.traversal_weights.peripheral_hub /= sum;
        } else {
            self.traversal_weights = TraversalWeights::default();
        }
    }
}
