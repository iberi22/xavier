//! LLM backend adapter for Xavier DocBot RAG.
//!
//! Supports Ollama (recommended for local-first), OpenAI-compatible APIs,
//! and a no-op "retrieval-only" mode that returns the top chunk without LLM.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

// ── Config ─────────────────────────────────────────────────────────────────────

/// Which LLM backend to use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum LlmBackend {
    /// Local Ollama server (default — local-first, privacy-preserving).
    #[default]
    Ollama,
    /// OpenAI API (or any OpenAI-compatible endpoint).
    OpenAI,
    /// No LLM — just return top retrieved chunks concatenated.
    RetrievalOnly,
}

/// Configuration for the LLM adapter.
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub backend: LlmBackend,
    /// Ollama base URL (default: http://localhost:11434)
    pub ollama_url: String,
    /// OpenAI-compatible API base URL
    pub openai_url: String,
    /// API key (OpenAI) — empty for Ollama
    pub api_key: String,
    /// Model name (e.g. "llama3.2", "gpt-4o-mini")
    pub model: String,
    /// Maximum tokens in the LLM response
    pub max_tokens: u32,
    /// Temperature
    pub temperature: f32,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            backend: LlmBackend::Ollama,
            ollama_url: "http://localhost:11434".to_string(),
            openai_url: "https://api.openai.com/v1".to_string(),
            api_key: String::new(),
            model: "llama3.2".to_string(),
            max_tokens: 1024,
            temperature: 0.2,
        }
    }
}

impl LlmConfig {
    pub fn from_env() -> Self {
        let backend_str =
            std::env::var("DOCBOT_LLM_BACKEND").unwrap_or_else(|_| "ollama".to_string());

        let backend = match backend_str.to_ascii_lowercase().as_str() {
            "openai" | "openai-compatible" => LlmBackend::OpenAI,
            "retrieval-only" | "none" => LlmBackend::RetrievalOnly,
            _ => LlmBackend::Ollama,
        };

        Self {
            backend,
            ollama_url: std::env::var("DOCBOT_OLLAMA_URL")
                .unwrap_or_else(|_| "http://localhost:11434".to_string()),
            openai_url: std::env::var("DOCBOT_OPENAI_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
            api_key: std::env::var("DOCBOT_OPENAI_API_KEY").unwrap_or_default(),
            model: std::env::var("DOCBOT_LLM_MODEL").unwrap_or_else(|_| "llama3.2".to_string()),
            max_tokens: std::env::var("DOCBOT_LLM_MAX_TOKENS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1024),
            temperature: std::env::var("DOCBOT_LLM_TEMPERATURE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.2),
        }
    }
}

// ── Trait ──────────────────────────────────────────────────────────────────────

/// LLM completion backend abstraction.
#[async_trait]
pub trait LlmAdapterTrait: Send + Sync {
    /// Generate a completion given a full prompt string.
    async fn complete(&self, prompt: &str) -> Result<String>;

    /// Return the model name for citation.
    fn model_name(&self) -> &str;
}

// ── Ollama adapter ─────────────────────────────────────────────────────────────

pub struct OllamaAdapter {
    config: LlmConfig,
    client: reqwest::Client,
}

impl OllamaAdapter {
    pub fn new(config: LlmConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
        }
    }
}

#[derive(Serialize)]
struct OllamaRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
    options: OllamaOptions,
}

#[derive(Serialize)]
struct OllamaOptions {
    num_predict: u32,
    temperature: f32,
}

#[derive(Deserialize)]
struct OllamaResponse {
    response: String,
}

#[async_trait]
impl LlmAdapterTrait for OllamaAdapter {
    async fn complete(&self, prompt: &str) -> Result<String> {
        let url = format!("{}/api/generate", self.config.ollama_url);
        let req = OllamaRequest {
            model: &self.config.model,
            prompt,
            stream: false,
            options: OllamaOptions {
                num_predict: self.config.max_tokens,
                temperature: self.config.temperature,
            },
        };

        debug!(model = %self.config.model, "sending to Ollama");

        let resp = self.client.post(&url).json(&req).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Ollama error {}: {}", status, body);
        }

        let ollama_resp: OllamaResponse = resp.json().await?;
        Ok(ollama_resp.response.trim().to_string())
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }
}

// ── OpenAI adapter ─────────────────────────────────────────────────────────────

pub struct OpenAIAdapter {
    config: LlmConfig,
    client: reqwest::Client,
}

impl OpenAIAdapter {
    pub fn new(config: LlmConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
        }
    }
}

#[derive(Serialize)]
struct OpenAIRequest<'a> {
    model: &'a str,
    messages: Vec<OpenAIMessage<'a>>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Serialize)]
struct OpenAIMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
}

#[derive(Deserialize)]
struct OpenAIChoice {
    message: OpenAIChoiceMessage,
}

#[derive(Deserialize)]
struct OpenAIChoiceMessage {
    content: String,
}

#[async_trait]
impl LlmAdapterTrait for OpenAIAdapter {
    async fn complete(&self, prompt: &str) -> Result<String> {
        let url = format!("{}/chat/completions", self.config.openai_url);
        let req = OpenAIRequest {
            model: &self.config.model,
            messages: vec![
                OpenAIMessage {
                    role: "system",
                    content: "Eres un asistente de gestión documental. Responde basado únicamente en los documentos proporcionados.",
                },
                OpenAIMessage {
                    role: "user",
                    content: prompt,
                },
            ],
            max_tokens: self.config.max_tokens,
            temperature: self.config.temperature,
        };

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.config.api_key)
            .json(&req)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI error {}: {}", status, body);
        }

        let openai_resp: OpenAIResponse = resp.json().await?;
        let content = openai_resp
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default();

        Ok(content.trim().to_string())
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }
}

// ── RetrievalOnly adapter ──────────────────────────────────────────────────────

/// No LLM — just returns the context verbatim (for testing or offline use).
pub struct RetrievalOnlyAdapter;

#[async_trait]
impl LlmAdapterTrait for RetrievalOnlyAdapter {
    async fn complete(&self, prompt: &str) -> Result<String> {
        // Extract the context section from the prompt template
        if let Some(ctx_start) = prompt.find("CONTEXTO:") {
            let ctx = &prompt[ctx_start..];
            if let Some(end) = ctx.find("\n\nPREGUNTA:") {
                return Ok(ctx[9..end].trim().to_string());
            }
        }
        Ok(prompt.to_string())
    }

    fn model_name(&self) -> &str {
        "retrieval-only"
    }
}

// ── Factory ────────────────────────────────────────────────────────────────────

/// Unified LLM adapter type.
pub enum LlmAdapter {
    Ollama(OllamaAdapter),
    OpenAI(OpenAIAdapter),
    RetrievalOnly(RetrievalOnlyAdapter),
}

impl LlmAdapter {
    pub fn from_config(config: LlmConfig) -> Self {
        match &config.backend {
            LlmBackend::Ollama => Self::Ollama(OllamaAdapter::new(config)),
            LlmBackend::OpenAI => Self::OpenAI(OpenAIAdapter::new(config)),
            LlmBackend::RetrievalOnly => Self::RetrievalOnly(RetrievalOnlyAdapter),
        }
    }

    pub fn from_env() -> Self {
        Self::from_config(LlmConfig::from_env())
    }
}

#[async_trait]
impl LlmAdapterTrait for LlmAdapter {
    async fn complete(&self, prompt: &str) -> Result<String> {
        match self {
            Self::Ollama(a) => a.complete(prompt).await,
            Self::OpenAI(a) => a.complete(prompt).await,
            Self::RetrievalOnly(a) => a.complete(prompt).await,
        }
    }

    fn model_name(&self) -> &str {
        match self {
            Self::Ollama(a) => a.model_name(),
            Self::OpenAI(a) => a.model_name(),
            Self::RetrievalOnly(a) => a.model_name(),
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_config_defaults() {
        let cfg = LlmConfig::default();
        assert_eq!(cfg.backend, LlmBackend::Ollama);
        assert_eq!(cfg.model, "llama3.2");
        assert_eq!(cfg.max_tokens, 1024);
    }

    #[tokio::test]
    async fn test_retrieval_only_adapter() {
        let adapter = RetrievalOnlyAdapter;
        let prompt = "CONTEXTO:\nEl artículo 5 dice que...\n\nPREGUNTA: ¿Qué dice el artículo 5?";
        let result = adapter.complete(prompt).await.unwrap();
        assert!(result.contains("artículo 5"));
    }

    #[test]
    fn test_llm_adapter_factory_retrieval_only() {
        let cfg = LlmConfig {
            backend: LlmBackend::RetrievalOnly,
            ..Default::default()
        };
        let adapter = LlmAdapter::from_config(cfg);
        assert_eq!(adapter.model_name(), "retrieval-only");
    }
}
