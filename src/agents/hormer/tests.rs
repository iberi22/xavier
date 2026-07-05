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
        0.1,
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
            zone: None,
        },
        ScoredResult {
            id: "2".to_string(),
            content: "Another relevant doc".to_string(),
            score: 0.8,
            source: "episodic".to_string(),
            path: "p2".to_string(),
            updated_at: None,
            zone: None,
        },
    ];

    let initial_count = policy.read().await.update_count;
    hormer
        .update_from_interaction(initial_weights, &results, None)
        .await;

    let updated_policy = policy.read().await;
    assert_eq!(updated_policy.update_count, initial_count + 1);

    // With high reward (relevance=0.85, diversity=1.0 -> reward=0.895)
    // Advantage = 0.895 - 0.5 = 0.395
    // Weights should increase
    let lw = updated_policy.layer_weights;
    let sum = lw.working + lw.episodic + lw.semantic;
    assert!((sum - 1.0).abs() < 0.001);

    // Verify metrics
    let metrics = hormer.get_metrics().await;
    assert_eq!(metrics["navigated_queries"], 1);
    assert_eq!(metrics["non_navigated_queries"], 0);
    assert!(metrics["average_reward"].as_f64().unwrap() > 0.0);
    let histogram = metrics["score_histogram"].as_array().unwrap();
    // bucket 0.9-1.0 (index 9) and 0.8-0.9 (index 8)
    assert_eq!(histogram[9].as_u64().unwrap(), 1);
    assert_eq!(histogram[8].as_u64().unwrap(), 1);
}

#[tokio::test]
async fn test_hormer_non_navigated_metric() {
    let policy = Arc::new(RwLock::new(NavigationPolicy::default()));
    let hormer = Hormer::new(policy);

    hormer.record_non_navigated();
    let metrics = hormer.get_metrics().await;
    assert_eq!(metrics["non_navigated_queries"], 1);
    assert_eq!(metrics["navigated_queries"], 0);
}

#[tokio::test]
async fn test_hormer_policy_no_update_on_low_advantage() {
    let initial_weights = LayerWeights::new(0.3, 0.3, 0.4);
    let policy = Arc::new(RwLock::new(NavigationPolicy::new(
        initial_weights,
        crate::retrieval::policy::TraversalWeights::default(),
        0.1,
    )));
    let hormer = Hormer::new(Arc::clone(&policy));

    // Reward = Relevance * 0.7 + Diversity * 0.3
    // To get Reward = 0.5 (Advantage 0.0):
    // 2 results from same source (Diversity = 0.5)
    // One result with score 0.5, one with 0.4 (Relevance = 0.5)
    // Reward = 0.5 * 0.7 + 0.5 * 0.3 = 0.35 + 0.15 = 0.5
    let results = vec![
        ScoredResult {
            id: "1".to_string(),
            content: "res 1".to_string(),
            score: 0.5,
            source: "working".to_string(),
            path: "path1".to_string(),
            updated_at: None,
            zone: None,
        },
        ScoredResult {
            id: "2".to_string(),
            content: "res 2".to_string(),
            score: 0.4,
            source: "working".to_string(),
            path: "path2".to_string(),
            updated_at: None,
            zone: None,
        },
    ];

    let initial_count = policy.read().await.update_count;
    hormer
        .update_from_interaction(initial_weights, &results, None)
        .await;

    let updated_policy = policy.read().await;
    assert_eq!(updated_policy.update_count, initial_count);
}

#[tokio::test]
async fn test_hormer_no_results_no_update() {
    let initial_weights = LayerWeights::new(0.3, 0.3, 0.4);
    let policy = Arc::new(RwLock::new(NavigationPolicy::new(
        initial_weights,
        crate::retrieval::policy::TraversalWeights::default(),
        0.1,
    )));
    let hormer = Hormer::new(Arc::clone(&policy));

    let results = vec![];
    hormer
        .update_from_interaction(initial_weights, &results, None)
        .await;

    let updated_policy = policy.read().await;
    assert_eq!(updated_policy.update_count, 0);
}
