//! Navigation Policy - Learned retrieval layer and traversal weight management
//!
//! Provides the data structures and logic for maintaining and updating
//! the weights used for multi-layer memory retrieval and graph traversal.

use serde::{Deserialize, Serialize};
use super::gating::LayerWeights;

/// Weights for graph traversal signals
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TraversalWeights {
    pub semantic_similarity: f32,
    pub confidence: f32,
    pub edge_weight: f32,
    pub recency: f32,
    pub cross_layer: f32,
    pub cross_dir: f32,
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
#[derive(Debug, Clone, Copy, Default)]
pub struct NavigationScore {
    pub semantic_similarity: f32,
    pub confidence: f32,
    pub edge_weight: f32,
    pub recency: f32,
    pub cross_layer: f32,
    pub cross_dir: f32,
    pub peripheral_hub: f32,
}

/// Learned navigation policy for retrieval and traversal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavigationPolicy {
    /// Current base weights for each layer
    pub layer_weights: LayerWeights,
    /// Weights for graph traversal signals
    pub traversal_weights: TraversalWeights,
    /// Learning rate for weight updates (default 0.01)
    pub learning_rate: f32,
    /// Number of updates applied to this policy
    pub update_count: u64,
}

impl Default for NavigationPolicy {
    fn default() -> Self {
        Self {
            layer_weights: LayerWeights::default(),
            traversal_weights: TraversalWeights::default(),
            learning_rate: 0.01,
            update_count: 0,
        }
    }
}

impl NavigationPolicy {
    pub fn with_defaults() -> Self {
        Self::default()
    }
}

impl NavigationPolicy {
    pub fn new(layer_weights: LayerWeights, traversal_weights: TraversalWeights, learning_rate: f32) -> Self {
        Self {
            layer_weights,
            traversal_weights,
            learning_rate,
            update_count: 0,
        }
    }

    /// Update weights based on gradients (deltas)
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
