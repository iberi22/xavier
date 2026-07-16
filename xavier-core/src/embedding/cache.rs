use std::sync::Arc;
use std::path::PathBuf;
use crate::embedding::{Embedder, EmbeddingError};

#[derive(Clone, Debug)]
pub struct EmbeddingCacheConfig {
    pub enabled: bool,
    pub max_capacity: usize,
    pub ttl_hours: u64,
    pub db_path: PathBuf,
}

impl EmbeddingCacheConfig {
    pub fn from_env() -> Self {
        Self {
            enabled: false,
            max_capacity: 0,
            ttl_hours: 0,
            db_path: PathBuf::new(),
        }
    }
}

pub struct EmbeddingCache;
impl EmbeddingCache {
    pub fn new(_config: EmbeddingCacheConfig) -> Self {
        Self
    }
}

pub struct CachedEmbedder {
    inner: Arc<dyn Embedder>,
}

impl CachedEmbedder {
    pub fn new(inner: Arc<dyn Embedder>, _cache: Arc<EmbeddingCache>) -> Self {
        Self { inner }
    }
}

#[async_trait::async_trait]
impl Embedder for CachedEmbedder {
    async fn encode(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        self.inner.encode(text).await
    }
    fn dimension(&self) -> usize {
        self.inner.dimension()
    }
}
