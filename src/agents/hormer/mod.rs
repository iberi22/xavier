//! HORMER - Learned Navigation Policy with Simplified GRPO
//!
//! Implementación simplificada de Group Relative Policy Optimization (GRPO)
//! para ajustar dinámicamente los pesos de las capas de memoria (Working, Episodic, Semantic).

pub mod reward;
#[cfg(test)]
mod tests;

use crate::memory::telemetry::NavTelemetry;
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
    /// Navigation telemetry: node hotspots, path counts, avg path length.
    telemetry: Arc<NavTelemetry>,
}

impl Hormer {
    pub fn new(policy: Arc<RwLock<NavigationPolicy>>) -> Self {
        Self {
            policy,
            reward_model: RewardModel::default(),
            navigated_queries: AtomicU64::new(0),
            non_navigated_queries: AtomicU64::new(0),
            score_histogram: Arc::new(RwLock::new([0; 10])),
            telemetry: Arc::new(NavTelemetry::new()),
        }
    }

    pub fn policy(&self) -> &Arc<RwLock<NavigationPolicy>> {
        &self.policy
    }

    /// Access the navigation telemetry collector (node visits, path lengths).
    pub fn telemetry(&self) -> &Arc<NavTelemetry> {
        &self.telemetry
    }

    /// Record a query that did not use the learned policy
    pub fn record_non_navigated(&self) {
        self.non_navigated_queries.fetch_add(1, Ordering::Relaxed);
    }

    /// Update telemetry metrics from retrieval results
    async fn update_metrics(&self, results: &[ScoredResult]) {
        self.navigated_queries.fetch_add(1, Ordering::Relaxed);

        // Navigation telemetry: count the completed path and each visited node.
        self.telemetry.record_path(results.len());
        for res in results {
            self.telemetry.record_visit(&res.id).await;
        }

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
            policy.last_reward = reward;
            policy.avg_reward = self.reward_model.get_average_reward();
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
        self.policy.read().await.layer_weights
    }
}
