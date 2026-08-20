//! ONNX Cross-Encoder Reranker
//!
//! Provides cross-encoder reranking capabilities for context documents
//! with graceful fallback when ONNX inference fails, outputs NaN, or encounters
//! out-of-vocabulary tokens or oversized inputs.

use std::collections::HashMap;
use std::sync::Arc;

use crate::context::hybrid::ContextSearchHit;

/// Configuration for the cross-encoder model and tokenization.
#[derive(Debug, Clone)]
pub struct CrossEncoderConfig {
    pub max_sequence_length: usize,
    pub model_name: String,
    pub pad_token_id: u32,
    pub unk_token_id: u32,
    pub cls_token_id: u32,
    pub sep_token_id: u32,
    pub fallback_to_original_score: bool,
    pub default_fallback_score: f32,
}

impl Default for CrossEncoderConfig {
    fn default() -> Self {
        Self {
            max_sequence_length: 512,
            model_name: "cross-encoder/ms-marco-MiniLM-L-6-v2".to_string(),
            pad_token_id: 0,
            unk_token_id: 1,
            cls_token_id: 2,
            sep_token_id: 3,
            fallback_to_original_score: true,
            default_fallback_score: 0.0,
        }
    }
}

/// Result entry from cross-encoder evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct CrossEncoderResult {
    pub document_id: String,
    pub score: f32,
    pub is_fallback: bool,
}

/// Trait abstracting ONNX inference execution for testability and runtime isolation.
pub trait OnnxInferenceBackend: Send + Sync {
    /// Evaluates input token IDs and attention mask to produce a relevance score.
    fn evaluate(&self, input_ids: &[u32], attention_mask: &[u32]) -> Result<f32, String>;
}

/// Default mock ONNX backend used for baseline tests and safe defaults.
pub struct MockOnnxBackend {
    score_fn: Arc<dyn Fn(&[u32], &[u32]) -> Result<f32, String> + Send + Sync>,
}

impl MockOnnxBackend {
    /// Creates a mock backend with custom score logic.
    pub fn new<F>(score_fn: F) -> Self
    where
        F: Fn(&[u32], &[u32]) -> Result<f32, String> + Send + Sync + 'static,
    {
        Self {
            score_fn: Arc::new(score_fn),
        }
    }

    /// Creates a mock backend returning a constant score.
    pub fn constant(score: f32) -> Self {
        Self::new(move |_, _| Ok(score))
    }

    /// Creates a mock backend returning NaN score.
    pub fn nan() -> Self {
        Self::new(|_, _| Ok(f32::NAN))
    }

    /// Creates a mock backend returning Infinity score.
    pub fn infinity() -> Self {
        Self::new(|_, _| Ok(f32::INFINITY))
    }

    /// Creates a mock backend returning an error.
    pub fn failing(err: &str) -> Self {
        let err_str = err.to_string();
        Self::new(move |_, _| Err(err_str.clone()))
    }
}

impl OnnxInferenceBackend for MockOnnxBackend {
    fn evaluate(&self, input_ids: &[u32], attention_mask: &[u32]) -> Result<f32, String> {
        (self.score_fn)(input_ids, attention_mask)
    }
}

/// Cross-encoder reranker wrapping ONNX inference with robust error and edge-case handling.
pub struct CrossEncoderReranker {
    config: CrossEncoderConfig,
    backend: Arc<dyn OnnxInferenceBackend>,
    vocab: HashMap<String, u32>,
}

impl CrossEncoderReranker {
    /// Creates a new cross-encoder reranker with configuration, ONNX backend, and vocabulary.
    pub fn new(
        config: CrossEncoderConfig,
        backend: Arc<dyn OnnxInferenceBackend>,
        vocab: HashMap<String, u32>,
    ) -> Self {
        Self {
            config,
            backend,
            vocab,
        }
    }

    /// Creates a default reranker with a mock backend and sample vocabulary.
    pub fn default_with_backend(backend: Arc<dyn OnnxInferenceBackend>) -> Self {
        let mut vocab = HashMap::new();
        vocab.insert("hello".to_string(), 10);
        vocab.insert("world".to_string(), 11);
        vocab.insert("context".to_string(), 12);
        vocab.insert("retrieval".to_string(), 13);
        vocab.insert("search".to_string(), 14);

        Self::new(CrossEncoderConfig::default(), backend, vocab)
    }

    /// Returns reference to configuration.
    pub fn config(&self) -> &CrossEncoderConfig {
        &self.config
    }

    /// Tokenizes query and document text into input token IDs and attention mask,
    /// enforcing out-of-vocabulary handling and maximum sequence length limits.
    pub fn tokenize_pair(&self, query: &str, doc: &str) -> (Vec<u32>, Vec<u32>) {
        let query_tokens: Vec<u32> = query
            .split_whitespace()
            .map(|word| {
                let cleaned = word
                    .to_lowercase()
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_string();
                self.vocab
                    .get(&cleaned)
                    .copied()
                    .unwrap_or(self.config.unk_token_id)
            })
            .collect();

        let doc_tokens: Vec<u32> = doc
            .split_whitespace()
            .map(|word| {
                let cleaned = word
                    .to_lowercase()
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_string();
                self.vocab
                    .get(&cleaned)
                    .copied()
                    .unwrap_or(self.config.unk_token_id)
            })
            .collect();

        // [CLS] + query + [SEP] + doc + [SEP] = 3 special tokens minimum
        let available_budget = self.config.max_sequence_length.saturating_sub(3);

        let (truncated_q, truncated_d) = if query_tokens.len() + doc_tokens.len() > available_budget
        {
            let half = available_budget / 2;
            let q_len = query_tokens.len().min(half);
            let d_len = doc_tokens.len().min(available_budget - q_len);
            (&query_tokens[..q_len], &doc_tokens[..d_len])
        } else {
            (&query_tokens[..], &doc_tokens[..])
        };

        let mut input_ids = Vec::with_capacity(self.config.max_sequence_length);
        input_ids.push(self.config.cls_token_id);
        input_ids.extend_from_slice(truncated_q);
        input_ids.push(self.config.sep_token_id);
        input_ids.extend_from_slice(truncated_d);
        input_ids.push(self.config.sep_token_id);

        let attention_mask = vec![1u32; input_ids.len()];

        (input_ids, attention_mask)
    }

    /// Scores a single pair, handling model errors, NaNs, and infinities gracefully.
    pub fn score_pair(&self, query: &str, doc: &str, original_score: f32) -> (f32, bool) {
        let (input_ids, attention_mask) = self.tokenize_pair(query, doc);

        match self.backend.evaluate(&input_ids, &attention_mask) {
            Ok(score) if !score.is_nan() && !score.is_infinite() => (score, false),
            _ => {
                let fallback = if self.config.fallback_to_original_score {
                    original_score
                } else {
                    self.config.default_fallback_score
                };
                (fallback, true)
            }
        }
    }

    /// Reranks search hits in place and returns metadata results.
    pub fn rerank(
        &self,
        query: &str,
        hits: &mut [ContextSearchHit],
    ) -> Vec<CrossEncoderResult> {
        let mut results = Vec::with_capacity(hits.len());

        for hit in hits.iter_mut() {
            let (new_score, is_fallback) =
                self.score_pair(query, &hit.document.content, hit.score);
            hit.score = new_score;
            results.push(CrossEncoderResult {
                document_id: hit.document.id.clone(),
                score: new_score,
                is_fallback,
            });
        }

        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.document.id.cmp(&b.document.id))
        });

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextDocument;

    fn test_doc(id: &str, content: &str) -> ContextDocument {
        ContextDocument::new(id, "session-1", "user", content)
    }

    fn test_hit(id: &str, content: &str, score: f32) -> ContextSearchHit {
        ContextSearchHit {
            document: test_doc(id, content),
            score,
            sources: vec!["bm25".to_string()],
        }
    }

    #[test]
    fn test_out_of_vocabulary_mapping() {
        let backend = Arc::new(MockOnnxBackend::constant(0.8));
        let reranker = CrossEncoderReranker::default_with_backend(backend);

        // "unknownwordxyz" is out-of-vocabulary, should map to unk_token_id (1)
        let (ids, mask) = reranker.tokenize_pair("hello unknownwordxyz", "world");
        assert_eq!(ids[0], reranker.config().cls_token_id);
        assert_eq!(ids[1], 10); // "hello"
        assert_eq!(ids[2], reranker.config().unk_token_id); // "unknownwordxyz" mapped to UNK
        assert_eq!(mask.len(), ids.len());
    }

    #[test]
    fn test_max_sequence_truncation() {
        let config = CrossEncoderConfig {
            max_sequence_length: 10,
            ..Default::default()
        };
        let backend = Arc::new(MockOnnxBackend::constant(0.5));
        let reranker = CrossEncoderReranker::new(config, backend, HashMap::new());

        let query = "word1 word2 word3 word4 word5 word6";
        let doc = "doc1 doc2 doc3 doc4 doc5 doc6";

        let (ids, mask) = reranker.tokenize_pair(query, doc);
        assert!(ids.len() <= 10);
        assert_eq!(ids.len(), mask.len());
        assert_eq!(ids[0], reranker.config().cls_token_id);
    }

    #[test]
    fn test_nan_output_fallback() {
        let backend = Arc::new(MockOnnxBackend::nan());
        let reranker = CrossEncoderReranker::default_with_backend(backend);

        let mut hits = vec![
            test_hit("1", "hello world", 0.9),
            test_hit("2", "context search", 0.4),
        ];

        let results = reranker.rerank("hello", &mut hits);

        assert_eq!(results.len(), 2);
        assert!(results[0].is_fallback);
        assert!(results[1].is_fallback);
        // Original scores preserved
        assert_eq!(hits[0].document.id, "1");
        assert_eq!(hits[0].score, 0.9);
        assert_eq!(hits[1].document.id, "2");
        assert_eq!(hits[1].score, 0.4);
    }

    #[test]
    fn test_infinity_output_fallback() {
        let backend = Arc::new(MockOnnxBackend::infinity());
        let reranker = CrossEncoderReranker::default_with_backend(backend);

        let (score, is_fallback) = reranker.score_pair("hello", "world", 0.75);
        assert!(is_fallback);
        assert_eq!(score, 0.75);
    }

    #[test]
    fn test_backend_error_fallback() {
        let backend = Arc::new(MockOnnxBackend::failing("ONNX session error"));
        let reranker = CrossEncoderReranker::default_with_backend(backend);

        let (score, is_fallback) = reranker.score_pair("hello", "world", 0.65);
        assert!(is_fallback);
        assert_eq!(score, 0.65);
    }

    #[test]
    fn test_successful_reranking() {
        let backend = Arc::new(MockOnnxBackend::new(|ids, _| {
            // Give higher score if token 10 ("hello") is present
            if ids.contains(&10) {
                Ok(0.95)
            } else {
                Ok(0.10)
            }
        }));
        let reranker = CrossEncoderReranker::default_with_backend(backend);

        let mut hits = vec![
            test_hit("1", "context search", 0.8),
            test_hit("2", "hello world", 0.2),
        ];

        let results = reranker.rerank("query", &mut hits);

        assert_eq!(results.len(), 2);
        assert!(!results[0].is_fallback);
        assert!(!results[1].is_fallback);
        // "hello world" should now be ranked first due to score 0.95
        assert_eq!(hits[0].document.id, "2");
        assert_eq!(hits[0].score, 0.95);
        assert_eq!(hits[1].document.id, "1");
        assert_eq!(hits[1].score, 0.10);
    }

    #[test]
    fn test_fallback_to_default_fallback_score() {
        let config = CrossEncoderConfig {
            fallback_to_original_score: false,
            default_fallback_score: -1.0,
            ..Default::default()
        };
        let backend = Arc::new(MockOnnxBackend::nan());
        let reranker = CrossEncoderReranker::new(config, backend, HashMap::new());

        let (score, is_fallback) = reranker.score_pair("hello", "world", 0.99);
        assert!(is_fallback);
        assert_eq!(score, -1.0);
    }
}
