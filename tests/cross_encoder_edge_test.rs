//! Integration and edge case tests for ONNX Cross-Encoder Reranker.

use std::collections::HashMap;
use std::sync::Arc;

use xavier::context::hybrid::ContextSearchHit;
use xavier::context::reranker::{
    CrossEncoderConfig, CrossEncoderReranker, MockOnnxBackend,
};
use xavier::context::ContextDocument;

fn create_doc(id: &str, content: &str) -> ContextDocument {
    ContextDocument::new(id, "session-test", "user", content)
}

fn create_hit(id: &str, content: &str, score: f32) -> ContextSearchHit {
    ContextSearchHit {
        document: create_doc(id, content),
        score,
        sources: vec!["hybrid".to_string()],
    }
}

#[test]
fn edge_test_out_of_vocabulary_tokens() {
    let mut vocab = HashMap::new();
    vocab.insert("known".to_string(), 100);

    let backend = Arc::new(MockOnnxBackend::constant(0.5));
    let reranker = CrossEncoderReranker::new(CrossEncoderConfig::default(), backend, vocab);

    let query = "known unknown_word_12345 !!!special_chars!!!";
    let doc = "another_unseen_token known";

    let (input_ids, attention_mask) = reranker.tokenize_pair(query, doc);

    // [CLS] + "known"(100) + "unknown_word_12345"(UNK=1) + "specialchars"(UNK=1) + [SEP] + "anotherunseentoken"(UNK=1) + "known"(100) + [SEP]
    assert_eq!(input_ids[0], 2); // CLS
    assert_eq!(input_ids[1], 100); // known
    assert_eq!(input_ids[2], 1); // UNK
    assert_eq!(input_ids[3], 1); // UNK
    assert_eq!(input_ids[4], 3); // SEP
    assert_eq!(input_ids[5], 1); // UNK
    assert_eq!(input_ids[6], 100); // known
    assert_eq!(input_ids[7], 3); // SEP

    assert_eq!(input_ids.len(), attention_mask.len());
    assert!(attention_mask.iter().all(|&mask| mask == 1));
}

#[test]
fn edge_test_max_sequence_length_truncation() {
    let max_len = 16;
    let config = CrossEncoderConfig {
        max_sequence_length: max_len,
        ..Default::default()
    };
    let backend = Arc::new(MockOnnxBackend::constant(0.7));
    let reranker = CrossEncoderReranker::new(config, backend, HashMap::new());

    let long_query = (0..50).map(|i| format!("qword{i}")).collect::<Vec<_>>().join(" ");
    let long_doc = (0..50).map(|i| format!("dword{i}")).collect::<Vec<_>>().join(" ");

    let (input_ids, attention_mask) = reranker.tokenize_pair(&long_query, &long_doc);

    assert!(
        input_ids.len() <= max_len,
        "Tokenized length {} exceeded max sequence length {}",
        input_ids.len(),
        max_len
    );
    assert_eq!(input_ids.len(), attention_mask.len());
    assert_eq!(input_ids[0], 2); // CLS
    assert_eq!(*input_ids.last().unwrap(), 3); // SEP
}

#[test]
fn edge_test_nan_tensor_output_fallback() {
    let backend = Arc::new(MockOnnxBackend::nan());
    let reranker = CrossEncoderReranker::default_with_backend(backend);

    let mut hits = vec![
        create_hit("doc-a", "content a", 0.85),
        create_hit("doc-b", "content b", 0.42),
        create_hit("doc-c", "content c", 0.91),
    ];

    let results = reranker.rerank("search query", &mut hits);

    assert_eq!(results.len(), 3);
    for res in &results {
        assert!(res.is_fallback, "Expected fallback flag to be true for document {}", res.document_id);
    }

    // Since fallback preserves original scores, sorting places doc-c (0.91) first, doc-a (0.85) second, doc-b (0.42) third.
    assert_eq!(hits[0].document.id, "doc-c");
    assert_eq!(hits[0].score, 0.91);
    assert_eq!(hits[1].document.id, "doc-a");
    assert_eq!(hits[1].score, 0.85);
    assert_eq!(hits[2].document.id, "doc-b");
    assert_eq!(hits[2].score, 0.42);
}

#[test]
fn edge_test_infinity_tensor_output_fallback() {
    let backend = Arc::new(MockOnnxBackend::infinity());
    let reranker = CrossEncoderReranker::default_with_backend(backend);

    let mut hits = vec![create_hit("doc-inf", "content infinity", 0.5)];
    let results = reranker.rerank("query", &mut hits);

    assert_eq!(results.len(), 1);
    assert!(results[0].is_fallback);
    assert_eq!(hits[0].score, 0.5);
}

#[test]
fn edge_test_neg_infinity_tensor_output_fallback() {
    let backend = Arc::new(MockOnnxBackend::new(|_, _| Ok(f32::NEG_INFINITY)));
    let reranker = CrossEncoderReranker::default_with_backend(backend);

    let mut hits = vec![create_hit("doc-neginf", "content neg infinity", 0.33)];
    let results = reranker.rerank("query", &mut hits);

    assert_eq!(results.len(), 1);
    assert!(results[0].is_fallback);
    assert_eq!(hits[0].score, 0.33);
}

#[test]
fn edge_test_onnx_runtime_error_fallback() {
    let backend = Arc::new(MockOnnxBackend::failing("ONNX C++ Session failed: Model corrupted"));
    let reranker = CrossEncoderReranker::default_with_backend(backend);

    let mut hits = vec![create_hit("doc-err", "content err", 0.67)];
    let results = reranker.rerank("query", &mut hits);

    assert_eq!(results.len(), 1);
    assert!(results[0].is_fallback);
    assert_eq!(hits[0].score, 0.67);
}

#[test]
fn edge_test_empty_hits_graceful_handling() {
    let backend = Arc::new(MockOnnxBackend::constant(1.0));
    let reranker = CrossEncoderReranker::default_with_backend(backend);

    let mut empty_hits: Vec<ContextSearchHit> = Vec::new();
    let results = reranker.rerank("query", &mut empty_hits);

    assert!(results.is_empty());
    assert!(empty_hits.is_empty());
}
