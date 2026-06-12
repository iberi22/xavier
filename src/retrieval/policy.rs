//! Navigation Policy - Learned retrieval layer weight management
//!
//! Provides the data structures and logic for maintaining and updating
//! the weights used for multi-layer memory retrieval.

use serde::{Deserialize, Serialize};
use super::gating::LayerWeights;

/// Learned navigation policy for retrieval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavigationPolicy {
    /// Current base weights for each layer
    pub weights: LayerWeights,
    /// Learning rate for weight updates (default 0.01)
    pub learning_rate: f32,
    /// Number of updates applied to this policy
    pub update_count: u64,
}

impl Default for NavigationPolicy {
    fn default() -> Self {
        Self {
            weights: LayerWeights::default(),
            learning_rate: 0.01,
            update_count: 0,
        }
    }
}

impl NavigationPolicy {
    pub fn new(weights: LayerWeights, learning_rate: f32) -> Self {
        Self {
            weights,
            learning_rate,
            update_count: 0,
        }
    }

    /// Update weights based on a gradient (delta)
    pub fn update(&mut self, deltas: LayerWeights) {
        self.weights.working = (self.weights.working + self.learning_rate * deltas.working).max(0.0);
        self.weights.episodic = (self.weights.episodic + self.learning_rate * deltas.episodic).max(0.0);
        self.weights.semantic = (self.weights.semantic + self.learning_rate * deltas.semantic).max(0.0);

        self.normalize();
        self.update_count += 1;
    }

    /// Normalize weights so they sum to 1.0
    fn normalize(&mut self) {
        let sum = self.weights.working + self.weights.episodic + self.weights.semantic;
        if sum > 0.0 {
            self.weights.working /= sum;
            self.weights.episodic /= sum;
            self.weights.semantic /= sum;
        } else {
            // Fallback to defaults if something goes wrong
            self.weights = LayerWeights::default();
        }
    }
}
