//! Unit and integration tests for local ONNX cross-encoder re-ranking pipeline in retrieval.

use std::path::PathBuf;
use std::sync::Arc;
use tempfile::NamedTempFile;

use xavier::retrieval::cross_encoder::{
    CrossEncoderConfig, CrossEncoderError, CrossEncoderReranker, RerankCandidate, TokenMetrics,
};
use xavier::search::rrf::ScoredResult;

fn make_candidate(id: &str, content: &str, base_score: f32) -> RerankCandidate {
    RerankCandidate::new(id, content, base_score)
}

fn make_scored_result(id: &str, content: &str, score: f32) -> ScoredResult {
    ScoredResult {
        id: id.to_string(),
        content: content.to_string(),
        score,
        source: "hybrid".to_string(),
        path: format!("doc/{}", id),
        updated_at: None,
        zone: None,
    }
}

#[test]
fn test_1_missing_model_fallback_to_bm25() {
    let reranker = CrossEncoderReranker::new("non_existent_model.onnx");
    assert!(!reranker.is_onnx_available());

    let candidates = vec![
        make_candidate("1", "rust language async runtime tokio", 0.3),
        make_candidate("2", "python data science pandas numpy", 0.4),
    ];

    let results = reranker
        .rerank_candidates("rust async", &candidates)
        .unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].id, "1");
    assert_eq!(results[0].source, "bm25_fallback");
}

#[test]
fn test_2_onnx_model_loading_from_bytes() {
    let mock_bytes = b"ONNX_MOCK_MODEL_BYTES_PAYLOAD_TEST";
    let reranker = CrossEncoderReranker::from_bytes(mock_bytes, "mock.onnx").unwrap();

    assert!(reranker.is_onnx_available());

    let candidates = vec![
        make_candidate("1", "onnx cross encoder re-ranking pipeline", 0.2),
        make_candidate("2", "completely unrelated text input candidate", 0.8),
    ];

    let results = reranker
        .rerank_candidates("onnx cross encoder", &candidates)
        .unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].id, "1");
    assert_eq!(results[0].source, "onnx_cross_encoder");
}

#[test]
fn test_3_empty_candidates_or_query_returns_empty() {
    let reranker = CrossEncoderReranker::new("missing.onnx");
    let results = reranker.rerank_candidates("query", &[]).unwrap();
    assert!(results.is_empty());

    let candidates = vec![make_candidate("1", "some text", 0.5)];
    let results = reranker.rerank_candidates("   ", &candidates).unwrap();
    assert!(results.is_empty());
}

#[test]
fn test_4_relevance_sorting_order_and_score_adjustment() {
    let mock_bytes = b"ONNX_MODEL_HEADER_TEST_BYTES";
    let reranker = CrossEncoderReranker::builder()
        .cross_encoder_weight(0.8)
        .build();
    let reranker_with_session = CrossEncoderReranker::from_bytes(mock_bytes, "test.onnx").unwrap();

    let candidates = vec![
        make_candidate("1", "java enterprise application spring", 0.8),
        make_candidate("2", "rust high performance cross encoder reranker", 0.2),
        make_candidate("3", "python automation script", 0.5),
    ];

    let results = reranker_with_session
        .rerank_candidates("cross encoder rust", &candidates)
        .unwrap();
    assert_eq!(results.len(), 3);
    assert_eq!(results[0].id, "2");
    assert!(results[0].score >= results[1].score);
    assert!(results[1].score >= results[2].score);

    // Verify score adjustments with weight=0.8: final = 0.2 * 0.3 + 0.8 * 0.9 = 0.78
    let (adj_score, delta) = reranker.calculate_score_adjustment(0.3, 0.9);
    assert!((adj_score - 0.78).abs() < 1e-4);
    assert!((delta - 0.48).abs() < 1e-4);
}

#[test]
fn test_5_batch_processing_and_invalid_batch_size() {
    let mock_bytes = b"ONNX_BATCH_TEST_BYTES_PAYLOAD";
    let reranker = CrossEncoderReranker::from_bytes(mock_bytes, "batch.onnx").unwrap();

    let candidates: Vec<_> = (0..10)
        .map(|i| {
            make_candidate(
                &format!("{i}"),
                &format!("candidate document item {i}"),
                0.1 * (i as f32),
            )
        })
        .collect();

    let res_b1 = reranker
        .rerank_candidates_batch("candidate item", &candidates, 1)
        .unwrap();
    let res_b4 = reranker
        .rerank_candidates_batch("candidate item", &candidates, 4)
        .unwrap();

    assert_eq!(res_b1.len(), 10);
    assert_eq!(res_b4.len(), 10);

    let err = reranker
        .rerank_candidates_batch("query", &candidates, 0)
        .unwrap_err();
    assert_eq!(err, CrossEncoderError::InvalidBatchSize(0));
}

#[test]
fn test_6_rerank_scored_results_in_place() {
    let mock_bytes = b"ONNX_SCORED_RESULTS_TEST_BYTES";
    let reranker = CrossEncoderReranker::from_bytes(mock_bytes, "scored.onnx").unwrap();

    let scored = vec![
        make_scored_result("doc1", "unrelated text content", 0.5),
        make_scored_result("doc2", "high performance cross encoder candidate", 0.2),
    ];

    let updated = reranker
        .rerank_scored_results("cross encoder candidate", &scored)
        .unwrap();
    assert_eq!(updated.len(), 2);
    assert_eq!(updated[0].id, "doc2");
    assert!(updated[0].score > updated[1].score);
}

#[test]
fn test_7_token_metrics_and_builder_config() {
    let metrics = TokenMetrics::compute("cross encoder", "fast cross encoder reranking");
    assert_eq!(metrics.query_token_count, 2);
    assert_eq!(metrics.matching_tokens, 2);
    assert!((metrics.overlap_ratio - 1.0).abs() < 1e-5);

    let temp_file = NamedTempFile::new().unwrap();
    std::fs::write(temp_file.path(), b"ONNX_FILE_BYTES_TEST").unwrap();

    let reranker = CrossEncoderReranker::builder()
        .model_path(temp_file.path())
        .batch_size(32)
        .max_seq_length(256)
        .cross_encoder_weight(0.5)
        .enable_fallback(true)
        .build();

    assert!(reranker.is_onnx_available());
    assert_eq!(reranker.config().batch_size, 32);
    assert_eq!(reranker.config().max_seq_length, 256);
    assert!((reranker.config().cross_encoder_weight - 0.5).abs() < 1e-5);
}

#[test]
fn test_8_disable_fallback_errors_when_no_model() {
    let config = CrossEncoderConfig {
        model_path: Some(PathBuf::from("non_existent_file.onnx")),
        enable_fallback: false,
        ..Default::default()
    };
    let reranker = CrossEncoderReranker::with_config(config);
    let candidates = vec![make_candidate("1", "content", 0.5)];

    let err = reranker
        .rerank_candidates("query", &candidates)
        .unwrap_err();
    assert!(matches!(err, CrossEncoderError::ModelLoadError(_)));
}

#[test]
fn test_9_multi_threaded_concurrent_reranker_calls() {
    let mock_bytes = b"ONNX_CONCURRENT_TEST_PAYLOAD";
    let reranker =
        Arc::new(CrossEncoderReranker::from_bytes(mock_bytes, "concurrent.onnx").unwrap());

    let mut handles = Vec::new();
    for t in 0..4 {
        let r = Arc::clone(&reranker);
        handles.push(std::thread::spawn(move || {
            let candidates = vec![
                make_candidate("1", &format!("thread {t} item one candidate"), 0.2),
                make_candidate("2", &format!("unrelated content thread {t}"), 0.8),
            ];
            r.rerank_candidates(&format!("thread {t} candidate"), &candidates)
        }));
    }

    for handle in handles {
        let res = handle.join().unwrap().unwrap();
        assert_eq!(res.len(), 2);
    }
}
