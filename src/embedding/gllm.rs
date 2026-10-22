//! Local LLM embedding generation
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use crate::embedding::{Embedder, EmbeddingError};

// Best overall: Qwen3-Embedding-0.6B (MTEB 67.5, 1024d) — ideal with GPU.
// Fallback: all-MiniLM-L6-v2 (MTEB 58.8, 384d) — fastest on CPU.
pub const DEFAULT_GLLM_MODEL: &str = "Qwen/Qwen3-Embedding-0.6B";
pub const DEFAULT_GLLM_DIMENSION: usize = 1024;

#[cfg(any(feature = "local-gllm", feature = "local-gllm-cuda"))]
type InnerEmbedder = ::gllm::FallbackEmbedder;

/// Returns the cached value for `key`, running `init` only on a miss.
/// A failed `init` is not cached. The lock is held during `init` so
/// concurrent callers do not load the same weights twice.
#[cfg_attr(
    not(any(feature = "local-gllm", feature = "local-gllm-cuda")),
    allow(dead_code)
)]
fn get_or_init_cached<T, E>(
    cache: &Mutex<HashMap<String, Arc<T>>>,
    key: &str,
    init: impl FnOnce() -> Result<Arc<T>, E>,
) -> Result<Arc<T>, E> {
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(hit) = guard.get(key) {
        return Ok(Arc::clone(hit));
    }
    let value = init()?;
    guard.insert(key.to_string(), Arc::clone(&value));
    Ok(value)
}

#[cfg(any(feature = "local-gllm", feature = "local-gllm-cuda"))]
fn embedder_cache() -> &'static Mutex<HashMap<String, Arc<InnerEmbedder>>> {
    static CACHE: std::sync::OnceLock<Mutex<HashMap<String, Arc<InnerEmbedder>>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub struct GllmEmbedder {
    #[cfg(any(feature = "local-gllm", feature = "local-gllm-cuda"))]
    inner: Arc<InnerEmbedder>,
    model: String,
    dimension: usize,
}

impl fmt::Debug for GllmEmbedder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GllmEmbedder")
            .field("model", &self.model)
            .field("dimension", &self.dimension)
            .finish_non_exhaustive()
    }
}

impl GllmEmbedder {
    #[cfg(any(feature = "local-gllm", feature = "local-gllm-cuda"))]
    /// New.
    pub fn new(model: String, dimension: usize) -> Result<Self, EmbeddingError> {
        let inner = get_or_init_cached(embedder_cache(), &model, || {
            ::gllm::FallbackEmbedder::new(&model)
                .map(Arc::new)
                .map_err(|error| EmbeddingError::Config(error.to_string()))
        })?;

        Ok(Self {
            inner,
            model,
            dimension,
        })
    }

    #[cfg(not(any(feature = "local-gllm", feature = "local-gllm-cuda")))]
    /// New.
    pub fn new(model: String, _dimension: usize) -> Result<Self, EmbeddingError> {
        Err(EmbeddingError::Config(format!(
            "gllm embedder requested for model {model}, but Xavier was built without the local-gllm feature"
        )))
    }
}

#[async_trait::async_trait]
impl Embedder for GllmEmbedder {
    async fn encode(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        #[cfg(any(feature = "local-gllm", feature = "local-gllm-cuda"))]
        {
            let inner = Arc::clone(&self.inner);
            let text = text.to_string();
            return tokio::task::spawn_blocking(move || inner.embed(&text))
                .await
                .map_err(|error| EmbeddingError::Network(error.to_string()))?
                .map_err(|error| EmbeddingError::Network(error.to_string()));
        }

        #[cfg(not(any(feature = "local-gllm", feature = "local-gllm-cuda")))]
        {
            let _ = text;
            Err(EmbeddingError::Config(
                "gllm embedder requested, but Xavier was built without the local-gllm feature"
                    .to_string(),
            ))
        }
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

/// Normalize model name.
pub fn normalize_model_name(raw: &str) -> String {
    match raw.trim() {
        "" => DEFAULT_GLLM_MODEL.to_string(),
        "minilm-l6-v2" | "minilm-l6-v2-q4" | "all-minilm-l6-v2" => DEFAULT_GLLM_MODEL.to_string(),
        value => value.to_string(),
    }
}

/// Dimension for model.
pub fn dimension_for_model(model: &str) -> usize {
    match model.trim().to_ascii_lowercase().as_str() {
        "all-minilm-l6-v2"
        | "minilm-l6-v2"
        | "minilm-l6-v2-q4"
        | "all-minilm-l12-v2"
        | "bge-small-en"
        | "e5-small"
        | "jina-embeddings-v2-small-en"
        | "multilingual-minilm-l12-v2" => 384,
        "bge-small-zh" => 512,
        "bge-base-en" | "e5-base" | "all-mpnet-base-v2" | "m3e-base" => 768,
        "bge-large-en" | "e5-large" | "qwen3-embedding-0.6b" | "codexembed-400m" => 1024,
        "codexembed-2b" => 1536,
        "jina-embeddings-v4" => 2048,
        "qwen3-embedding-4b" => 2560,
        "qwen3-embedding-8b" | "llama-embed-nemotron-8b" | "codexembed-7b" => 4096,
        _ => DEFAULT_GLLM_DIMENSION,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn cache_runs_init_once_per_key() {
        let cache = Mutex::new(HashMap::new());
        let calls = Cell::new(0);
        let init = || -> Result<Arc<u32>, String> {
            calls.set(calls.get() + 1);
            Ok(Arc::new(calls.get()))
        };
        let a = get_or_init_cached(&cache, "m", init).unwrap();
        let b = get_or_init_cached(&cache, "m", init).unwrap();
        assert_eq!(calls.get(), 1);
        assert!(Arc::ptr_eq(&a, &b));
        let c = get_or_init_cached(&cache, "other", init).unwrap();
        assert_eq!(calls.get(), 2);
        assert!(!Arc::ptr_eq(&a, &c));
    }

    #[test]
    fn cache_does_not_store_errors() {
        let cache: Mutex<HashMap<String, Arc<u32>>> = Mutex::new(HashMap::new());
        let calls = Cell::new(0);
        let failed = get_or_init_cached(&cache, "m", || {
            calls.set(calls.get() + 1);
            Err::<Arc<u32>, _>("boom")
        });
        assert!(failed.is_err());
        let ok = get_or_init_cached(&cache, "m", || {
            calls.set(calls.get() + 1);
            Ok::<_, &str>(Arc::new(7))
        })
        .unwrap();
        assert_eq!((*ok, calls.get()), (7, 2));
    }

    #[test]
    fn test_normalize_model_name() {
        assert_eq!(normalize_model_name(""), DEFAULT_GLLM_MODEL);
        assert_eq!(normalize_model_name("minilm-l6-v2"), DEFAULT_GLLM_MODEL);
        assert_eq!(normalize_model_name("all-minilm-l6-v2"), DEFAULT_GLLM_MODEL);
        assert_eq!(normalize_model_name("custom-model"), "custom-model");
    }

    #[test]
    fn test_dimension_for_model() {
        assert_eq!(dimension_for_model("all-minilm-l6-v2"), 384);
        assert_eq!(dimension_for_model("bge-base-en"), 768);
        assert_eq!(dimension_for_model("qwen3-embedding-0.6b"), 1024);
        assert_eq!(dimension_for_model("unknown-model"), DEFAULT_GLLM_DIMENSION);
    }

    #[tokio::test]
    #[ignore = "requires large model download and local-gllm feature; run with --ignored"]
    #[cfg(feature = "local-gllm")]
    async fn test_gllm_embedder_load() {
        let model = DEFAULT_GLLM_MODEL.to_string();
        let dim = DEFAULT_GLLM_DIMENSION;
        let embedder = GllmEmbedder::new(model, dim);
        assert!(embedder.is_ok());
    }

    #[tokio::test]
    #[ignore = "requires large model download and local-gllm feature; run with --ignored"]
    #[cfg(feature = "local-gllm")]
    async fn test_gllm_embedder_encode() {
        let model = DEFAULT_GLLM_MODEL.to_string();
        let dim = DEFAULT_GLLM_DIMENSION;
        let embedder = GllmEmbedder::new(model, dim).unwrap();
        let result = embedder.encode("hello world").await;
        assert!(result.is_ok());
        let vector = result.unwrap();
        assert_eq!(vector.len(), dim);
    }

    #[test]
    #[cfg(not(any(feature = "local-gllm", feature = "local-gllm-cuda")))]
    fn test_gllm_embedder_disabled_error() {
        let model = DEFAULT_GLLM_MODEL.to_string();
        let dim = DEFAULT_GLLM_DIMENSION;
        let embedder = GllmEmbedder::new(model, dim);
        assert!(embedder.is_err());
        let err = embedder.unwrap_err();
        assert!(err.to_string().contains("without the local-gllm feature"));
    }
}
