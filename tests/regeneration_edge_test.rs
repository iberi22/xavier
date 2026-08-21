//! Edge case tests and 100% coverage tests for Context Regeneration and RRF Tuner.
//!
//! Tests extreme outlier weights, vector sparsity, blocking thread starvation,
//! and full struct lifecycles.

use std::sync::Arc;
use tokio::sync::oneshot;
use xavier::retrieval::regeneration::{
    ContextRegenerator, ContextRegeneratorConfig, RegenerationResult,
};
use xavier::search::rrf::ScoredResult;

#[test]
fn test_config_serde_defaults_and_debug() {
    let default_config = ContextRegeneratorConfig::default();
    assert_eq!(default_config.interval_secs, 60);
    assert_eq!(default_config.learning_rate, 0.1);
    assert_eq!(default_config.target_top_k, 10);
    assert_eq!(default_config.convergence_threshold, 0.005);
    assert_eq!(default_config.min_hit_sample, 5);

    let cloned = default_config.clone();
    let debug_str = format!("{:?}", cloned);
    assert!(debug_str.contains("ContextRegeneratorConfig"));

    let json = serde_json::to_string(&default_config).expect("Config serialization failed");
    let deserialized: ContextRegeneratorConfig =
        serde_json::from_str(&json).expect("Config deserialization failed");
    assert_eq!(deserialized.interval_secs, default_config.interval_secs);
}

#[test]
fn test_regeneration_result_serde_and_debug() {
    let result = RegenerationResult {
        baseline_score: 0.75,
        regenerated_score: 0.85,
        keyword_weight: 0.6,
        vector_weight: 0.4,
        top_k_score_shift: 0.1,
        converged: true,
        candidates_processed: 42,
        duration_ms: 15,
    };

    let cloned = result.clone();
    let debug_str = format!("{:?}", cloned);
    assert!(debug_str.contains("RegenerationResult"));

    let json = serde_json::to_string(&result).expect("Result serialization failed");
    let deserialized: RegenerationResult =
        serde_json::from_str(&json).expect("Result deserialization failed");
    assert_eq!(deserialized.candidates_processed, 42);
    assert!(deserialized.converged);
}

#[test]
fn test_rrf_tuner_extreme_outlier_learning_rates() {
    let regenerator = ContextRegenerator::with_defaults();

    // Negative learning rate config
    let config_neg = ContextRegeneratorConfig {
        learning_rate: -10.0,
        min_hit_sample: 1,
        ..Default::default()
    };
    let regen_neg = ContextRegenerator::new(config_neg);
    let (kw1, vw1) = regen_neg.calculate_rrf_weights(10, 0, 10, 0.5, 0.5);
    assert!(
        kw1 > vw1,
        "Keyword weight should increase with 100% keyword hits"
    );
    assert!((kw1 + vw1 - 1.0).abs() < 1e-4);

    // High learning rate config (> 1.0)
    let config_high = ContextRegeneratorConfig {
        learning_rate: 50.0,
        min_hit_sample: 1,
        ..Default::default()
    };
    let regen_high = ContextRegenerator::new(config_high);
    let (kw2, vw2) = regen_high.calculate_rrf_weights(0, 10, 10, 0.5, 0.5);
    assert!(
        vw2 > kw2,
        "Vector weight should increase with 100% vector hits"
    );
    assert!((kw2 + vw2 - 1.0).abs() < 1e-4);

    // NaN learning rate
    let config_nan = ContextRegeneratorConfig {
        learning_rate: f32::NAN,
        min_hit_sample: 1,
        ..Default::default()
    };
    let regen_nan = ContextRegenerator::new(config_nan);
    let (kw3, vw3) = regen_nan.calculate_rrf_weights(5, 5, 10, 0.5, 0.5);
    assert!((kw3 + vw3 - 1.0).abs() < 1e-4);

    let _ = regenerator;
}

#[test]
fn test_rrf_tuner_extreme_outlier_current_weights() {
    let config = ContextRegeneratorConfig {
        min_hit_sample: 1,
        learning_rate: 0.1,
        ..Default::default()
    };
    let regenerator = ContextRegenerator::new(config);

    // Negative current weights
    let (kw1, vw1) = regenerator.calculate_rrf_weights(10, 10, 20, -5.0, -10.0);
    assert!(
        kw1 > 0.0 && vw1 > 0.0,
        "Weights must be positive after normalization"
    );
    assert!((kw1 + vw1 - 1.0).abs() < 1e-4);

    // NaN current weights
    let (kw2, vw2) = regenerator.calculate_rrf_weights(10, 10, 20, f32::NAN, f32::NAN);
    assert!(kw2 > 0.0 && vw2 > 0.0);
    assert!((kw2 + vw2 - 1.0).abs() < 1e-4);

    // Huge current weights
    let (kw3, vw3) = regenerator.calculate_rrf_weights(10, 10, 20, 1e10, 1e10);
    assert!((kw3 + vw3 - 1.0).abs() < 1e-4);
}

#[test]
fn test_rrf_tuner_zero_hits_and_zero_queries() {
    let config = ContextRegeneratorConfig {
        min_hit_sample: 5,
        ..Default::default()
    };
    let regenerator = ContextRegenerator::new(config);

    // Sample size reached but 0 hits total
    let (kw1, vw1) = regenerator.calculate_rrf_weights(0, 0, 10, 0.4, 0.6);
    assert_eq!(
        (kw1, vw1),
        (0.4, 0.6),
        "Zero total hits should preserve current weights"
    );

    // Total queries < min_hit_sample
    let (kw2, vw2) = regenerator.calculate_rrf_weights(1, 1, 2, 0.3, 0.7);
    assert_eq!(
        (kw2, vw2),
        (0.3, 0.7),
        "Insufficient queries should preserve current weights"
    );
}

#[tokio::test]
async fn test_extreme_vector_sparsity_similarity_recalculation() {
    let regenerator = ContextRegenerator::with_defaults();

    let candidates = vec![
        // Both zero vectors
        (vec![0.0, 0.0, 0.0], vec![0.0, 0.0, 0.0]),
        // Sparse orthogonal vectors
        (vec![0.0, 0.0, 1.0], vec![0.0, 1.0, 0.0]),
        // Negative direction vectors
        (vec![-1.0, -2.0, -3.0], vec![1.0, 2.0, 3.0]),
        // Huge floating point magnitude
        (vec![1e20, 1e20], vec![1e20, 1e20]),
        // Tiny floating point magnitude
        (vec![1e-20, 1e-20], vec![1e-20, 1e-20]),
        // Empty vectors
        (vec![], vec![]),
        // Mismatched lengths
        (vec![1.0, 2.0, 3.0], vec![1.0]),
    ];

    let scores = regenerator
        .recalculate_similarity_batch(candidates, 0.5, 0.5)
        .await
        .expect("Similarity batch failed on sparse vectors");

    assert_eq!(scores.len(), 7);
    for score in &scores {
        assert!(
            *score >= 0.0 && *score <= 1.0,
            "Score {score} must be clamped between 0.0 and 1.0"
        );
    }

    // Negative direction vector gives cos_sim = -1.0, so abs(cos_sim) = 1.0, vector_weight term is negative -> score clamped to 0.0
    assert_eq!(scores[2], 0.0);
}

#[tokio::test]
async fn test_similarity_recalculation_extreme_weights() {
    let regenerator = ContextRegenerator::with_defaults();
    let candidates = vec![(vec![1.0, 0.0], vec![1.0, 0.0])];

    // Negative / NaN weights passed to recalculate_similarity_batch
    let scores_neg = regenerator
        .recalculate_similarity_batch(candidates.clone(), -1.0, -5.0)
        .await
        .unwrap();
    assert_eq!(scores_neg.len(), 1);
    assert!(scores_neg[0] >= 0.0 && scores_neg[0] <= 1.0);

    let scores_nan = regenerator
        .recalculate_similarity_batch(candidates, f32::NAN, f32::NAN)
        .await
        .unwrap();
    assert_eq!(scores_nan.len(), 1);
    assert!(scores_nan[0] >= 0.0 && scores_nan[0] <= 1.0);
}

#[tokio::test]
async fn test_simulate_blocking_thread_pool_starvation() {
    let regenerator = Arc::new(ContextRegenerator::with_defaults());

    // Generate large batch of candidate vector pairs
    let candidate_pair = (
        (0..128).map(|i| i as f32 * 0.01).collect::<Vec<f32>>(),
        (0..128)
            .map(|i| (128 - i) as f32 * 0.01)
            .collect::<Vec<f32>>(),
    );
    let candidate_batch: Vec<(Vec<f32>, Vec<f32>)> = vec![candidate_pair; 50];

    // Dispatch 100 concurrent tasks calling recalculate_similarity_batch to simulate thread pool pressure
    let mut tasks = Vec::new();
    for _ in 0..100 {
        let reg = regenerator.clone();
        let batch = candidate_batch.clone();
        tasks.push(tokio::spawn(async move {
            reg.recalculate_similarity_batch(batch, 0.6, 0.4).await
        }));
    }

    for task in tasks {
        let res = task
            .await
            .expect("Task join error")
            .expect("Batch similarity error");
        assert_eq!(res.len(), 50);
    }
}

#[test]
fn test_adjust_top_k_scores_edge_cases() {
    let config = ContextRegeneratorConfig {
        target_top_k: 3,
        ..Default::default()
    };
    let regenerator = ContextRegenerator::new(config);

    // Empty results
    let mut empty_results: Vec<ScoredResult> = vec![];
    let shift_empty = regenerator.adjust_top_k_scores(&mut empty_results, 0.5, 0.5);
    assert_eq!(shift_empty, 0.0);

    // Target top k = 0
    let config_zero = ContextRegeneratorConfig {
        target_top_k: 0,
        ..Default::default()
    };
    let regen_zero = ContextRegenerator::new(config_zero);
    let mut res_zero = vec![ScoredResult {
        id: "1".to_string(),
        score: 0.5,
        source: "working".to_string(),
        path: "".to_string(),
        content: "".to_string(),
        updated_at: None,
        zone: None,
    }];
    assert_eq!(regen_zero.adjust_top_k_scores(&mut res_zero, 0.5, 0.5), 0.0);

    // Diverse source channels, NaN initial scores, and score clamping
    let mut results = vec![
        ScoredResult {
            id: "1".to_string(),
            score: 0.8,
            source: "working_keyword".to_string(),
            path: "".to_string(),
            content: "".to_string(),
            updated_at: None,
            zone: None,
        },
        ScoredResult {
            id: "2".to_string(),
            score: 0.8,
            source: "semantic_vector".to_string(),
            path: "".to_string(),
            content: "".to_string(),
            updated_at: None,
            zone: None,
        },
        ScoredResult {
            id: "3".to_string(),
            score: 0.8,
            source: "custom_hybrid".to_string(),
            path: "".to_string(),
            content: "".to_string(),
            updated_at: None,
            zone: None,
        },
        ScoredResult {
            id: "4".to_string(),
            score: f32::NAN,
            source: "working".to_string(),
            path: "".to_string(),
            content: "".to_string(),
            updated_at: None,
            zone: None,
        },
    ];

    let shift = regenerator.adjust_top_k_scores(&mut results, 0.8, 0.2);
    assert!(shift > 0.0);
    // Score clamped to 1.0 max (0.8 * 0.8 * 2.0 = 1.28 -> 1.0)
    assert_eq!(results[0].score, 1.0);
    // Semantic multiplier: 0.2 * 2.0 = 0.4 -> 0.8 * 0.4 = 0.32
    assert!((results[1].score - 0.32).abs() < 1e-4);
    // Custom source multiplier: 0.8 + 0.2 = 1.0 -> 0.8 * 1.0 = 0.8
    assert!((results[2].score - 0.8).abs() < 1e-4);
    // Fourth item untouched as target_top_k = 3
    assert!(results[3].score.is_nan());
}

#[tokio::test]
async fn test_full_regeneration_pass_edge_cases() {
    let regenerator = ContextRegenerator::with_defaults();

    // Empty candidates and empty top_k
    let res = regenerator
        .run_regeneration_pass(vec![], vec![])
        .await
        .expect("Pass with empty inputs failed");

    assert_eq!(res.baseline_score, 0.0);
    assert_eq!(res.regenerated_score, 0.0);
    assert_eq!(res.candidates_processed, 0);

    // Record hits in parallel to verify thread safety
    let reg = Arc::new(regenerator);
    let mut hit_handles = Vec::new();
    for i in 0..20 {
        let r = reg.clone();
        hit_handles.push(tokio::spawn(async move {
            r.record_hit(i % 2 == 0, i % 3 == 0);
        }));
    }
    for h in hit_handles {
        h.await.unwrap();
    }

    let (kw, vw) = reg.current_weights().await;
    assert!(kw > 0.0 && vw > 0.0);

    // Format ContextRegenerator debug
    let debug_str = format!("{:?}", reg);
    assert!(debug_str.contains("ContextRegenerator"));
}

#[tokio::test]
async fn test_background_loop_with_zero_interval() {
    let config = ContextRegeneratorConfig {
        interval_secs: 0, // Clamped to max(1) internally
        ..Default::default()
    };

    let regenerator = Arc::new(ContextRegenerator::new(config));
    let (tx, rx) = oneshot::channel();

    let handle = regenerator.start_regeneration_loop(rx);

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    tx.send(()).expect("Shutdown failed");
    handle.await.expect("Loop join failed");
}
