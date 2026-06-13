//! Navigation Policy for intelligent memory traversal
//!
//! Implements scoring for graph transitions based on multiple signals:
//! cosine similarity, edge confidence, node importance, and context relevance.

use crate::domain::memory::belief::BeliefEdge;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct NavigationWeights {
    pub semantic_similarity: f32,
    pub confidence: f32,
    pub edge_weight: f32,
    pub recency: f32,
}

impl Default for NavigationWeights {
    fn default() -> Self {
        Self {
            semantic_similarity: 0.5,
            confidence: 0.2,
            edge_weight: 0.2,
            recency: 0.1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavigationPolicy {
    pub weights: NavigationWeights,
}

impl NavigationPolicy {
    pub fn new(weights: NavigationWeights) -> Self {
        Self { weights }
    }

    pub fn with_defaults() -> Self {
        Self {
            weights: NavigationWeights::default(),
        }
    }

    /// Scores a transition (edge) from a current node towards a target given a query.
    pub fn score_transition(
        &self,
        query: &str,
        edge: &BeliefEdge,
        now: chrono::DateTime<chrono::Utc>,
    ) -> f32 {
        let query_lower = query.to_lowercase();
        let target_lower = edge.target.to_lowercase();
        let relation_lower = edge.relation_type.to_lowercase();

        // 1. Semantic similarity (lexical match for now as proxy)
        let mut similarity = 0.0_f32;
        if target_lower.contains(&query_lower) || query_lower.contains(&target_lower) {
            similarity = 1.0;
        } else {
            let query_terms: Vec<&str> = query_lower.split_whitespace().collect();
            let mut matches = 0;
            for term in &query_terms {
                if target_lower.contains(term) || relation_lower.contains(term) {
                    matches += 1;
                }
            }
            if !query_terms.is_empty() {
                similarity = matches as f32 / query_terms.len() as f32;
            }
        }

        // 2. Confidence score
        let confidence = edge.confidence_score;

        // 3. Edge weight
        let weight = edge.weight;

        // 4. Recency
        let age_hours = (now - edge.updated_at).num_hours() as f32;
        let recency = if age_hours <= 0.0 {
            1.0
        } else {
            // Exponential decay: e^(-age / 168) where 168 is one week
            (-age_hours / 168.0).exp()
        };

        (similarity * self.weights.semantic_similarity)
            + (confidence * self.weights.confidence)
            + (weight * self.weights.edge_weight)
            + (recency * self.weights.recency)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::memory::belief::BeliefEdge;

    #[test]
    fn test_score_transition_exact_match() {
        let policy = NavigationPolicy::with_defaults();
        let edge = BeliefEdge::new(
            "Xavier".to_string(),
            "Rust".to_string(),
            "written_in".to_string(),
            0.9,
            "provenance".to_string(),
        );
        let score = policy.score_transition("Rust", &edge, chrono::Utc::now());
        // similarity (1.0 * 0.5) + confidence (0.9 * 0.2) + weight (0.9 * 0.2) + recency (1.0 * 0.1)
        // 0.5 + 0.18 + 0.18 + 0.1 = 0.96
        assert!((score - 0.96).abs() < 0.001);
    }

    #[test]
    fn test_score_transition_no_match() {
        let policy = NavigationPolicy::with_defaults();
        let edge = BeliefEdge::new(
            "Xavier".to_string(),
            "Rust".to_string(),
            "written_in".to_string(),
            0.9,
            "provenance".to_string(),
        );
        let score = policy.score_transition("Python", &edge, chrono::Utc::now());
        // similarity (0.0 * 0.5) + confidence (0.9 * 0.2) + weight (0.9 * 0.2) + recency (1.0 * 0.1)
        // 0.0 + 0.18 + 0.18 + 0.1 = 0.46
        assert!((score - 0.46).abs() < 0.001);
    }
}
