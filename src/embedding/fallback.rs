use crate::embedding::{increment_embedding_error_count, Embedder, EmbeddingError};
use async_trait::async_trait;
use std::sync::Arc;

pub struct FallbackEmbedder {
    pub embedders: Vec<Arc<dyn Embedder>>,
}

impl FallbackEmbedder {
    pub fn new(embedders: Vec<Arc<dyn Embedder>>) -> Self {
        Self { embedders }
    }
}

#[async_trait]
impl Embedder for FallbackEmbedder {
    async fn encode(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let mut last_error = None;
        for embedder in &self.embedders {
            match embedder.encode(text).await {
                Ok(vector) if !vector.is_empty() => return Ok(vector),
                Ok(_) => {
                    last_error = Some(EmbeddingError::Parse(
                        "embedding backend returned an empty vector".to_string(),
                    ))
                }
                Err(error) => last_error = Some(error),
            }
        }

        increment_embedding_error_count();
        // Fallback chain conforms to ADR-033: If all neural providers fail/unavailable,
        // return a typed error that triggers graceful degradation to BM25/FTS in the caller,
        // rather than panicking or failing the user request. We must ensure no unhandled panics.
        Err(last_error.unwrap_or_else(|| {
            EmbeddingError::Config("no embedding backend produced a usable vector".to_string())
        }))
    }

    async fn probe_health(&self) -> Result<f64, EmbeddingError> {
        let mut last_error = None;
        for embedder in &self.embedders {
            match embedder.probe_health().await {
                Ok(latency_ms) => return Ok(latency_ms),
                Err(err) => last_error = Some(err),
            }
        }
        increment_embedding_error_count();
        Err(last_error.unwrap_or_else(|| {
            EmbeddingError::Config("no embedding backend responded to health probe".to_string())
        }))
    }

    fn dimension(&self) -> usize {
        self.embedders
            .iter()
            .map(|embedder| embedder.dimension())
            .find(|dimension| *dimension > 0)
            .unwrap_or(0)
    }
}
