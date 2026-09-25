//! Memory embedding generator
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use std::sync::Arc;

use anyhow::{anyhow, Result};

use crate::embedding::{build_embedder_from_env, Embedder, EmbedderConfig};

pub struct EmbeddingClient {
    embedder: Arc<dyn Embedder>,
}

impl EmbeddingClient {
    /// From env.
    pub fn from_env() -> Result<Self> {
        let config = EmbedderConfig::from_env();
        if !config.is_configured() {
            return Err(anyhow!("embedding provider is not configured"));
        }

        Ok(Self {
            embedder: config
                .build_sync()
                .map_err(|error| anyhow!(error.to_string()))?,
        })
    }

    /// Is configured from env.
    pub fn is_configured_from_env() -> bool {
        EmbedderConfig::from_env().is_configured()
    }

    /// Embed.
    pub async fn embed(&self, input: &str) -> Result<Vec<f32>> {
        self.embedder
            .encode(input)
            .await
            .map_err(|error| anyhow!(error.to_string()))
    }

    /// Health.
    pub async fn health(&self) -> Result<bool> {
        Ok(!self.embed("health check").await?.is_empty())
    }

    /// From env async.
    pub async fn from_env_async() -> Result<Self> {
        let config = EmbedderConfig::from_env();
        if !config.is_configured() {
            return Err(anyhow!("embedding provider is not configured"));
        }

        Ok(Self {
            embedder: build_embedder_from_env()
                .await
                .map_err(|error| anyhow!(error.to_string()))?,
        })
    }
}

/// Per-attempt timeout budget for a single embedding request, configurable via
/// `XAVIER_EMBEDDING_TIMEOUT_MS` (default: 3000ms / 3s).
///
/// Kept intentionally short: on a fresh install the configured embedding
/// endpoint (remote Ollama, OpenAI-compatible URL, ...) is commonly
/// unreachable, and memory writes/searches must degrade to lexical/FTS
/// quickly instead of hanging behind the outer HTTP request-timeout
/// middleware. Callers that need a hard ceiling on *total* wall-clock time
/// (including retries) should additionally wrap the call in
/// `tokio::time::timeout` using `embedding_fallback_budget()`.
pub fn embedding_timeout() -> std::time::Duration {
    let ms = std::env::var("XAVIER_EMBEDDING_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(3000);
    std::time::Duration::from_millis(ms)
}

/// Total wall-clock budget allowed for an embedding call (including internal
/// retries) before a caller on the interactive add/search path must give up
/// and degrade to lexical/FTS results. Configurable via
/// `XAVIER_EMBEDDING_FALLBACK_BUDGET_MS` (default: 2000ms / 2s).
pub fn embedding_fallback_budget() -> std::time::Duration {
    let ms = std::env::var("XAVIER_EMBEDDING_FALLBACK_BUDGET_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(2000);
    std::time::Duration::from_millis(ms)
}
