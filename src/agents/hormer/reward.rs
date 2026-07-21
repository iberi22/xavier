// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! HORMER Reward Model - Evaluation of retrieval quality
//!
//! Evaluates the effectiveness of a navigation policy by scoring
//! the resulting retrieval set against relevance and diversity metrics.

use crate::search::rrf::ScoredResult;
use std::sync::atomic::{AtomicU64, Ordering};

/// Reward model for retrieval evaluation
pub struct RewardModel {
    /// Minimum score for a result to be considered relevant
    pub relevance_threshold: f32,
    /// Historical sum of rewards (stored as micros to avoid float in atomics)
    total_reward_sum: AtomicU64,
    /// Total number of reward samples
    total_reward_count: AtomicU64,
}

impl Default for RewardModel {
    fn default() -> Self {
        Self {
            relevance_threshold: 0.5,
            total_reward_sum: AtomicU64::new(0),
            total_reward_count: AtomicU64::new(0),
        }
    }
}

impl RewardModel {
    pub fn new(relevance_threshold: f32) -> Self {
        Self {
            relevance_threshold,
            total_reward_sum: AtomicU64::new(0),
            total_reward_count: AtomicU64::new(0),
        }
    }

    /// Calculate total reward for a set of retrieved results
    pub fn calculate_reward(&self, results: &[ScoredResult]) -> f32 {
        if results.is_empty() {
            return 0.0;
        }

        let relevance = self.calculate_relevance_reward(results);
        let diversity = self.calculate_diversity_reward(results);

        // Final reward is a weighted combination of relevance and diversity
        let reward = (relevance * 0.7 + diversity * 0.3).clamp(0.0, 1.0);
        self.record_reward(reward);
        reward
    }

    /// Record a reward sample for historical tracking
    pub fn record_reward(&self, reward: f32) {
        let micros = (reward * 1_000_000.0) as u64;
        self.total_reward_sum.fetch_add(micros, Ordering::Relaxed);
        self.total_reward_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Get historical average reward
    pub fn get_average_reward(&self) -> f32 {
        let count = self.total_reward_count.load(Ordering::Relaxed);
        if count == 0 {
            return 0.0;
        }
        let sum = self.total_reward_sum.load(Ordering::Relaxed);
        (sum as f32 / count as f32) / 1_000_000.0
    }

    /// Relevance reward: mean score of results above threshold
    fn calculate_relevance_reward(&self, results: &[ScoredResult]) -> f32 {
        let relevant_count = results
            .iter()
            .filter(|r| r.score >= self.relevance_threshold)
            .count();
        if relevant_count == 0 {
            return 0.0;
        }

        let sum_score: f32 = results
            .iter()
            .filter(|r| r.score >= self.relevance_threshold)
            .map(|r| r.score)
            .sum();

        sum_score / relevant_count as f32
    }

    /// Diversity reward: penalize results from the same source
    fn calculate_diversity_reward(&self, results: &[ScoredResult]) -> f32 {
        if results.is_empty() {
            return 0.0;
        }

        let mut sources = std::collections::HashSet::with_capacity(results.len());
        for r in results {
            sources.insert(&r.source);
        }

        sources.len() as f32 / results.len() as f32
    }
}
