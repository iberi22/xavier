//! Human Challenge Analyzer Bridge with Local Embeddings
//!
//! Provides the `HcAnalyzerBridge` which implements `ChallengeEmbedder` and
//! scores incoming human challenge responses against reference answers using
//! Xavier's vector embedding engine with graceful fallback heuristics.

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use tracing::{debug, warn};

use crate::embedding::{Embedder, EmbeddingError};

/// Trait for generating vector embeddings of human challenge text.
#[async_trait]
pub trait ChallengeEmbedder: Send + Sync {
    /// Generates a vector embedding for the given input text.
    async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingError>;
}

/// Bridge connecting Maloca human challenges with Xavier's local vector engine.
pub struct HcAnalyzerBridge {
    embedder: Option<Arc<dyn Embedder>>,
}

impl HcAnalyzerBridge {
    /// Create a new `HcAnalyzerBridge` with a specified `Embedder`.
    pub fn new(embedder: Arc<dyn Embedder>) -> Self {
        Self {
            embedder: Some(embedder),
        }
    }

    /// Create a new `HcAnalyzerBridge` with an optional `Embedder`.
    pub fn with_optional_embedder(embedder: Option<Arc<dyn Embedder>>) -> Self {
        Self { embedder }
    }

    /// Create a new `HcAnalyzerBridge` that relies purely on fallback heuristics.
    pub fn fallback_only() -> Self {
        Self { embedder: None }
    }

    /// Compute cosine similarity between two f32 embedding vectors.
    pub fn cosine_similarity(v1: &[f32], v2: &[f32]) -> f32 {
        if v1.is_empty() || v2.is_empty() || v1.len() != v2.len() {
            return 0.0;
        }

        let mut dot = 0.0f32;
        let mut norm1 = 0.0f32;
        let mut norm2 = 0.0f32;

        for (a, b) in v1.iter().zip(v2.iter()) {
            dot += a * b;
            norm1 += a * a;
            norm2 += b * b;
        }

        if norm1 <= 0.0 || norm2 <= 0.0 {
            return 0.0;
        }

        let sim = dot / (norm1.sqrt() * norm2.sqrt());
        if sim.is_nan() || sim.is_infinite() {
            0.0
        } else {
            sim.clamp(-1.0, 1.0)
        }
    }

    /// Calculate fallback similarity heuristic based on Jaccard token overlap
    /// when the embedding engine is warming up or unavailable.
    pub fn fallback_similarity(a: &str, b: &str) -> f32 {
        let normalize_tokens = |text: &str| -> HashSet<String> {
            text.to_lowercase()
                .split_whitespace()
                .map(|s| s.trim_matches(|c: char| !c.is_alphanumeric()))
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect()
        };

        let set_a = normalize_tokens(a);
        let set_b = normalize_tokens(b);

        if set_a.is_empty() && set_b.is_empty() {
            return 1.0;
        }
        if set_a.is_empty() || set_b.is_empty() {
            return 0.0;
        }

        let intersection_count = set_a.intersection(&set_b).count();
        let union_count = set_a.union(&set_b).count();

        if union_count == 0 {
            0.0
        } else {
            (intersection_count as f32 / union_count as f32).clamp(0.0, 1.0)
        }
    }

    /// Scores a human challenge response against a reference answer.
    ///
    /// Uses vector embedding cosine similarity when available and non-empty.
    /// Falls back gracefully to token-overlap heuristics if the embedding engine
    /// is warming up, unavailable, or fails to return valid embeddings.
    pub async fn score_response(&self, response: &str, reference: &str) -> f32 {
        if response.trim().is_empty() || reference.trim().is_empty() {
            return 0.0;
        }

        if let Some(ref embedder) = self.embedder {
            let res_emb = embedder.encode(response).await;
            let ref_emb = embedder.encode(reference).await;

            match (res_emb, ref_emb) {
                (Ok(v1), Ok(v2)) if !v1.is_empty() && !v2.is_empty() => {
                    let sim = Self::cosine_similarity(&v1, &v2);
                    debug!(
                        similarity = sim,
                        "Scored human challenge response using vector embeddings"
                    );
                    return sim.clamp(0.0, 1.0);
                }
                (Err(e1), _) => {
                    warn!(error = %e1, "Embedder error for response, using fallback heuristic");
                }
                (_, Err(e2)) => {
                    warn!(error = %e2, "Embedder error for reference, using fallback heuristic");
                }
                _ => {
                    debug!("Embeddings empty (possibly warming up), using fallback heuristic");
                }
            }
        }

        let fallback_score = Self::fallback_similarity(response, reference);
        debug!(
            score = fallback_score,
            "Scored human challenge response using fallback heuristic"
        );
        fallback_score
    }
}

#[async_trait]
impl ChallengeEmbedder for HcAnalyzerBridge {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        if let Some(ref embedder) = self.embedder {
            embedder.encode(text).await
        } else {
            Ok(Vec::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::NoopEmbedder;

    struct DummyMockEmbedder {
        dim: usize,
    }

    #[async_trait]
    impl Embedder for DummyMockEmbedder {
        async fn encode(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
            if text.contains("apple") {
                Ok(vec![1.0, 0.0, 0.0])
            } else if text.contains("fruit") {
                Ok(vec![0.8, 0.6, 0.0])
            } else if text.contains("car") {
                Ok(vec![0.0, 1.0, 0.0])
            } else {
                Ok(vec![0.0, 0.0, 1.0])
            }
        }

        fn dimension(&self) -> usize {
            self.dim
        }
    }

    #[test]
    fn test_cosine_similarity_edge_cases() {
        assert_eq!(HcAnalyzerBridge::cosine_similarity(&[], &[]), 0.0);
        assert_eq!(
            HcAnalyzerBridge::cosine_similarity(&[1.0, 2.0], &[1.0]),
            0.0
        );
        assert_eq!(
            HcAnalyzerBridge::cosine_similarity(&[0.0, 0.0], &[0.0, 0.0]),
            0.0
        );

        let v1 = vec![1.0, 0.0, 0.0];
        let v2 = vec![1.0, 0.0, 0.0];
        assert!((HcAnalyzerBridge::cosine_similarity(&v1, &v2) - 1.0).abs() < 1e-5);

        let v3 = vec![0.0, 1.0, 0.0];
        assert!((HcAnalyzerBridge::cosine_similarity(&v1, &v3) - 0.0).abs() < 1e-5);
    }

    #[test]
    fn test_fallback_similarity() {
        assert_eq!(HcAnalyzerBridge::fallback_similarity("", ""), 1.0);
        assert_eq!(HcAnalyzerBridge::fallback_similarity("hello", ""), 0.0);

        let score_exact =
            HcAnalyzerBridge::fallback_similarity("use sqlite store", "use sqlite store!");
        assert_eq!(score_exact, 1.0);

        let score_partial =
            HcAnalyzerBridge::fallback_similarity("use sqlite database", "use postgres database");
        // "use", "database" overlap (2 common) out of {"use", "sqlite", "postgres", "database"} (4 total) => 2/4 = 0.5
        assert!((score_partial - 0.5).abs() < 1e-4);

        let score_disjoint = HcAnalyzerBridge::fallback_similarity("alpha beta", "gamma delta");
        assert_eq!(score_disjoint, 0.0);
    }

    #[tokio::test]
    async fn test_hc_analyzer_bridge_fallback_only() {
        let bridge = HcAnalyzerBridge::fallback_only();
        let score = bridge
            .score_response("we decided sqlite", "we decided sqlite")
            .await;
        assert_eq!(score, 1.0);

        let empty_score = bridge.score_response("", "ref").await;
        assert_eq!(empty_score, 0.0);
    }

    #[tokio::test]
    async fn test_hc_analyzer_bridge_noop_embedder_fallback() {
        let bridge = HcAnalyzerBridge::new(Arc::new(NoopEmbedder));
        // NoopEmbedder returns Ok(vec![]), so bridge should gracefully use fallback_similarity
        let score = bridge.score_response("we use rust", "we use rust").await;
        assert_eq!(score, 1.0);
    }

    #[tokio::test]
    async fn test_hc_analyzer_bridge_with_mock_embedder() {
        let embedder = Arc::new(DummyMockEmbedder { dim: 3 });
        let bridge = HcAnalyzerBridge::new(embedder.clone());

        // "apple" -> [1, 0, 0], "fruit" -> [0.8, 0.6, 0]
        // Cosine sim = 0.8 / (1.0 * 1.0) = 0.8
        let score = bridge.score_response("red apple", "fresh fruit").await;
        assert!((score - 0.8).abs() < 1e-4);

        // Test embed trait method
        let vec = bridge.embed("red apple").await.unwrap();
        assert_eq!(vec, vec![1.0, 0.0, 0.0]);
    }
}
