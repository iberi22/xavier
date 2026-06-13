//! HORMER - Learned Navigation Policy with Simplified GRPO
//!
//! Implementación simplificada de Group Relative Policy Optimization (GRPO)
//! para ajustar dinámicamente los pesos de las capas de memoria (Working, Episodic, Semantic).

pub mod reward;
#[cfg(test)]
mod tests;

use crate::retrieval::{LayerWeights, NavigationPolicy};
use crate::search::rrf::ScoredResult;
use reward::RewardModel;
use std::sync::Arc;
use tokio::sync::RwLock;

/// HORMER Coordinator
pub struct Hormer {
    policy: Arc<RwLock<NavigationPolicy>>,
    reward_model: RewardModel,
}

impl Hormer {
    pub fn new(policy: Arc<RwLock<NavigationPolicy>>) -> Self {
        Self {
            policy,
            reward_model: RewardModel::default(),
        }
    }

    pub fn policy(&self) -> &Arc<RwLock<NavigationPolicy>> {
        &self.policy
    }

    /// Update the policy using a simplified GRPO approach
    ///
    /// GRPO (Group Relative Policy Optimization) updates the policy based on
    /// the relative advantage of a group of samples.
    ///
    /// En esta versión simplificada:
    /// 1. Tomamos los resultados de una navegación.
    /// 2. Evaluamos el reward.
    /// 3. Si el reward es superior al promedio histórico (o una base), ajustamos pesos.
    pub async fn update_from_interaction(&self, weights_used: LayerWeights, results: &[ScoredResult]) {
        if results.is_empty() {
            return;
        }
        let reward = self.reward_model.calculate_reward(results);

        // Simplified Advantage calculation:
        // In a full GRPO we would have multiple samples (group) and calculate
        // (reward - mean(rewards)) / std(rewards).
        // Here we use a simpler heuristic: if reward > 0.6, we consider it a positive signal.
        let advantage = reward - 0.5;

        if advantage.abs() > 0.05 {
            let mut policy = self.policy.write().await;

            // The delta is proportional to the advantage and the weights that produced the result
            let layer_delta = LayerWeights::new(
                weights_used.working * advantage,
                weights_used.episodic * advantage,
                weights_used.semantic * advantage,
            );

            // For now, update traversal weights with a smaller fixed delta based on advantage
            // In a full implementation, we would track which traversal signals were active.
            let traversal_weights_used = policy.traversal_weights;
            let traversal_delta = crate::retrieval::policy::TraversalWeights {
                semantic_similarity: traversal_weights_used.semantic_similarity * advantage,
                confidence: traversal_weights_used.confidence * advantage,
                edge_weight: traversal_weights_used.edge_weight * advantage,
                recency: traversal_weights_used.recency * advantage,
                cross_layer: traversal_weights_used.cross_layer * advantage,
                cross_dir: traversal_weights_used.cross_dir * advantage,
                peripheral_hub: traversal_weights_used.peripheral_hub * advantage,
            };

            policy.update(layer_delta, traversal_delta);
            tracing::info!(
                reward = reward,
                advantage = advantage,
                "HORMER: Policy updated. New weights: w={:.2} e={:.2} s={:.2}",
                policy.layer_weights.working,
                policy.layer_weights.episodic,
                policy.layer_weights.semantic
            );

            // Persist the updated policy to settings
            let mut settings = crate::settings::XavierSettings::current();
            settings.retrieval.learned_policy.working_weight = policy.layer_weights.working;
            settings.retrieval.learned_policy.episodic_weight = policy.layer_weights.episodic;
            settings.retrieval.learned_policy.semantic_weight = policy.layer_weights.semantic;

            settings.retrieval.learned_policy.semantic_similarity_weight = policy.traversal_weights.semantic_similarity;
            settings.retrieval.learned_policy.confidence_weight = policy.traversal_weights.confidence;
            settings.retrieval.learned_policy.edge_weight = policy.traversal_weights.edge_weight;
            settings.retrieval.learned_policy.recency_weight = policy.traversal_weights.recency;
            settings.retrieval.learned_policy.cross_layer_weight = policy.traversal_weights.cross_layer;
            settings.retrieval.learned_policy.cross_dir_weight = policy.traversal_weights.cross_dir;
            settings.retrieval.learned_policy.peripheral_hub_weight = policy.traversal_weights.peripheral_hub;
            settings.retrieval.learned_policy.update_count = policy.update_count;

            if let Err(e) = settings.save().await {
                tracing::warn!("HORMER: Failed to persist policy: {}", e);
            }
        }
    }

    /// Get current weights from policy
    pub async fn get_weights(&self) -> LayerWeights {
        self.policy.read().await.layer_weights
    }
}
