//! Tests for HORMER GRPO policy updates
use super::*;
use crate::retrieval::{LayerWeights, NavigationPolicy};
use crate::search::rrf::ScoredResult;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_hormer_policy_update_positive() {
    let initial_weights = LayerWeights::new(0.3, 0.3, 0.4);
    let policy = Arc::new(RwLock::new(NavigationPolicy::new(
        initial_weights,
        crate::retrieval::policy::TraversalWeights::default(),
        0.1
    )));
    let hormer = Hormer::new(Arc::clone(&policy));

    // Results with high relevance (simulating a good interaction)
    let results = vec![
        ScoredResult {
            id: "1".to_string(),
            content: "Relevant doc".to_string(),
            score: 0.9,
            source: "working".to_string(),
            path: "p1".to_string(),
            updated_at: None,
        },
        ScoredResult {
            id: "2".to_string(),
            content: "Another relevant doc".to_string(),
            score: 0.8,
            source: "episodic".to_string(),
            path: "p2".to_string(),
            updated_at: None,
        },
    ];

    hormer.update_from_interaction(initial_weights, &results).await;

    let updated_policy = policy.read().await;
    assert!(updated_policy.update_count > 0);
    // Layer weights should still be valid (sum to 1)
    let lw = updated_policy.layer_weights;
    let sum = lw.working + lw.episodic + lw.semantic;
    assert!((sum - 1.0).abs() < 0.001);
}

#[tokio::test]
async fn test_hormer_no_update_on_low_advantage() {
    let initial_weights = LayerWeights::new(0.3, 0.3, 0.4);
    let policy = Arc::new(RwLock::new(NavigationPolicy::new(
        initial_weights,
        crate::retrieval::policy::TraversalWeights::default(),
        0.1
    )));
    let hormer = Hormer::new(Arc::clone(&policy));

    // Results with average relevance (advantage ~0)
    // relevance = 0.5, diversity = 0.5 -> reward = 0.5
    let results = vec![
        ScoredResult {
            id: "1".to_string(),
            content: "Meh doc".to_string(),
            score: 0.5,
            source: "working".to_string(),
            path: "p1".to_string(),
            updated_at: None,
        },
        ScoredResult {
            id: "2".to_string(),
            content: "Another meh doc".to_string(),
            score: 0.5,
            source: "working".to_string(),
            path: "p2".to_string(),
            updated_at: None,
        },
    ];

    hormer.update_from_interaction(initial_weights, &results).await;

    let updated_policy = policy.read().await;
    // Advantage calculation: reward(0.5) - 0.5 = 0.0
    // No update should happen when advantage.abs() <= 0.05
    assert_eq!(updated_policy.update_count, 0);
}

#[tokio::test]
async fn test_hormer_no_results_no_update() {
    let initial_weights = LayerWeights::new(0.3, 0.3, 0.4);
    let policy = Arc::new(RwLock::new(NavigationPolicy::new(
        initial_weights,
        crate::retrieval::policy::TraversalWeights::default(),
        0.1
    )));
    let hormer = Hormer::new(Arc::clone(&policy));

    let results = vec![];
    hormer.update_from_interaction(initial_weights, &results).await;

    let updated_policy = policy.read().await;
    assert_eq!(updated_policy.update_count, 0);
}
