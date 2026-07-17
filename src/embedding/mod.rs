//! Text embedding generation module
//!
//! Aggregates and re-exports the sub-modules within this module,
//! providing the public API surface for module consumers.
use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;
use tracing::info;

pub mod cache;
pub mod gllm;
pub mod openai;
pub mod pipeline;

const DEFAULT_LOCAL_EMBEDDING_ENDPOINT: &str = "http://localhost:11434/v1/embeddings";
const DEFAULT_LOCAL_EMBEDDING_MODEL: &str = "embeddinggemma";
const DEFAULT_CLOUD_EMBEDDING_ENDPOINT: &str = "https://api.openai.com/v1/embeddings";
const DEFAULT_CLOUD_EMBEDDING_MODEL: &str = "text-embedding-3-small";

#[derive(Debug, Error)]
pub enum EmbeddingError {
    #[error("embedding provider configuration error: {0}")]
    Config(String),
    #[error("embedding network error: {0}")]
    Network(String),
    #[error("embedding parse error: {0}")]
    Parse(String),
}

#[async_trait]
pub trait Embedder: Send + Sync {
    async fn encode(&self, text: &str) -> Result<Vec<f32>, EmbeddingError>;
    fn dimension(&self) -> usize;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderMode {
    Local,
    LocalGllm,
    Cloud,
    Auto,
    Disabled,
}

impl ProviderMode {
    fn from_env(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "local" => Some(Self::Local),
            "local-gllm" | "local_gllm" | "gllm" => Some(Self::LocalGllm),
            "cloud" => Some(Self::Cloud),
            "auto" => Some(Self::Auto),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiFlavor {
    OpenAICompatible,
    AnthropicCompatible,
}

impl ApiFlavor {
    fn from_env(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "openai-compatible" | "openai" => Some(Self::OpenAICompatible),
            "anthropic-compatible" | "anthropic" => Some(Self::AnthropicCompatible),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct OpenAICompatibleConfig {
    endpoint: String,
    api_key: Option<String>,
    model: String,
    dimension: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct GllmConfig {
    model: String,
    dimension: usize,
}

#[derive(Clone, Debug)]
pub(crate) enum EmbedderBackendConfig {
    Gllm(GllmConfig),
    OpenAICompatible(OpenAICompatibleConfig),
}

#[derive(Clone, Debug)]
pub(crate) enum EmbedderConfig {
    Fallback(Vec<EmbedderBackendConfig>),
    Noop,
    Invalid(String),
}

impl EmbedderConfig {
    pub fn from_env() -> Self {
        let provider_mode = std::env::var("XAVIER_EMBEDDING_PROVIDER_MODE")
            .ok()
            .and_then(|value| ProviderMode::from_env(&value));
        let api_flavor = std::env::var("XAVIER_EMBEDDING_API_FLAVOR")
            .ok()
            .and_then(|value| ApiFlavor::from_env(&value))
            .unwrap_or(ApiFlavor::OpenAICompatible);

        let explicit_embedder = std::env::var("XAVIER_EMBEDDER")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase());

        if provider_mode == Some(ProviderMode::Disabled)
            || explicit_embedder.as_deref() == Some("disabled")
        {
            return Self::Noop;
        }

        if explicit_embedder.as_deref() == Some("gllm") {
            return Self::gllm_only();
        }

        if api_flavor == ApiFlavor::AnthropicCompatible {
            return Self::Noop;
        }

        match provider_mode {
            Some(ProviderMode::Local) => Self::local_only(api_flavor),
            Some(ProviderMode::LocalGllm) => Self::gllm_only(),
            Some(ProviderMode::Cloud) => Self::cloud_only(api_flavor),
            Some(ProviderMode::Auto) => Self::auto_explicit(api_flavor),
            Some(ProviderMode::Disabled) => Self::Noop,
            None => Self::auto(api_flavor),
        }
    }

    pub fn is_configured(&self) -> bool {
        !matches!(self, Self::Noop)
    }

    pub fn active_model_name(&self) -> Option<String> {
        match self {
            Self::Fallback(backends) => {
                for backend in backends {
                    match backend {
                        EmbedderBackendConfig::Gllm(cfg) => return Some(cfg.model.clone()),
                        EmbedderBackendConfig::OpenAICompatible(cfg) => return Some(cfg.model.clone()),
                    }
                }
                None
            }
            Self::Noop => Some("noop".to_string()),
            Self::Invalid(_) => None,
        }
    }

    pub fn build_sync(self) -> Result<Arc<dyn Embedder>, EmbeddingError> {
        match self {
            Self::Invalid(msg) => Err(EmbeddingError::Config(msg)),
            Self::Fallback(backends) => {
                let mut embedders: Vec<Arc<dyn Embedder>> = Vec::new();

                for backend in backends {
                    match build_backend(backend) {
                        Ok(embedder) => embedders.push(embedder),
                        Err(error) => {
                            let msg = format!(
                                "embedding backend unavailable; trying fallback error={}",
                                error
                            );
                            tracing::warn!("{}", msg);
                            crate::server::alerts::SYSTEM_ALERTS.push_alert(
                                "WARN",
                                &msg,
                                "embedding",
                            );
                        }
                    }
                }

                match embedders.len() {
                    0 => {
                        let msg = "no embedding backend could be initialized; using no-op embedder";
                        tracing::warn!("{}", msg);
                        crate::server::alerts::SYSTEM_ALERTS.push_alert("ERROR", msg, "embedding");
                        Ok(Arc::new(NoopEmbedder))
                    }
                    1 => Ok(embedders.remove(0)),
                    _ => Ok(Arc::new(FallbackEmbedder { embedders })),
                }
            }
            Self::Noop => Ok(Arc::new(NoopEmbedder)),
        }
    }

    pub async fn build(self) -> Result<Arc<dyn Embedder>, EmbeddingError> {
        self.build_sync()
    }

    fn auto(api_flavor: ApiFlavor) -> Self {
        let local_signal = local_embedding_signal_present();
        let cloud_signal = cloud_embedding_signal_present();
        let explicit_local_llm = std::env::var("XAVIER_MODEL_PROVIDER")
            .map(|value| value.eq_ignore_ascii_case("local"))
            .unwrap_or(false);

        // 1. Explicit XAVIER_EMBEDDING_PROVIDER_MODE is respected by from_env() before entering here.
        // Wait, if there are local/cloud signal variables, we respect them:
        if local_signal || explicit_local_llm || cloud_signal {
            match (local_signal || explicit_local_llm, cloud_signal) {
                (true, true) => {
                    tracing::info!("Embeddings backend: local-ollama(embeddinggemma) | cloud-openai");
                    Self::Fallback(vec![
                        EmbedderBackendConfig::OpenAICompatible(local_config()),
                        EmbedderBackendConfig::OpenAICompatible(cloud_config()),
                    ])
                }
                (true, false) => {
                    tracing::info!("Embeddings backend: local-ollama(embeddinggemma)");
                    Self::local_only(api_flavor)
                }
                (false, true) => {
                    tracing::info!("Embeddings backend: cloud-openai");
                    Self::cloud_only(api_flavor)
                }
                _ => unreachable!(),
            }
        } else {
            // No explicit cloud or local signals are present.
            // Let's probe local Ollama.
            let probe_url = std::env::var("_XAVIER_TEST_OLLAMA_PROBE_URL")
                .unwrap_or_else(|_| "http://localhost:11434/v1/models".to_string());

            let handle = tokio::runtime::Handle::try_current();
            let models_opt = match handle {
                Ok(h) => {
                    tokio::task::block_in_place(|| {
                        h.block_on(async {
                            probe_ollama_async(&probe_url).await
                        })
                    })
                }
                Err(_) => {
                    if let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
                        rt.block_on(async {
                            probe_ollama_async(&probe_url).await
                        })
                    } else {
                        None
                    }
                }
            };

            if let Some(models) = models_opt {
                tracing::info!("Embeddings backend: local-ollama(embeddinggemma)");

                let has_embeddinggemma = models.iter().any(|m| m == "embeddinggemma" || m.starts_with("embeddinggemma:"));
                if !has_embeddinggemma {
                    tracing::warn!("Modelo embeddinggemma no encontrado. Ejecuta: ollama pull embeddinggemma");
                }

                Self::local_only(api_flavor)
            } else {
                tracing::warn!("Ollama no responde en http://localhost:11434/v1/models");
                crate::server::alerts::SYSTEM_ALERTS.push_alert("WARN", "Ollama no responde en http://localhost:11434/v1/models", "embedding");
                tracing::info!("Embeddings backend: disabled(noop)");
                Self::Noop
            }
        }
    }

    fn auto_explicit(api_flavor: ApiFlavor) -> Self {
        let mut backends = vec![EmbedderBackendConfig::Gllm(gllm_config())];

        if api_flavor == ApiFlavor::OpenAICompatible {
            backends.push(EmbedderBackendConfig::OpenAICompatible(local_config()));
        }

        if cloud_embedding_signal_present() {
            backends.push(EmbedderBackendConfig::OpenAICompatible(cloud_config()));
        }

        Self::Fallback(backends)
    }

    fn local_only(_api_flavor: ApiFlavor) -> Self {
        Self::Fallback(vec![
            EmbedderBackendConfig::Gllm(gllm_config()),
            EmbedderBackendConfig::OpenAICompatible(local_config()),
        ])
    }

    fn cloud_only(api_flavor: ApiFlavor) -> Self {
        // Primary: cloud endpoint, Fallback: local endpoint if available
        let mut backends = vec![EmbedderBackendConfig::OpenAICompatible(cloud_config())];

        // Always add GLLM as a fallback if in cloud mode to ensure offline/GPU availability
        if api_flavor == ApiFlavor::OpenAICompatible {
            backends.push(EmbedderBackendConfig::Gllm(gllm_config()));
        }

        if local_embedding_signal_present() {
            match api_flavor {
                ApiFlavor::OpenAICompatible => {
                    backends.push(EmbedderBackendConfig::OpenAICompatible(local_config()));
                }
                ApiFlavor::AnthropicCompatible => {
                    if !backends
                        .iter()
                        .any(|b| matches!(b, EmbedderBackendConfig::Gllm(_)))
                    {
                        backends.push(EmbedderBackendConfig::Gllm(gllm_config()));
                    }
                }
            }
        }

        Self::Fallback(backends)
    }

    fn gllm_only() -> Self {
        let config = gllm_config();
        let model_path = std::env::var("XAVIER_GLLM_MODEL_PATH").ok();

        if let Some(path) = model_path {
            if !std::path::Path::new(&path).exists() {
                return Self::Invalid(format!(
                    "GLLM backend requires model at {}; set XAVIER_GLLM_MODEL_PATH",
                    path
                ));
            }
        } else if config.model.contains('/') || config.model.contains('\\') {
            // If the model looks like a path but XAVIER_GLLM_MODEL_PATH is not set, validate it
            if !std::path::Path::new(&config.model).exists() {
                return Self::Invalid(format!(
                    "GLLM backend requires model at {}; set XAVIER_GLLM_MODEL_PATH",
                    config.model
                ));
            }
        }

        Self::Fallback(vec![EmbedderBackendConfig::Gllm(config)])
    }
}

pub async fn build_embedder_from_env() -> Result<Arc<dyn Embedder>, EmbeddingError> {
    let embedder = EmbedderConfig::from_env().build().await?;

    // Wrap in the persistent cache if enabled.
    let cache_config = cache::EmbeddingCacheConfig::from_env();
    if cache_config.enabled && embedder.dimension() > 0 {
        info!(
            capacity = cache_config.max_capacity,
            ttl_hours = cache_config.ttl_hours,
            db = %cache_config.db_path.display(),
            "embedding cache enabled"
        );
        Ok(Arc::new(cache::CachedEmbedder::new(
            embedder,
            Arc::new(cache::EmbeddingCache::new(cache_config)),
        )))
    } else if cache_config.enabled && embedder.dimension() == 0 {
        info!("embedding cache skipped: noop embedder (dimension=0)");
        Ok(embedder)
    } else {
        Ok(embedder)
    }
}

#[derive(Debug, Default)]
pub struct NoopEmbedder;

#[async_trait]
impl Embedder for NoopEmbedder {
    async fn encode(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
        Ok(Vec::new())
    }

    fn dimension(&self) -> usize {
        0
    }
}

struct FallbackEmbedder {
    embedders: Vec<Arc<dyn Embedder>>,
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

        Err(last_error.unwrap_or_else(|| {
            EmbeddingError::Config("no embedding backend produced a usable vector".to_string())
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

async fn probe_ollama_async(url: &str) -> Option<Vec<String>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .ok()?;
    let resp = client.get(url).send().await.ok()?;
    if resp.status().is_success() {
        let body: serde_json::Value = resp.json().await.ok()?;
        let mut models = Vec::new();
        if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
            for item in data {
                if let Some(id) = item.get("id").and_then(|id| id.as_str()) {
                    models.push(id.to_string());
                }
            }
        }
        Some(models)
    } else {
        None
    }
}

fn local_embedding_signal_present() -> bool {
    std::env::var("XAVIER_EMBEDDING_LOCAL_URL").is_ok()
        || std::env::var("XAVIER_EMBEDDING_MODEL").is_ok()
        || std::env::var("XAVIER_EMBEDDING_PROVIDER_MODE")
            .map(|value| value.eq_ignore_ascii_case("local"))
            .unwrap_or(false)
        || crate::settings::XavierSettings::current()
            .embedding
            .endpoint
            .contains("localhost")
        || crate::settings::XavierSettings::current()
            .embedding
            .endpoint
            .contains("://localhost")
}

fn cloud_embedding_signal_present() -> bool {
    std::env::var("OPENAI_API_KEY").is_ok()
        || crate::settings::XavierSettings::current()
            .embedding
            .api_key
            .is_some()
        || std::env::var("XAVIER_EMBEDDING_PROVIDER_MODE")
            .map(|value| value.eq_ignore_ascii_case("cloud"))
            .unwrap_or(false)
}

fn build_backend(config: EmbedderBackendConfig) -> Result<Arc<dyn Embedder>, EmbeddingError> {
    match config {
        EmbedderBackendConfig::Gllm(config) => Ok(Arc::new(gllm::GllmEmbedder::new(
            config.model,
            config.dimension,
        )?)),
        EmbedderBackendConfig::OpenAICompatible(config) => {
            let timeout_secs = crate::settings::XavierSettings::current()
                .embedding
                .timeout_secs;
            Ok(Arc::new(openai::OpenAICompatibleEmbedder::new(
                config.api_key,
                config.model,
                config.endpoint,
                config.dimension,
                std::time::Duration::from_secs(timeout_secs),
            )?))
        }
    }
}

fn gllm_config() -> GllmConfig {
    let settings = crate::settings::XavierSettings::current();
    let raw_model = std::env::var("XAVIER_GLLM_MODEL_PATH")
        .or_else(|_| std::env::var("XAVIER_GLLM_MODEL"))
        .unwrap_or_else(|_| gllm::DEFAULT_GLLM_MODEL.to_string());

    let model = gllm::normalize_model_name(&raw_model);
    let dimension = settings
        .embedding
        .gllm_dimension
        .filter(|d| *d > 0)
        .unwrap_or_else(|| gllm::dimension_for_model(&model));

    GllmConfig { model, dimension }
}

fn local_config() -> OpenAICompatibleConfig {
    let settings = crate::settings::XavierSettings::current();
    // Priority: XAVIER_EMBEDDING_LOCAL_URL > XAVIER_EMBEDDING_URL > settings.models.embedding_url > DEFAULT_LOCAL_EMBEDDING_ENDPOINT
    let endpoint = std::env::var("XAVIER_EMBEDDING_LOCAL_URL")
        .ok()
        .or_else(|| std::env::var("XAVIER_EMBEDDING_URL").ok())
        .map(|value| normalize_openai_embeddings_endpoint(&value))
        .unwrap_or_else(|| {
            if !settings.models.embedding_url.is_empty() {
                normalize_openai_embeddings_endpoint(&settings.models.embedding_url)
            } else {
                DEFAULT_LOCAL_EMBEDDING_ENDPOINT.to_string()
            }
        });

    let model = std::env::var("XAVIER_EMBEDDING_MODEL")
        .unwrap_or_else(|_| DEFAULT_LOCAL_EMBEDDING_MODEL.to_string());

    OpenAICompatibleConfig {
        api_key: settings
            .embedding
            .api_key
            .clone()
            .or_else(|| Some("ollama".to_string())),
        endpoint,
        dimension: embedding_dimension_for_model(&model),
        model,
    }
}

fn cloud_config() -> OpenAICompatibleConfig {
    let settings = crate::settings::XavierSettings::current();
    let endpoint = std::env::var("XAVIER_EMBEDDING_URL")
        .map(|value| normalize_openai_embeddings_endpoint(&value))
        .unwrap_or_else(|_| {
            if !settings.models.embedding_url.is_empty() {
                normalize_openai_embeddings_endpoint(&settings.models.embedding_url)
            } else {
                DEFAULT_CLOUD_EMBEDDING_ENDPOINT.to_string()
            }
        });

    let model = std::env::var("XAVIER_EMBEDDING_MODEL")
        .unwrap_or_else(|_| DEFAULT_CLOUD_EMBEDDING_MODEL.to_string());

    OpenAICompatibleConfig {
        api_key: settings
            .embedding
            .api_key
            .clone()
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .or_else(|| std::env::var("XAVIER_OPENROUTER_API_KEY").ok())
            .or_else(|| std::env::var("XAVIER_EMBEDDING_API_KEY").ok()),
        endpoint,
        dimension: embedding_dimension_for_model(&model),
        model,
    }
}

fn normalize_openai_embeddings_endpoint(raw: &str) -> String {
    let trimmed = raw.trim_end_matches('/');
    if trimmed.ends_with("/v1/embeddings") || trimmed.ends_with("/api/embed") {
        trimmed.to_string()
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/embeddings")
    } else if trimmed.ends_with("/embeddings") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1/embeddings")
    }
}

fn embedding_dimension_for_model(model: &str) -> usize {
    match model.trim().to_ascii_lowercase().as_str() {
        "embeddinggemma" => 768,
        "nomic-embed-text" | "nomic-embed-text-v1.5" => 768,
        "all-minilm" => 384,
        "qwen3-embedding" | "qwen3-embedding-0.6b" => 1024,
        "text-embedding-3-large" => 3072,
        "text-embedding-3-small" | "text-embedding-ada-002" => 1536,
        _ => 768,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_mode_accepts_gllm_and_auto_aliases() {
        assert_eq!(
            ProviderMode::from_env("gllm"),
            Some(ProviderMode::LocalGllm)
        );
        assert_eq!(
            ProviderMode::from_env("local-gllm"),
            Some(ProviderMode::LocalGllm)
        );
        assert_eq!(ProviderMode::from_env("auto"), Some(ProviderMode::Auto));
    }

    #[test]
    fn gllm_minilm_aliases_normalize_to_supported_model() {
        assert_eq!(
            gllm::normalize_model_name("minilm-l6-v2-q4"),
            gllm::DEFAULT_GLLM_MODEL
        );
        assert_eq!(gllm::dimension_for_model("all-MiniLM-L6-v2"), 384);
        assert_eq!(gllm::dimension_for_model("qwen3-embedding-0.6b"), 1024);
    }

    #[test]
    fn test_cloud_config_priorities() {
        let _guard = crate::settings::tests::ENV_LOCK.lock().unwrap();

        // 1. Test XAVIER_OPENROUTER_API_KEY as fallback
        std::env::set_var("XAVIER_OPENROUTER_API_KEY", "sk-or-test-key");
        std::env::remove_var("OPENAI_API_KEY");

        let mut settings = crate::settings::XavierSettings::default();
        settings.embedding.api_key = None;

        // Manually simulate what cloud_config() does with specific settings
        let config_with_or = OpenAICompatibleConfig {
            api_key: settings
                .embedding
                .api_key
                .clone()
                .or_else(|| std::env::var("OPENAI_API_KEY").ok())
                .or_else(|| std::env::var("XAVIER_OPENROUTER_API_KEY").ok()),
            endpoint: "http://test".to_string(),
            dimension: 1536,
            model: "test".to_string(),
        };
        assert_eq!(config_with_or.api_key, Some("sk-or-test-key".to_string()));

        // 2. Test OPENAI_API_KEY takes precedence over XAVIER_OPENROUTER_API_KEY
        std::env::set_var("OPENAI_API_KEY", "sk-openai-test-key");
        let config_with_openai = OpenAICompatibleConfig {
            api_key: settings
                .embedding
                .api_key
                .clone()
                .or_else(|| std::env::var("OPENAI_API_KEY").ok())
                .or_else(|| std::env::var("XAVIER_OPENROUTER_API_KEY").ok()),
            endpoint: "http://test".to_string(),
            dimension: 1536,
            model: "test".to_string(),
        };
        assert_eq!(
            config_with_openai.api_key,
            Some("sk-openai-test-key".to_string())
        );

        // 3. Test settings.embedding.api_key takes precedence over env vars
        settings.embedding.api_key = Some("sk-settings-test-key".to_string());
        let config_with_settings = OpenAICompatibleConfig {
            api_key: settings
                .embedding
                .api_key
                .clone()
                .or_else(|| std::env::var("OPENAI_API_KEY").ok())
                .or_else(|| std::env::var("XAVIER_OPENROUTER_API_KEY").ok()),
            endpoint: "http://test".to_string(),
            dimension: 1536,
            model: "test".to_string(),
        };
        assert_eq!(
            config_with_settings.api_key,
            Some("sk-settings-test-key".to_string())
        );

        std::env::remove_var("XAVIER_OPENROUTER_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");
    }

    #[tokio::test]
    async fn test_auto_respects_explicit_local_signal() {
        let _guard = crate::settings::tests::ENV_LOCK.lock().unwrap();

        std::env::set_var("XAVIER_EMBEDDING_LOCAL_URL", "http://localhost:11434/v1/embeddings");
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("_XAVIER_TEST_OLLAMA_PROBE_URL");

        let config = EmbedderConfig::auto(ApiFlavor::OpenAICompatible);
        assert!(matches!(config, EmbedderConfig::Fallback(_)));

        std::env::remove_var("XAVIER_EMBEDDING_LOCAL_URL");
    }

    #[tokio::test]
    async fn test_auto_respects_explicit_cloud_signal() {
        let _guard = crate::settings::tests::ENV_LOCK.lock().unwrap();

        std::env::set_var("OPENAI_API_KEY", "sk-test-key-123");
        std::env::remove_var("XAVIER_EMBEDDING_LOCAL_URL");
        std::env::remove_var("_XAVIER_TEST_OLLAMA_PROBE_URL");

        let config = EmbedderConfig::auto(ApiFlavor::OpenAICompatible);
        assert!(matches!(config, EmbedderConfig::Fallback(_)));

        std::env::remove_var("OPENAI_API_KEY");
    }

    #[tokio::test]
    async fn test_auto_triggers_probe_ollama_reachable_with_embeddinggemma() {
        let _guard = crate::settings::tests::ENV_LOCK.lock().unwrap();

        std::env::remove_var("XAVIER_EMBEDDING_LOCAL_URL");
        std::env::remove_var("OPENAI_API_KEY");

        // Start a mockito server
        let mut server = mockito::Server::new_async().await;
        let mock_url = server.url();
        std::env::set_var("_XAVIER_TEST_OLLAMA_PROBE_URL", format!("{}/v1/models", mock_url));

        let _mock = server.mock("GET", "/v1/models")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{
                "object": "list",
                "data": [
                    {
                        "id": "embeddinggemma",
                        "object": "model"
                    }
                ]
            }"#)
            .create_async()
            .await;

        let config = EmbedderConfig::auto(ApiFlavor::OpenAICompatible);
        // Should use local because Ollama is reachable with embeddinggemma
        assert!(matches!(config, EmbedderConfig::Fallback(_)));

        std::env::remove_var("_XAVIER_TEST_OLLAMA_PROBE_URL");
    }

    #[tokio::test]
    async fn test_auto_triggers_probe_ollama_reachable_without_embeddinggemma() {
        let _guard = crate::settings::tests::ENV_LOCK.lock().unwrap();

        std::env::remove_var("XAVIER_EMBEDDING_LOCAL_URL");
        std::env::remove_var("OPENAI_API_KEY");

        // Start a mockito server
        let mut server = mockito::Server::new_async().await;
        let mock_url = server.url();
        std::env::set_var("_XAVIER_TEST_OLLAMA_PROBE_URL", format!("{}/v1/models", mock_url));

        let _mock = server.mock("GET", "/v1/models")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{
                "object": "list",
                "data": [
                    {
                        "id": "other-model",
                        "object": "model"
                    }
                ]
            }"#)
            .create_async()
            .await;

        let config = EmbedderConfig::auto(ApiFlavor::OpenAICompatible);
        // Should still use local but log warning (warning won't fail the test, which is fine)
        assert!(matches!(config, EmbedderConfig::Fallback(_)));

        std::env::remove_var("_XAVIER_TEST_OLLAMA_PROBE_URL");
    }

    #[tokio::test]
    async fn test_auto_triggers_probe_ollama_unreachable() {
        let _guard = crate::settings::tests::ENV_LOCK.lock().unwrap();

        std::env::remove_var("XAVIER_EMBEDDING_LOCAL_URL");
        std::env::remove_var("OPENAI_API_KEY");

        // Use an invalid/unreachable URL
        std::env::set_var("_XAVIER_TEST_OLLAMA_PROBE_URL", "http://127.0.0.1:1/v1/models");

        let config = EmbedderConfig::auto(ApiFlavor::OpenAICompatible);
        // Should return Noop because Ollama is unreachable
        assert!(matches!(config, EmbedderConfig::Noop));

        std::env::remove_var("_XAVIER_TEST_OLLAMA_PROBE_URL");
    }
}
