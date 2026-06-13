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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

/// HORMER Coordinator
pub struct Hormer {
    policy: Arc<RwLock<NavigationPolicy>>,
    reward_model: RewardModel,
    /// Counter for queries that used the learned policy
    navigated_queries: AtomicU64,
    /// Counter for queries that used manual weights
    non_navigated_queries: AtomicU64,
    /// Histogram of retrieval scores (10 buckets: 0.0-0.1, 0.1-0.2, ..., 0.9-1.0)
    score_histogram: Arc<RwLock<[u64; 10]>>,
}

impl Hormer {
    pub fn new(policy: Arc<RwLock<NavigationPolicy>>) -> Self {
        Self {
            policy,
            reward_model: RewardModel::default(),
            navigated_queries: AtomicU64::new(0),
            non_navigated_queries: AtomicU64::new(0),
            score_histogram: Arc::new(RwLock::new([0; 10])),
        }
    }

    pub fn policy(&self) -> &Arc<RwLock<NavigationPolicy>> {
        &self.policy
    }

    /// Record a query that did not use the learned policy
    pub fn record_non_navigated(&self) {
        self.non_navigated_queries.fetch_add(1, Ordering::Relaxed);
    }

    /// Update telemetry metrics from retrieval results
    async fn update_metrics(&self, results: &[ScoredResult]) {
        self.navigated_queries.fetch_add(1, Ordering::Relaxed);

        let mut histogram = self.score_histogram.write().await;
        for res in results {
            let bucket = (res.score * 10.0).floor() as usize;
            let bucket = bucket.min(9);
            histogram[bucket] += 1;
        }
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
        self.update_metrics(results).await;
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
            policy.last_reward = reward;
            policy.avg_reward = self.reward_model.get_average_reward();

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

            if let Err(e) = settings.save().await {
                tracing::warn!("HORMER: Failed to persist policy: {}", e);
            }
        }
    }

    /// Get telemetry metrics
    pub async fn get_metrics(&self) -> serde_json::Value {
        let histogram = self.score_histogram.read().await;
        serde_json::json!({
            "navigated_queries": self.navigated_queries.load(Ordering::Relaxed),
            "non_navigated_queries": self.non_navigated_queries.load(Ordering::Relaxed),
            "average_reward": self.reward_model.get_average_reward(),
            "score_histogram": *histogram
        })
    }

    /// Get current weights from policy
    pub async fn get_weights(&self) -> LayerWeights {
        self.policy.read().await.weights
    }
}
