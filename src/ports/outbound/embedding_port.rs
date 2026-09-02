//! Embedding port — port-based embedding provider abstraction dispatch.

use crate::embedding::{Embedder, EmbeddingError};
use std::sync::Arc;

pub struct EmbeddingPort {
    embedder: Option<Arc<dyn Embedder>>,
}

impl EmbeddingPort {
    /// New.
    pub fn new() -> Self {
        Self { embedder: None }
    }

    /// Create embedding port with an embedder provider.
    pub fn with_embedder(embedder: Arc<dyn Embedder>) -> Self {
        Self {
            embedder: Some(embedder),
        }
    }

    /// Embed text using the configured embedding provider.
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        if let Some(ref embedder) = self.embedder {
            embedder.encode(text).await
        } else {
            Err(EmbeddingError::Config(
                "Embedding provider not configured for EmbeddingPort".to_string(),
            ))
        }
    }
}

impl Default for EmbeddingPort {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_embedding_port_unconfigured() {
        let port = EmbeddingPort::new();
        let res = port.embed("test").await;
        assert!(res.is_err());
    }
}
