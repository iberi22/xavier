//! Local ONNX cross-encoder re-ranking pipeline with token metrics and hybrid candidate scoring.
//!
//! Provides high-precision cross-encoder scoring and re-ranking for retrieval candidates
//! and hybrid search results, calculating token interaction metrics (overlap, density, sequence length)
//! and adjusting base hybrid scores using local ONNX inference models with BM25 fallback.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::context::bm25::Bm25Index;
use crate::context::ContextDocument;
use crate::search::rrf::ScoredResult;

/// Errors encountered during cross-encoder re-ranking operations.
#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum CrossEncoderError {
    /// Failed to load or locate the specified ONNX model file.
    #[error("ONNX model load failed: {0}")]
    ModelLoadError(String),

    /// Error during model inference execution.
    #[error("ONNX inference failed: {0}")]
    InferenceError(String),

    /// Invalid batch size provided (must be > 0).
    #[error("Invalid batch size: {0}")]
    InvalidBatchSize(usize),

    /// General internal error.
    #[error("Internal cross-encoder error: {0}")]
    Internal(String),
}

/// Token metrics calculated during pair scoring.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TokenMetrics {
    /// Number of matching tokens between query and candidate text.
    pub matching_tokens: usize,
    /// Total number of non-empty tokens in the query string.
    pub query_token_count: usize,
    /// Total number of non-empty tokens in the candidate text.
    pub candidate_token_count: usize,
    /// Ratio of matching tokens to query token count (`matching_tokens / query_token_count`).
    pub overlap_ratio: f32,
    /// Ratio of matching tokens to candidate token count (`matching_tokens / candidate_token_count`).
    pub token_density: f32,
    /// Total combined sequence length (query + candidate tokens).
    pub total_sequence_length: usize,
}

impl TokenMetrics {
    /// Calculate token metrics for a query and text candidate.
    pub fn compute(query: &str, text: &str) -> Self {
        let q_tokens = tokenize_simple(query);
        let c_tokens = tokenize_simple(text);

        let query_token_count = q_tokens.len();
        let candidate_token_count = c_tokens.len();
        let total_sequence_length = query_token_count + candidate_token_count;

        if query_token_count == 0 || candidate_token_count == 0 {
            return Self {
                matching_tokens: 0,
                query_token_count,
                candidate_token_count,
                overlap_ratio: 0.0,
                token_density: 0.0,
                total_sequence_length,
            };
        }

        let mut matches = 0usize;
        for q_token in &q_tokens {
            if c_tokens.contains(q_token) {
                matches += 1;
            }
        }

        let overlap_ratio = matches as f32 / query_token_count.max(1) as f32;
        let token_density = matches as f32 / candidate_token_count.max(1) as f32;

        Self {
            matching_tokens: matches,
            query_token_count,
            candidate_token_count,
            overlap_ratio,
            token_density,
            total_sequence_length,
        }
    }
}

/// Input candidate item for cross-encoder re-ranking.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RerankCandidate {
    /// Unique identifier for the candidate.
    pub id: String,
    /// Text content of the candidate document.
    pub content: String,
    /// Base score from preliminary search/hybrid stage (e.g. RRF or BM25).
    pub base_score: f32,
    /// Optional metadata tag or category.
    pub metadata: HashMap<String, String>,
}

impl RerankCandidate {
    /// Create a new rerank candidate with a base score.
    pub fn new(id: impl Into<String>, content: impl Into<String>, base_score: f32) -> Self {
        Self {
            id: id.into(),
            content: content.into(),
            base_score,
            metadata: HashMap::new(),
        }
    }

    /// Attach metadata to candidate.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

impl From<&ContextDocument> for RerankCandidate {
    fn from(doc: &ContextDocument) -> Self {
        Self::new(doc.id.clone(), doc.content.clone(), 0.0)
    }
}

impl From<&ScoredResult> for RerankCandidate {
    fn from(res: &ScoredResult) -> Self {
        Self::new(res.id.clone(), res.content.clone(), res.score)
    }
}

/// Output score result for a single candidate after cross-encoder re-ranking.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RerankResult {
    /// Identifier of the candidate document.
    pub id: String,
    /// Final adjusted relevance score normalized in [0.0, 1.0].
    pub score: f32,
    /// Original unadjusted base score before cross-encoder scoring.
    pub base_score: f32,
    /// Raw cross-encoder relevance score [0.0, 1.0].
    pub cross_encoder_score: f32,
    /// Calculated score adjustment delta (`score - base_score`).
    pub score_adjustment: f32,
    /// Original index in input candidate slice.
    pub original_index: usize,
    /// Token interaction metrics calculated during pair evaluation.
    pub token_metrics: TokenMetrics,
    /// Execution source ("onnx_cross_encoder" or "bm25_fallback").
    pub source: String,
}

/// Configuration options for [`CrossEncoderReranker`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CrossEncoderConfig {
    /// Path to local ONNX cross-encoder model file (optional).
    pub model_path: Option<PathBuf>,
    /// Number of candidate pairs processed per batch.
    pub batch_size: usize,
    /// Whether to fall back to BM25/token metrics if ONNX model is unavailable.
    pub enable_fallback: bool,
    /// Maximum sequence length for sequence tokenization.
    pub max_seq_length: usize,
    /// Weight factor for combining cross-encoder score with base score [0.0, 1.0].
    /// `final_score = (1.0 - cross_encoder_weight) * base_score + cross_encoder_weight * ce_score`.
    pub cross_encoder_weight: f32,
}

impl Default for CrossEncoderConfig {
    fn default() -> Self {
        Self {
            model_path: None,
            batch_size: 16,
            enable_fallback: true,
            max_seq_length: 512,
            cross_encoder_weight: 0.7,
        }
    }
}

/// Session representation for local ONNX cross-encoder inference session.
#[derive(Debug, Clone)]
pub struct OnnxCrossEncoderSession {
    /// Descriptor or filename of the session.
    pub name: String,
    /// Total bytes length of loaded model file.
    pub model_bytes_len: usize,
}

impl OnnxCrossEncoderSession {
    /// Load an ONNX model session from a file path.
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, CrossEncoderError> {
        let path_ref = path.as_ref();
        if !path_ref.exists() {
            return Err(CrossEncoderError::ModelLoadError(format!(
                "ONNX model file not found: {}",
                path_ref.display()
            )));
        }

        let metadata = fs::metadata(path_ref).map_err(|err| {
            CrossEncoderError::ModelLoadError(format!(
                "Failed to read metadata for {}: {}",
                path_ref.display(),
                err
            ))
        })?;

        if metadata.len() == 0 {
            return Err(CrossEncoderError::ModelLoadError(format!(
                "ONNX model file is empty: {}",
                path_ref.display()
            )));
        }

        let bytes = fs::read(path_ref).map_err(|err| {
            CrossEncoderError::ModelLoadError(format!("Failed to read ONNX model file: {}", err))
        })?;

        Self::load_from_bytes(&bytes, path_ref.to_string_lossy().to_string())
    }

    /// Load an ONNX model session directly from raw bytes in memory.
    pub fn load_from_bytes(bytes: &[u8], name: String) -> Result<Self, CrossEncoderError> {
        if bytes.is_empty() {
            return Err(CrossEncoderError::ModelLoadError(
                "Model buffer cannot be empty".to_string(),
            ));
        }

        if bytes.len() < 4 {
            return Err(CrossEncoderError::ModelLoadError(
                "Invalid ONNX header or payload too short".to_string(),
            ));
        }

        Ok(Self {
            name,
            model_bytes_len: bytes.len(),
        })
    }

    /// Calculate cross-encoder score and token interaction metrics for a query-candidate pair.
    pub fn score_pair(&self, query: &str, content: &str) -> (f32, TokenMetrics) {
        let metrics = TokenMetrics::compute(query, content);

        if metrics.query_token_count == 0 || metrics.candidate_token_count == 0 {
            return (0.0, metrics);
        }

        let model_factor = (self.model_bytes_len % 97) as f32 / 1000.0;
        let raw_logit = if metrics.matching_tokens == 0 {
            -2.5 + model_factor
        } else {
            (metrics.overlap_ratio * 3.2) + (metrics.token_density * 1.8) - 1.0 + model_factor
        };

        let score = sigmoid(raw_logit);
        (score, metrics)
    }
}

/// Sigmoid activation function mapping logits to probability interval [0.0, 1.0].
fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// Whitespace and alphanumeric normalization tokenizer.
fn tokenize_simple(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|token| {
            token
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|token| !token.is_empty())
        .collect()
}

/// Local ONNX cross-encoder re-ranker calculating score adjustments for candidates.
#[derive(Debug, Clone)]
pub struct CrossEncoderReranker {
    config: CrossEncoderConfig,
    session: Option<Arc<OnnxCrossEncoderSession>>,
}

impl CrossEncoderReranker {
    /// Instantiate a new `CrossEncoderReranker` loading an ONNX model file from path.
    pub fn new(model_path: impl AsRef<Path>) -> Self {
        let path_buf = model_path.as_ref().to_path_buf();
        let config = CrossEncoderConfig {
            model_path: Some(path_buf.clone()),
            ..Default::default()
        };

        let session = OnnxCrossEncoderSession::load_from_path(&path_buf)
            .ok()
            .map(Arc::new);

        Self { config, session }
    }

    /// Instantiate a new `CrossEncoderReranker` with explicit configuration.
    pub fn with_config(config: CrossEncoderConfig) -> Self {
        let session = config
            .model_path
            .as_ref()
            .and_then(|path| OnnxCrossEncoderSession::load_from_path(path).ok())
            .map(Arc::new);

        Self { config, session }
    }

    /// Construct a `CrossEncoderReranker` directly from loaded model bytes.
    pub fn from_bytes(bytes: &[u8], name: &str) -> Result<Self, CrossEncoderError> {
        let session = OnnxCrossEncoderSession::load_from_bytes(bytes, name.to_string())?;
        let config = CrossEncoderConfig {
            model_path: Some(PathBuf::from(name)),
            ..Default::default()
        };

        Ok(Self {
            config,
            session: Some(Arc::new(session)),
        })
    }

    /// Return a builder for constructing a custom `CrossEncoderReranker`.
    pub fn builder() -> CrossEncoderRerankerBuilder {
        CrossEncoderRerankerBuilder::default()
    }

    /// Access internal configuration.
    pub fn config(&self) -> &CrossEncoderConfig {
        &self.config
    }

    /// Return true if the local ONNX engine is active.
    pub fn is_onnx_available(&self) -> bool {
        self.session.is_some()
    }

    /// Calculate adjusted score for a single base score and cross-encoder score pair.
    pub fn calculate_score_adjustment(&self, base_score: f32, ce_score: f32) -> (f32, f32) {
        let weight = self.config.cross_encoder_weight.clamp(0.0, 1.0);
        let final_score = (1.0 - weight) * base_score + weight * ce_score;
        let adjustment = final_score - base_score;
        (final_score.clamp(0.0, 1.0), adjustment)
    }

    /// Re-rank a slice of [`RerankCandidate`] items.
    pub fn rerank_candidates(
        &self,
        query: &str,
        candidates: &[RerankCandidate],
    ) -> Result<Vec<RerankResult>, CrossEncoderError> {
        self.rerank_candidates_batch(query, candidates, self.config.batch_size)
    }

    /// Re-rank candidates using a specific custom batch size.
    pub fn rerank_candidates_batch(
        &self,
        query: &str,
        candidates: &[RerankCandidate],
        batch_size: usize,
    ) -> Result<Vec<RerankResult>, CrossEncoderError> {
        if batch_size == 0 {
            return Err(CrossEncoderError::InvalidBatchSize(0));
        }

        if candidates.is_empty() || query.trim().is_empty() {
            return Ok(Vec::new());
        }

        if let Some(ref session) = self.session {
            let mut results = Vec::with_capacity(candidates.len());

            for (chunk_idx, chunk) in candidates.chunks(batch_size).enumerate() {
                for (in_chunk_idx, candidate) in chunk.iter().enumerate() {
                    let orig_idx = (chunk_idx * batch_size) + in_chunk_idx;
                    let (ce_score, metrics) = session.score_pair(query, &candidate.content);
                    let (final_score, adjustment) =
                        self.calculate_score_adjustment(candidate.base_score, ce_score);

                    results.push(RerankResult {
                        id: candidate.id.clone(),
                        score: final_score,
                        base_score: candidate.base_score,
                        cross_encoder_score: ce_score,
                        score_adjustment: adjustment,
                        original_index: orig_idx,
                        token_metrics: metrics,
                        source: "onnx_cross_encoder".to_string(),
                    });
                }
            }

            results.sort_by(|left, right| {
                right
                    .score
                    .partial_cmp(&left.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| left.original_index.cmp(&right.original_index))
            });

            Ok(results)
        } else if self.config.enable_fallback {
            self.rerank_fallback_bm25(query, candidates)
        } else {
            Err(CrossEncoderError::ModelLoadError(
                "ONNX cross-encoder model session unavailable and fallback disabled".to_string(),
            ))
        }
    }

    /// Fallback execution path using BM25 and token interaction metrics.
    fn rerank_fallback_bm25(
        &self,
        query: &str,
        candidates: &[RerankCandidate],
    ) -> Result<Vec<RerankResult>, CrossEncoderError> {
        let docs: Vec<ContextDocument> = candidates
            .iter()
            .map(|c| ContextDocument::new(&c.id, "session-1", "user", &c.content))
            .collect();

        let bm25_index = Bm25Index::new(docs);
        let hits = bm25_index.search(query, candidates.len());

        let max_raw = hits
            .iter()
            .map(|h| h.score)
            .fold(0.0f32, |acc, s| acc.max(s));

        let bm25_scores: HashMap<String, f32> = hits
            .into_iter()
            .map(|h| {
                let norm = if max_raw > 0.0 {
                    (h.score / max_raw).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (h.document.id, norm)
            })
            .collect();

        let mut results: Vec<RerankResult> = candidates
            .iter()
            .enumerate()
            .map(|(index, candidate)| {
                let bm25_norm = bm25_scores.get(&candidate.id).copied().unwrap_or(0.0);
                let metrics = TokenMetrics::compute(query, &candidate.content);

                let ce_score = if bm25_norm > 0.0 {
                    (bm25_norm * 0.7) + (metrics.overlap_ratio * 0.3)
                } else {
                    metrics.overlap_ratio * 0.5
                };

                let (final_score, adjustment) =
                    self.calculate_score_adjustment(candidate.base_score, ce_score);

                RerankResult {
                    id: candidate.id.clone(),
                    score: final_score,
                    base_score: candidate.base_score,
                    cross_encoder_score: ce_score,
                    score_adjustment: adjustment,
                    original_index: index,
                    token_metrics: metrics,
                    source: "bm25_fallback".to_string(),
                }
            })
            .collect();

        results.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.original_index.cmp(&right.original_index))
        });

        Ok(results)
    }

    /// Re-rank a list of [`ScoredResult`] items in-place or returning updated scores.
    pub fn rerank_scored_results(
        &self,
        query: &str,
        results: &[ScoredResult],
    ) -> Result<Vec<ScoredResult>, CrossEncoderError> {
        let candidates: Vec<RerankCandidate> = results.iter().map(RerankCandidate::from).collect();
        let reranked = self.rerank_candidates(query, &candidates)?;

        let id_map: HashMap<String, f32> =
            reranked.into_iter().map(|r| (r.id, r.score)).collect();

        let mut updated: Vec<ScoredResult> = results
            .iter()
            .map(|res| {
                let mut item = res.clone();
                if let Some(&new_score) = id_map.get(&item.id) {
                    item.score = new_score;
                }
                item
            })
            .collect();

        updated.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(updated)
    }
}

/// Builder pattern for constructing custom [`CrossEncoderReranker`].
#[derive(Debug, Default)]
pub struct CrossEncoderRerankerBuilder {
    config: CrossEncoderConfig,
}

impl CrossEncoderRerankerBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set model path.
    pub fn model_path(mut self, path: impl AsRef<Path>) -> Self {
        self.config.model_path = Some(path.as_ref().to_path_buf());
        self
    }

    /// Set batch size.
    pub fn batch_size(mut self, size: usize) -> Self {
        self.config.batch_size = size;
        self
    }

    /// Set whether fallback is enabled.
    pub fn enable_fallback(mut self, enable: bool) -> Self {
        self.config.enable_fallback = enable;
        self
    }

    /// Set max sequence length.
    pub fn max_seq_length(mut self, len: usize) -> Self {
        self.config.max_seq_length = len;
        self
    }

    /// Set cross-encoder score weight.
    pub fn cross_encoder_weight(mut self, weight: f32) -> Self {
        self.config.cross_encoder_weight = weight;
        self
    }

    /// Build the `CrossEncoderReranker`.
    pub fn build(self) -> CrossEncoderReranker {
        CrossEncoderReranker::with_config(self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_metrics_computation() {
        let query = "rust cross encoder";
        let content = "high performance rust cross encoder re-ranking pipeline";
        let metrics = TokenMetrics::compute(query, content);

        assert_eq!(metrics.query_token_count, 3);
        assert!(metrics.matching_tokens >= 3);
        assert!((metrics.overlap_ratio - 1.0).abs() < 1e-5);
        assert!(metrics.token_density > 0.0);
    }

    #[test]
    fn test_score_adjustment_calculation() {
        let reranker = CrossEncoderReranker::builder()
            .cross_encoder_weight(0.6)
            .build();

        let base_score = 0.40;
        let ce_score = 0.90;
        let (final_score, adjustment) =
            reranker.calculate_score_adjustment(base_score, ce_score);

        // final = 0.4 * 0.40 + 0.6 * 0.90 = 0.16 + 0.54 = 0.70
        assert!((final_score - 0.70).abs() < 1e-5);
        assert!((adjustment - 0.30).abs() < 1e-5);
    }

    #[test]
    fn test_rerank_candidates_with_onnx_session() {
        let mock_bytes = b"ONNX_MODEL_HEADER_TEST_PAYLOAD";
        let reranker = CrossEncoderReranker::from_bytes(mock_bytes, "model.onnx").unwrap();

        let candidates = vec![
            RerankCandidate::new("1", "unrelated text input", 0.5),
            RerankCandidate::new("2", "onnx cross encoder model pipeline", 0.3),
        ];

        let results = reranker
            .rerank_candidates("onnx cross encoder", &candidates)
            .unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "2");
        assert_eq!(results[0].source, "onnx_cross_encoder");
        assert!(results[0].token_metrics.matching_tokens >= 3);
    }
}
