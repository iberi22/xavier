use std::sync::Arc;
use tokio::sync::oneshot;
use xavier::retrieval::regeneration::{
    ContextRegenerator, ContextRegeneratorConfig, RegenerationResult,
};
use xavier::search::rrf::ScoredResult;

#[test]
fn test_rrf_weight_calculation_and_normalization() {
    let config = ContextRegeneratorConfig {
        min_hit_sample: 10,
        learning_rate: 0.2,
        ..Default::default()
    };
    let regenerator = ContextRegenerator::new(config);

    // Baseline prior to sample threshold
    let (kw, vw) = regenerator.calculate_rrf_weights(4, 1, 5, 0.5, 0.5);
    assert_eq!((kw, vw), (0.5, 0.5));

    // After reaching sample threshold with 80% keyword hits and 20% vector hits
    let (new_kw, new_vw) = regenerator.calculate_rrf_weights(80, 20, 100, 0.5, 0.5);
    assert!(
        (new_kw + new_vw - 1.0).abs() < 1e-5,
        "Sum of weights must be 1.0"
    );
    assert!(
        new_kw > new_vw,
        "Keyword weight ({new_kw}) should be higher than vector weight ({new_vw})"
    );
}

#[test]
fn test_top_k_score_adjustments() {
    let regenerator = ContextRegenerator::new(ContextRegeneratorConfig {
        target_top_k: 2,
        ..Default::default()
    });

    let mut results = vec![
        ScoredResult {
            id: "doc-keyword".to_string(),
            score: 0.5,
            source: "working_keyword".to_string(),
            path: "docs/1.md".to_string(),
            content: "Rust RAG".to_string(),
            updated_at: None,
            zone: None,
        },
        ScoredResult {
            id: "doc-vector".to_string(),
            score: 0.5,
            source: "semantic_vector".to_string(),
            path: "docs/2.md".to_string(),
            content: "Dense Search".to_string(),
            updated_at: None,
            zone: None,
        },
        ScoredResult {
            id: "doc-outside-top-k".to_string(),
            score: 0.5,
            source: "working_keyword".to_string(),
            path: "docs/3.md".to_string(),
            content: "Ignored".to_string(),
            updated_at: None,
            zone: None,
        },
    ];

    let avg_shift = regenerator.adjust_top_k_scores(&mut results, 0.7, 0.3);

    assert!(avg_shift > 0.0, "Average shift must be non-zero");
    // Keyword multiplier: 0.7 * 2.0 = 1.4 -> 0.5 * 1.4 = 0.7
    assert!((results[0].score - 0.7).abs() < 1e-4);
    // Vector multiplier: 0.3 * 2.0 = 0.6 -> 0.5 * 0.6 = 0.3
    assert!((results[1].score - 0.3).abs() < 1e-4);
    // Unchanged item outside top_k (target_top_k = 2)
    assert_eq!(results[2].score, 0.5);
}

#[tokio::test]
async fn test_spawn_blocking_similarity_recalculation() {
    let regenerator = ContextRegenerator::with_defaults();

    let candidates = vec![
        (vec![1.0, 0.0, 0.0], vec![1.0, 0.0, 0.0]), // Cosine similarity = 1.0
        (vec![1.0, 0.0, 0.0], vec![0.0, 1.0, 0.0]), // Cosine similarity = 0.0
    ];

    let scores = regenerator
        .recalculate_similarity_batch(candidates, 0.5, 0.5)
        .await
        .expect("spawn_blocking batch similarity failed");

    assert_eq!(scores.len(), 2);
    assert!(
        scores[0] > scores[1],
        "High similarity pair should score higher"
    );
}

#[tokio::test]
async fn test_full_regeneration_pass_and_convergence() {
    let config = ContextRegeneratorConfig {
        min_hit_sample: 5,
        learning_rate: 0.1,
        target_top_k: 5,
        convergence_threshold: 0.01,
        ..Default::default()
    };

    let regenerator = ContextRegenerator::new(config);

    for _ in 0..10 {
        regenerator.record_hit(true, false);
    }

    let candidates = vec![(vec![1.0, 0.0], vec![0.8, 0.2])];
    let top_k = vec![ScoredResult {
        id: "res-1".to_string(),
        score: 0.6,
        source: "working".to_string(),
        path: "working/1".to_string(),
        content: "content".to_string(),
        updated_at: None,
        zone: None,
    }];

    let result1: RegenerationResult = regenerator
        .run_regeneration_pass(candidates.clone(), top_k.clone())
        .await
        .expect("Pass 1 failed");

    assert!(
        !result1.converged,
        "Pass 1 should not converge after initial weight update"
    );
    assert!(result1.keyword_weight > result1.vector_weight);

    // Run passes until converged (or max 50 iterations)
    let mut converged = false;
    for _ in 0..50 {
        let res = regenerator
            .run_regeneration_pass(candidates.clone(), top_k.clone())
            .await
            .expect("Pass failed");
        if res.converged {
            converged = true;
            break;
        }
    }

    assert!(
        converged,
        "Passes should eventually converge as weights approach stationary value"
    );
}

#[tokio::test]
async fn test_background_scheduled_regeneration_loop() {
    let config = ContextRegeneratorConfig {
        interval_secs: 1,
        ..Default::default()
    };

    let regenerator = Arc::new(ContextRegenerator::new(config));
    let (tx, rx) = oneshot::channel();

    let handle = regenerator.clone().start_regeneration_loop(rx);

    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    tx.send(()).expect("Failed to send shutdown signal");
    handle.await.expect("Background loop join error");
}
