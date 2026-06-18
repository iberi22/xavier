use std::sync::Arc;
use tokio::sync::RwLock;
use xavier::agents::hormer::Hormer;
use xavier::memory::belief_graph::BeliefGraph;
use xavier::retrieval::{LayerWeights, NavigationPolicy};
use xavier::search::rrf::ScoredResult;

#[tokio::test]
async fn test_hormer_navigation_at_scale() {
    // 1. Setup scale environment
    let graph = Arc::new(RwLock::new(BeliefGraph::new()));
    let policy = Arc::new(RwLock::new(NavigationPolicy::with_defaults()));
    let hormer = Hormer::new(Arc::clone(&policy));

    {
        let g = graph.write().await;
        // Populate 1000 nodes
        for i in 0..1000 {
            g.add_node(format!("concept_{}", i), 0.8, None);
        }
        // Add 2000 edges
        for i in 0..1000 {
            let next = (i + 1) % 1000;
            let skip = (i + 5) % 1000;
            g.add_relation(format!("concept_{}", i), format!("concept_{}", next), "leads_to".to_string(), None, None).await.unwrap();
            g.add_relation(format!("concept_{}", i), format!("concept_{}", skip), "shortcut".to_string(), None, None).await.unwrap();
        }
    }

    // 2. Simulate 50 navigation/interaction cycles
    for i in 0..50 {
        let weights = hormer.get_weights().await;

        // Mock results for a "successful" navigation
        let results = vec![
            ScoredResult {
                id: format!("concept_{}", i),
                content: format!("Content for concept {}", i),
                score: 0.9,
                source: "semantic".to_string(),
                ..Default::default()
            },
            ScoredResult {
                id: format!("concept_{}", i + 1),
                content: format!("Content for concept {}", i + 1),
                score: 0.8,
                source: "semantic".to_string(),
                ..Default::default()
            },
        ];

        hormer.update_from_interaction(weights, &results).await;

        // Verify weights remain valid
        let current_weights = hormer.get_weights().await;
        assert!(current_weights.is_valid(), "Weights must sum to ~1.0 after update {}", i);
    }

    let metrics = hormer.get_metrics().await;
    assert_eq!(metrics["navigated_queries"], 50);
}
