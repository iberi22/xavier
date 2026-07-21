use std::sync::Arc;
use std::time::Duration;
use xavier::context::{
    regen_loop::{RegenDecision, RegenerationConfig, RegenerationLoop},
    ContextDocument, Orchestrator,
};

#[tokio::test]
async fn test_context_regen_loop_e2e_flow() {
    // 1. Setup custom RegenerationConfig
    let config = RegenerationConfig {
        stale_after_secs: 10,        // Stale after 10 seconds
        growth_ratio_threshold: 0.20, // 20% growth triggers
        min_growth_tokens: 50,       // Requires at least 50 new tokens
        cooldown_secs: 1,            // 1 second cooldown
        max_rebuilds_per_window: 5,  // Max 5 rebuilds
    };

    // 2. Initialize orchestrator and loop
    let orchestrator = Arc::new(Orchestrator::new());
    let loop_ = RegenerationLoop::with_config(config).with_orchestrator(orchestrator);
    let session_id = "test-session-e2e-regen";

    // 3. Check fresh session — should skip (no baseline yet)
    let decision = loop_.check(session_id, 30).await;
    assert_eq!(decision, RegenDecision::Skip);

    let stats = loop_.get_stats(session_id).await.unwrap();
    assert_eq!(stats.total_tokens_seen, 30);
    assert_eq!(stats.rebuild_count, 0);

    // 4. Perform a cycle with trigger_rebuild to establish a baseline
    // The query used by trigger_rebuild is "regenerate context" so let's include matching keywords
    let current_context = vec![
        ContextDocument::new("doc_1", session_id, "user", "Regenerate session context")
            .with_token_count(10),
        ContextDocument::new("doc_2", session_id, "assistant", "Context updated successfully")
            .with_token_count(20),
    ];

    // Trigger rebuild directly to set baseline in loop state
    let count = loop_
        .trigger_rebuild(session_id, &current_context)
        .await
        .expect("Trigger rebuild failed");

    assert!(count > 0, "Should select matching documents in rebuild");

    let stats = loop_.get_stats(session_id).await.unwrap();
    assert_eq!(stats.rebuild_count, 1, "Rebuild count should be incremented");
    assert_eq!(stats.tokens_at_last_rebuild, 30);

    // 5. Test Cooldown — check right after rebuild should skip even if there's massive growth
    let decision_cooldown = loop_.check(session_id, 100).await;
    assert_eq!(decision_cooldown, RegenDecision::Skip, "Should skip during cooldown");

    // Wait for cooldown to expire
    tokio::time::sleep(Duration::from_millis(1100)).await;

    // 6. Test Growth trigger — add 10 tokens (total 140, which is > 20% growth of 30, and total added is 110 which is > min_growth_tokens of 50)
    let decision_growth = loop_.check(session_id, 10).await;
    match decision_growth {
        RegenDecision::Growth { growth_ratio, tokens_added } => {
            assert!(growth_ratio >= 0.20);
            assert_eq!(tokens_added, 110);
        }
        other => panic!("Expected Growth decision, got {:?}", other),
    }

    // 7. Verify cycle convenience method triggers rebuild correctly
    let (cycle_decision, selected_count) = loop_.cycle(session_id, 0, &current_context).await;
    assert!(matches!(cycle_decision, RegenDecision::Growth { .. }));
    assert!(selected_count.unwrap_or(0) > 0);

    let final_stats = loop_.get_stats(session_id).await.unwrap();
    assert_eq!(final_stats.rebuild_count, 2);
    assert_eq!(final_stats.tokens_at_last_rebuild, 140);
}
