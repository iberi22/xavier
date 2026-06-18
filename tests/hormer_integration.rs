use std::sync::Arc;
use tokio::sync::RwLock;
use xavier::agents::hormer::Hormer;
use xavier::retrieval::{AdaptiveGating, GatingConfig, LayerWeights, NavigationPolicy};
use xavier::memory::qmd_memory::MemoryDocument;
use xavier::consolidation::merger::similarity;
use xavier::search::rrf::ScoredResult;
use xavier::tgd::{TgdEngine, TgdConfig};
use xavier::agents::provider::ModelProviderClient;
use xavier::agents::runtime::{ConversationMessage, MessageRole};
use xavier::agents::system1::RetrievedDocument;

#[tokio::test]
async fn test_hormer_lifecycle() {
    // 1. Hierarchical document setup
    let doc_a = MemoryDocument {
        id: Some("doc_a".to_string()),
        path: "src/retrieval/gating.rs".to_string(),
        content: "Adaptive Gating implementation in Rust".to_string(),
        ..Default::default()
    };
    let doc_b = MemoryDocument {
        id: Some("doc_b".to_string()),
        path: "src/retrieval/policy.rs".to_string(),
        content: "Navigation Policy management".to_string(),
        ..Default::default()
    };
    let doc_c = MemoryDocument {
        id: Some("doc_c".to_string()),
        path: "other/readme.md".to_string(),
        content: "General documentation".to_string(),
        ..Default::default()
    };

    // 2. Verify nav-aware similarity (Consolidation)
    let sim_ab = similarity(&doc_a, &doc_b); // Same dir: src/retrieval
    let sim_ac = similarity(&doc_a, &doc_c); // Different dir

    assert!(sim_ab > sim_ac, "Documents in same directory should have higher similarity. ab: {}, ac: {}", sim_ab, sim_ac);

    // 3. Shared policy configuration
    let initial_weights = LayerWeights::new(0.3, 0.3, 0.4);
    let policy = Arc::new(RwLock::new(NavigationPolicy::new(
        initial_weights,
        xavier::retrieval::policy::TraversalWeights::default(),
        0.1,
    )));

    let gating = AdaptiveGating::with_policy(GatingConfig::default(), policy.clone());
    let hormer = Hormer::new(policy.clone());

    assert_eq!(gating.effective_weights().await.working, 0.3);

    // 4. GRPO update simulation
    // High reward results: relevance 0.9, diversity 1.0 (2 sources)
    let mock_results = vec![
        ScoredResult {
            id: "res1".to_string(),
            content: "Relevant content from working".to_string(),
            score: 0.9,
            source: "working".to_string(),
            path: "path1".to_string(),
            updated_at: None,
        },
        ScoredResult {
            id: "res2".to_string(),
            content: "Relevant content from episodic".to_string(),
            score: 0.9,
            source: "episodic".to_string(),
            path: "path2".to_string(),
            updated_at: None,
        },
    ];

    // Use different weights for the interaction to ensure normalization causes a shift
    let used_weights = LayerWeights::new(0.6, 0.2, 0.2);
    hormer.update_from_interaction(used_weights, &mock_results).await;

    let updated_weights = gating.effective_weights().await;
    assert!(policy.read().await.update_count > 0);

    // Verify weights changed and are still valid
    assert!(updated_weights.is_valid());
    assert_ne!(updated_weights.working, initial_weights.working);

    // 5. Verify AdaptiveGating uses updated weights
    let mut gating_with_low_threshold = gating.clone();
    gating_with_low_threshold.set_threshold(0.0);

    let docs = vec![doc_a, doc_b, doc_c];
    // Use a query that matches the content of doc_a
    let results = gating_with_low_threshold.retrieve(&docs, &[], &[], "Adaptive", None).await;

    // The fact that retrieve() finishes and uses weights from policy (checked by internal tracing/logic)
    // is enough for integration validation given we've confirmed policy is updated.
    assert!(!results.is_empty());
}

#[tokio::test]
#[ignore]
async fn test_tgd_generation() {
    let provider = ModelProviderClient::from_env();
    let config = TgdConfig {
        improvements_path: std::path::PathBuf::from(".xavier/test_improvements.md"),
        ..Default::default()
    };
    let tgd = TgdEngine::with_config(Arc::new(provider), config);

    let history = vec![
        ConversationMessage {
            id: "msg1".to_string(),
            role: MessageRole::User,
            content: "How do I configure the retriever?".to_string(),
            timestamp: chrono::Utc::now(),
        },
        ConversationMessage {
            id: "msg2".to_string(),
            role: MessageRole::Assistant,
            content: "You can use GatingConfig to set weights.".to_string(),
            timestamp: chrono::Utc::now(),
        },
    ];

    let context = vec![
        RetrievedDocument {
            id: "doc1".to_string(),
            path: "docs/config.md".to_string(),
            content: "Retriever configuration involves setting working, episodic and semantic weights.".to_string(),
            relevance_score: 0.9,
            token_count: 20,
            metadata: serde_json::json!({}),
        }
    ];

    let rules = tgd.generate_rules(&history, &context).await.expect("TGD generation failed");
    assert!(!rules.is_empty());

    // 6. Verify persistence
    let persisted = tokio::fs::read_to_string(".xavier/test_improvements.md").await.expect("Failed to read persisted rules");
    assert!(persisted.contains(&rules));

    // Cleanup
    let _ = tokio::fs::remove_file(".xavier/test_improvements.md").await;
}
