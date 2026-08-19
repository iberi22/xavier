//! Integration tests for the complete Context Regeneration Pipeline
//!
//! Verifies:
//! - Auto-regeneration triggers based on message volume & staleness
//! - Recall@K and MRR evaluation harness
//! - Parameter auto-tuning loop to achieve 100% target recall
//! - Extractive episodic dialogue summarization (masticación)

use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use xavier::context::{ContextDocument, ContextRegenerationPipeline, Orchestrator, RegenDecision};

#[tokio::test]
async fn test_full_context_regeneration_pipeline_workflow() {
    // 1. Setup the pipeline
    let orchestrator = Arc::new(Orchestrator::default());
    let pipeline = ContextRegenerationPipeline::new(orchestrator, 5, 0.8);
    let session_id = "long-session-123";

    // Initialize budgets to extremely low limits to force a low initial recall
    // and guarantee that the auto-tuning loop has to run and increment them.
    {
        let mut b = pipeline.budgets.write().await;
        b.precompact_min_docs = 1;
        b.precompact_med_docs = 1;
        b.precompact_max_docs = 1;
        b.session_start_min_docs = 1;
        b.session_start_med_docs = 1;
        b.session_start_max_docs = 1;
    }

    // 2. Define realistic long-session conversational messages
    let doc1 = ContextDocument::new(
        "doc-1",
        session_id,
        "user",
        "What is the recommended design pattern for xavier database locks?",
    );
    let doc2 = ContextDocument::new(
        "doc-2",
        session_id,
        "assistant",
        "Decision: Use a lightweight RwLock around internal state maps to prevent deadlocks under heavy concurrent queries.",
    );
    let doc3 = ContextDocument::new(
        "doc-3",
        session_id,
        "user",
        "Why do we clamp the negative bases in entity decay?",
    );
    let doc4 = ContextDocument::new(
        "doc-4",
        session_id,
        "assistant",
        "Incident: A crash occurred during hourly memory decay calculations due to mathematical instability.",
    );
    let doc5 = ContextDocument::new(
        "doc-5",
        session_id,
        "user",
        "Can you outline the fix for the decay crash?",
    );
    let doc6 = ContextDocument::new(
        "doc-6",
        session_id,
        "assistant",
        "Decision: We resolved the decay crash by clamping the factor strictly to the range 0.0 to 1.0.",
    );

    // 3. Process first two messages
    let d_init1 = pipeline.process_message(session_id, doc1).await.unwrap();
    assert_eq!(d_init1, RegenDecision::Skip);
    let d_init2 = pipeline.process_message(session_id, doc2).await.unwrap();
    assert_eq!(d_init2, RegenDecision::Skip);

    // Manually trigger initial rebuild to establish baseline (tokens_at_last_rebuild > 0)
    let _ = pipeline.regenerate_context(session_id).await.unwrap();

    // Sleep for 1.2 seconds to bypass the 1-second cooldown
    sleep(Duration::from_millis(1200)).await;

    // Process doc3, doc4, doc5, doc6 - growth trigger should be met
    let mut growth_triggered = false;
    for doc in vec![doc3, doc4, doc5, doc6] {
        let decision = pipeline.process_message(session_id, doc).await.unwrap();
        match decision {
            RegenDecision::Growth { .. } | RegenDecision::Stale { .. } => {
                growth_triggered = true;
            }
            _ => {}
        }
    }

    assert!(
        growth_triggered,
        "The pipeline should auto-trigger context regeneration after cooldown and growth threshold are met"
    );

    // 4. Force a rebuild and retrieve final constructed context
    let final_ctx = pipeline.regenerate_context(session_id).await.unwrap();
    assert!(final_ctx.contains("# System Prompt"));
    assert!(final_ctx.contains("RwLock"));
    assert!(final_ctx.contains("decay"));

    // 5. Setup a ground truth map with matching keywords to verify Recall@K and MRR
    let mut ground_truth = HashMap::new();
    ground_truth.insert(
        "design pattern RwLock".to_string(),
        vec!["doc-1".to_string(), "doc-2".to_string()],
    );
    ground_truth.insert(
        "decay crash clamping".to_string(),
        vec!["doc-4".to_string(), "doc-6".to_string()],
    );

    let queries = vec![
        "design pattern RwLock".to_string(),
        "decay crash clamping".to_string(),
    ];

    // Evaluate initial metrics. Since budget is 1, only 1 correct document can be selected per query.
    // Thus the average recall should be exactly 0.5.
    let initial_metrics = pipeline
        .evaluate_recall(session_id, &queries, &ground_truth, 2)
        .await;
    assert_eq!(initial_metrics.total_queries, 2);
    assert_eq!(initial_metrics.recall_at_k, 0.5);

    // 6. Test the Auto-tuning optimization loop
    // Set a high target recall and let the optimizer adjust document limits
    let target_recall = 1.0;
    let tuning_success = pipeline
        .auto_tune(session_id, &queries, &ground_truth, target_recall)
        .await
        .unwrap();

    assert!(
        tuning_success,
        "The auto-tune loop should optimize budgets to reach 100% target recall"
    );

    // Verify budgets were updated and incremented
    let budgets = pipeline.budgets.read().await;
    assert!(budgets.precompact_min_docs > 1);

    // Re-evaluate to verify perfect recall
    let final_metrics = pipeline
        .evaluate_recall(session_id, &queries, &ground_truth, 5)
        .await;
    assert_eq!(final_metrics.recall_at_k, 1.0);
    assert_eq!(final_metrics.mrr, 1.0);

    // 7. Verify extractive episodic dialogue summarizer (ctx-episodic-real)
    let docs_all = vec![
        ContextDocument::new("1", "s-1", "user", "How to fix the memory leak?"),
        ContextDocument::new(
            "2",
            "s-1",
            "assistant",
            "Decision: We will use a bounded FIFO cache to avoid memory leak.",
        ),
    ];
    let summary = pipeline.summarize_episodic(&docs_all);
    assert!(summary.contains("Extractive Dialogue Episodic Summary"));
    assert!(summary.contains("Decision:"));
    assert!(summary.contains("How to fix"));
}
