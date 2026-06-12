//! HORMER Reward Model - Evaluation of retrieval quality
//!
//! Evaluates the effectiveness of a navigation policy by scoring
//! the resulting retrieval set against relevance and diversity metrics.

use crate::search::rrf::ScoredResult;

/// Reward model for retrieval evaluation
pub struct RewardModel {
    /// Minimum score for a result to be considered relevant
    pub relevance_threshold: f32,
}

impl Default for RewardModel {
    fn default() -> Self {
        Self {
            relevance_threshold: 0.5,
        }
    }
}

impl RewardModel {
    pub fn new(relevance_threshold: f32) -> Self {
        Self { relevance_threshold }
    }

    /// Calculate total reward for a set of retrieved results
    pub fn calculate_reward(&self, results: &[ScoredResult]) -> f32 {
        if results.is_empty() {
            return 0.0;
        }

        let relevance = self.calculate_relevance_reward(results);
        let diversity = self.calculate_diversity_reward(results);

        // Final reward is a weighted combination of relevance and diversity
        (relevance * 0.7 + diversity * 0.3).clamp(0.0, 1.0)
    }

    /// Relevance reward: mean score of results above threshold
    fn calculate_relevance_reward(&self, results: &[ScoredResult]) -> f32 {
        let relevant_count = results.iter().filter(|r| r.score >= self.relevance_threshold).count();
        if relevant_count == 0 {
            return 0.0;
        }

        let sum_score: f32 = results.iter()
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
