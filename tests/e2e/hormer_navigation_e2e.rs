use std::sync::Arc;
use tokio::sync::RwLock;
use xavier::agents::hormer::Hormer;
use xavier::memory::entity_graph::EntityRecord;
use xavier::memory::qmd_memory::MemoryDocument;
use xavier::retrieval::gating::SessionSummary;
use xavier::retrieval::{AdaptiveGating, GatingConfig, LayerWeights, NavigationPolicy};
use xavier::search::rrf::ScoredResult;

#[tokio::test]
async fn test_hormer_navigation_e2e_flow() {
    // 1. Initialize Navigation Policy and Adaptive Gating
    let initial_weights = LayerWeights::new(0.3, 0.3, 0.4);
    let policy = Arc::new(RwLock::new(NavigationPolicy::new(
        initial_weights,
        xavier::retrieval::policy::TraversalWeights::default(),
        0.1,
    )));

    let config = GatingConfig {
        relevance_threshold: 0.0,
        grounding_enabled: false,
        ..Default::default()
    };

    let gating = AdaptiveGating::with_policy(config, policy.clone());
    let hormer = Hormer::new(policy.clone());

    // 2. Prepare test documents, sessions, and entities
    let docs = vec![
        MemoryDocument {
            id: Some("doc_alpha".to_string()),
            path: "lib/alpha.rs".to_string(),
            content: "Xavier's decentralized governance and consensus mechanics.".to_string(),
            ..Default::default()
        },
        MemoryDocument {
            id: Some("doc_beta".to_string()),
            path: "lib/beta.rs".to_string(),
            content: "Optimized tensor computation graphs for neural networks.".to_string(),
            ..Default::default()
        },
    ];
    let sessions: Vec<SessionSummary> = vec![];
    let entities: Vec<EntityRecord> = vec![];

    // 3. Perform a retrieval query
    let query = "governance";
    let results = gating
        .retrieve(&docs, &sessions, &entities, query, None)
        .await;

    assert!(!results.is_empty(), "Should return matching documents");
    assert_eq!(
        results[0].id, "doc_alpha",
        "Highest relevant doc should match the query"
    );

    // 4. Update the policy based on interaction (HORMER adaptive feedback loop)
    // We simulate positive interaction with "working" layer results
    let weights_used = LayerWeights::new(0.7, 0.2, 0.1);
    let interaction_results = vec![ScoredResult {
        id: "doc_alpha".to_string(),
        content: "Xavier's decentralized governance and consensus mechanics.".to_string(),
        score: 0.98,
        source: "working".to_string(),
        path: "lib/alpha.rs".to_string(),
        updated_at: None,
        zone: None,
    }];

    // HORMER processes the interaction to update weights
    hormer
        .update_from_interaction(weights_used, &interaction_results, None)
        .await;

    // 5. Assert that the layer weights have updated adaptively
    let updated_weights = gating.effective_weights().await;
    assert_ne!(
        updated_weights.working, initial_weights.working,
        "The working weight should have evolved based on interaction"
    );
    assert!(
        updated_weights.is_valid(),
        "Updated weights must remain normalized and valid"
    );

    // 6. Perform a second retrieval query using the updated policy
    let results_after = gating
        .retrieve(&docs, &sessions, &entities, query, None)
        .await;
    assert!(!results_after.is_empty());
    assert_eq!(results_after[0].id, "doc_alpha");
}
