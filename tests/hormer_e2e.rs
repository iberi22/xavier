use std::sync::Arc;
use tokio::sync::RwLock;
use xavier::agents::hormer::Hormer;
use xavier::retrieval::{AdaptiveGating, GatingConfig, LayerWeights, NavigationPolicy};
use xavier::memory::qmd_memory::MemoryDocument;
use xavier::retrieval::gating::SessionSummary;
use xavier::memory::entity_graph::EntityRecord;
use xavier::search::rrf::ScoredResult;

#[tokio::test]
async fn test_hormer_full_pipeline_e2e() {
    // 1. Setup
    let initial_weights = LayerWeights::new(0.4, 0.3, 0.3);
    let policy = Arc::new(RwLock::new(NavigationPolicy::new(
        initial_weights,
        xavier::retrieval::policy::TraversalWeights::default(),
        0.1,
    )));

    let mut config = GatingConfig::default();
    config.relevance_threshold = 0.0; // Ensure we see results
    config.grounding_enabled = false;

    let gating = AdaptiveGating::with_policy(config, policy.clone());
    let hormer = Hormer::new(policy.clone());

    // 2. Mock Data
    let docs = vec![
        MemoryDocument {
            id: Some("doc_1".to_string()),
            path: "src/main.rs".to_string(),
            content: "Xavier core logic".to_string(),
            ..Default::default()
        },
        MemoryDocument {
            id: Some("doc_2".to_string()),
            path: "README.md".to_string(),
            content: "Project documentation".to_string(),
            ..Default::default()
        },
    ];
    let sessions: Vec<SessionSummary> = vec![];
    let entities: Vec<EntityRecord> = vec![];

    // 3. First Retrieval
    let query = "Xavier";
    let results = gating.retrieve(&docs, &sessions, &entities, query, None).await;

    assert!(!results.is_empty(), "Should return results");
    assert_eq!(results[0].id, "doc_1");

    // 4. Interaction Simulation (HORMER learns)
    // We simulate that 'working' layer results were very good.
    // To see a change in weights, we simulate that we used a different weight distribution
    // than the one currently in the policy.
    let weights_used = LayerWeights::new(0.8, 0.1, 0.1);
    let interaction_results = vec![
        ScoredResult {
            id: "doc_1".to_string(),
            content: "Xavier core logic".to_string(),
            score: 0.95,
            source: "working".to_string(),
            path: "src/main.rs".to_string(),
            updated_at: None,
            zone: None,
        }
    ];

    hormer.update_from_interaction(weights_used, &interaction_results, None).await;

    // 5. Verify Policy Update
    let updated_weights = gating.effective_weights().await;
    assert_ne!(updated_weights.working, initial_weights.working, "Working weight should change");
    assert!(updated_weights.is_valid());

    // 6. Second Retrieval (using updated policy)
    let results_after = gating.retrieve(&docs, &sessions, &entities, query, None).await;
    assert!(!results_after.is_empty());
    assert_eq!(results_after[0].id, "doc_1");
}

#[tokio::test]
async fn test_hormer_edge_empty_corpus() {
    let policy = Arc::new(RwLock::new(NavigationPolicy::default()));
    let gating = AdaptiveGating::with_policy(GatingConfig::default(), policy);

    let docs: Vec<MemoryDocument> = vec![];
    let sessions: Vec<SessionSummary> = vec![];
    let entities: Vec<EntityRecord> = vec![];

    let results = gating.retrieve(&docs, &sessions, &entities, "anything", None).await;
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_hormer_edge_single_document() {
    let policy = Arc::new(RwLock::new(NavigationPolicy::default()));
    let mut config = GatingConfig::default();
    config.relevance_threshold = 0.0;
    config.grounding_enabled = false;
    let gating = AdaptiveGating::with_policy(config, policy);

    let docs = vec![
        MemoryDocument {
            id: Some("only_one".to_string()),
            content: "Unique content".to_string(),
            ..Default::default()
        }
    ];

    let results = gating.retrieve(&docs, &[], &[], "Unique", None).await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "only_one");
}

#[tokio::test]
async fn test_hormer_edge_duplicate_documents() {
    let policy = Arc::new(RwLock::new(NavigationPolicy::default()));
    let mut config = GatingConfig::default();
    config.relevance_threshold = 0.0;
    config.grounding_enabled = false;
    let gating = AdaptiveGating::with_policy(config, policy);

    // Duplicate content and ID in same layer
    let docs = vec![
        MemoryDocument {
            id: Some("dup".to_string()),
            content: "Duplicate content".to_string(),
            ..Default::default()
        },
        MemoryDocument {
            id: Some("dup".to_string()),
            content: "Duplicate content".to_string(),
            ..Default::default()
        }
    ];

    let results = gating.retrieve(&docs, &[], &[], "Duplicate", None).await;

    // RRF should handle duplicates by merging them or keeping one depending on implementation.
    // Looking at rrf.rs, it typically aggregates scores.
    assert_eq!(results.len(), 1, "Should deduplicate by ID");
}

#[tokio::test]
async fn test_hormer_ranking_consistency() {
    let policy = Arc::new(RwLock::new(NavigationPolicy::default()));
    let mut config = GatingConfig::default();
    config.relevance_threshold = 0.0;
    let gating = AdaptiveGating::with_policy(config, policy);

    let docs = vec![
        MemoryDocument {
            id: Some("low".to_string()),
            content: "Something".to_string(), // Low relevance to "Xavier"
            ..Default::default()
        },
        MemoryDocument {
            id: Some("high".to_string()),
            content: "Xavier is here".to_string(),
            ..Default::default()
        }
    ];

    let results = gating.retrieve(&docs, &[], &[], "Xavier", None).await;
    assert_eq!(results[0].id, "high");
}
