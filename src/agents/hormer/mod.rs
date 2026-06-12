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
        let reward = self.reward_model.calculate_reward(results);

        // Simplified Advantage calculation:
        // In a full GRPO we would have multiple samples (group) and calculate
        // (reward - mean(rewards)) / std(rewards).
        // Here we use a simpler heuristic: if reward > 0.6, we consider it a positive signal.
        let advantage = reward - 0.5;

        if advantage.abs() > 0.05 {
            let mut policy = self.policy.write().await;

            // The delta is proportional to the advantage and the weights that produced the result
            let delta = LayerWeights::new(
                weights_used.working * advantage,
                weights_used.episodic * advantage,
                weights_used.semantic * advantage,
            );

            policy.update(delta);
            tracing::info!(
                reward = reward,
                advantage = advantage,
                "HORMER: Policy updated. New weights: w={:.2} e={:.2} s={:.2}",
                policy.weights.working,
                policy.weights.episodic,
                policy.weights.semantic
            );

            // Persist the updated policy to settings
            let mut settings = crate::settings::XavierSettings::current();
            settings.retrieval.learned_policy.working_weight = policy.weights.working;
            settings.retrieval.learned_policy.episodic_weight = policy.weights.episodic;
            settings.retrieval.learned_policy.semantic_weight = policy.weights.semantic;
            settings.retrieval.learned_policy.update_count = policy.update_count;

            if let Err(e) = settings.save() {
                tracing::warn!("HORMER: Failed to persist policy: {}", e);
            }
        }
    }

    /// Get current weights from policy
    pub async fn get_weights(&self) -> LayerWeights {
        self.policy.read().await.weights
    }
}
