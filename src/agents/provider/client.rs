//! HTTP client abstraction for LLM provider API calls.
//!
//! Provides a shared reqwest client with configurable timeouts,
//! retry logic, and connection management for all provider backends.

use anyhow::{anyhow, Result};
use reqwest::Client;
use std::time::Duration;
use tokio::time::timeout;
use tracing::warn;
use async_trait::async_trait;

use crate::agents::system1::RetrievedDocument;
use crate::agents::provider::config::ModelProviderConfig;
use crate::agents::provider::types::{ModelProviderStatus, ProviderMode, ProviderTarget, LLM_TIMEOUT};
pub use crate::agents::provider::traits::LlmProvider;
use crate::agents::provider::openai::generate_openai_compatible;
use crate::agents::provider::anthropic::generate_anthropic_compatible;
use crate::agents::provider::gemini::generate_gemini_legacy;
use crate::agents::provider::minimax::generate_minimax_legacy;

/// Client for interacting with various model providers.
#[derive(Clone)]
pub struct ModelProviderClient {
    pub(crate) client: Client,
    pub(crate) config: ModelProviderConfig,
}

impl ModelProviderClient {
    /// Creates a new ModelProviderClient with the given configuration.
    pub fn new(config: ModelProviderConfig) -> Self {
        Self {
            client: Client::builder()
                .connect_timeout(Duration::from_secs(2))
                .timeout(Duration::from_secs(30))
                .build()
                .expect("model provider HTTP client"),
            config,
        }
    }

    /// Creates a client from environment variables.
    pub fn from_env() -> Self {
        Self::from_model_override(None)
    }

    /// Creates a client from environment variables with a model override.
    pub fn from_model_override(model_override: Option<String>) -> Self {
        Self::new(ModelProviderConfig::from_env().with_model_override(model_override))
    }

    /// Creates a client for a specific provider.
    pub fn for_provider(provider: &str, model_override: Option<String>) -> Self {
        Self::new(ModelProviderConfig::for_provider(provider).with_model_override(model_override))
    }

    /// Returns the current status of the provider.
    pub fn status(&self) -> ModelProviderStatus {
        ModelProviderStatus {
            provider: if self.config.provider_mode == ProviderMode::Disabled {
                "disabled".to_string()
            } else {
                format!(
                    "{}:{}",
                    self.config.provider_label,
                    self.config.api_flavor.as_str()
                )
            },
            model: self.config.model.clone(),
            configured: self.config.is_configured(),
        }
    }

    /// Generates text with optional caching.
    pub async fn generate_text_with_cache(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        use_cache: bool,
    ) -> Result<String> {
        if !self.config.is_configured() || self.config.provider_mode == ProviderMode::Disabled {
            return Err(anyhow!("no LLM provider configured"));
        }

        let future = async {
            match self.config.target {
                ProviderTarget::GenericOpenAICompatible => {
                    generate_openai_compatible(&self.client, &self.config, system_prompt, user_prompt, use_cache)
                        .await
                }
                ProviderTarget::AnthropicMessages => {
                    generate_anthropic_compatible(&self.client, &self.config, system_prompt, user_prompt, use_cache)
                        .await
                }
                ProviderTarget::GeminiLegacy => {
                    generate_gemini_legacy(&self.client, &self.config, system_prompt, user_prompt, use_cache)
                        .await
                }
                ProviderTarget::MiniMaxLegacy => {
                    generate_minimax_legacy(&self.client, &self.config, system_prompt, user_prompt, use_cache)
                        .await
                }
            }
        };

        timeout(LLM_TIMEOUT, future).await.map_err(|_| {
            warn!("LLM provider timed out after {}s", LLM_TIMEOUT.as_secs());
            anyhow!(
                "LLM provider timed out after {} seconds",
                LLM_TIMEOUT.as_secs()
            )
        })?
    }

    pub async fn generate_response(
        &self,
        query: &str,
        context: &[RetrievedDocument],
    ) -> Result<String> {
        <Self as LlmProvider>::generate_response(self, query, context).await
    }

    pub async fn generate_hypothetical_document(&self, query: &str) -> Result<String> {
        <Self as LlmProvider>::generate_hypothetical_document(self, query).await
    }

    pub async fn evaluate_context(
        &self,
        query: &str,
        context: &[RetrievedDocument],
    ) -> Result<f32> {
        <Self as LlmProvider>::evaluate_context(self, query, context).await
    }

    pub async fn generate_text(&self, system_prompt: &str, user_prompt: &str) -> Result<String> {
        <Self as LlmProvider>::generate_text(self, system_prompt, user_prompt, false).await
    }
}

#[async_trait]
impl LlmProvider for ModelProviderClient {
    async fn generate_text(&self, system_prompt: &str, user_prompt: &str, use_cache: bool) -> Result<String> {
        self.generate_text_with_cache(system_prompt, user_prompt, use_cache).await
    }

    async fn generate_response(
        &self,
        query: &str,
        context: &[RetrievedDocument],
    ) -> Result<String> {
        let system_prompt = "You are a helpful AI assistant part of the Xavier memory system. Use the provided memory context accurately. If the context is insufficient, say so clearly. Be concise but informative.";
        let context_text = context
            .iter()
            .map(|doc| format!("- {}\n  Source: {}", doc.content, doc.path))
            .collect::<Vec<_>>()
            .join("\n\n");
        let mut user_prompt = format!(
            "Context from memory:\n{}\n\nUser question: {}",
            context_text, query
        );

        // Special tool execution wrapper for DeepSeek (as per requirement)
        if self.config.provider_label == "deepseek" {
            user_prompt = format!(
                "{}\n\n[TOOL_INSTRUCTION] If you need to perform actions, describe them using this format: TOOL: <tool_name> ARGS: <json_arguments>. If you can answer directly, just provide the answer.",
                user_prompt
            );
        }

        <Self as LlmProvider>::generate_text(self, system_prompt, &user_prompt, false).await
    }

    async fn generate_hypothetical_document(&self, query: &str) -> Result<String> {
        let system_prompt = "You are an expert knowledge system. Generate a hypothetical, highly plausible document snippet or answer that directly addresses the user's query. Do not include introductory or concluding remarks. Write only the factual content as if it were a real, authoritative reference document.";
        <Self as LlmProvider>::generate_text(self, system_prompt, query, false).await
    }

    async fn evaluate_context(
        &self,
        query: &str,
        context: &[RetrievedDocument],
    ) -> Result<f32> {
        if !self.config.is_configured() || self.config.provider_mode == ProviderMode::Disabled {
            return Ok(1.0);
        }

        let system_prompt = "You are a critical evaluator for a RAG system. Read the context and the user query. Evaluate if the context contains sufficient and accurate information to fully answer the query. Return ONLY a valid JSON object in this exact format: {\"confidence\": 0.95} where confidence is a float between 0.0 (useless) and 1.0 (perfect).";

        let context_text = context
            .iter()
            .map(|doc| format!("- {}", doc.content))
            .collect::<Vec<_>>()
            .join("\n\n");

        let user_prompt = format!("Context:\n{}\n\nQuery: {}", context_text, query);
        let response = <Self as LlmProvider>::generate_text(self, system_prompt, &user_prompt, false).await?;

        let normalized = response.replace("```json", "").replace("```", "");
        let result: serde_json::Value = serde_json::from_str(normalized.trim())
            .unwrap_or_else(|_| serde_json::json!({"confidence": 1.0}));

        Ok(result["confidence"].as_f64().unwrap_or(1.0) as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn test_model_provider_respects_override() {
        let _guard = env_lock().lock().expect("test assertion");
        std::env::set_var("XAVIER_MODEL_PROVIDER", "openai");
        std::env::set_var("OPENAI_API_KEY", "sk-test");

        let client = ModelProviderClient::from_model_override(Some("gpt-4o-override".to_string()));
        let status = client.status();

        assert_eq!(status.model, "gpt-4o-override");
        assert_eq!(client.config.model, "gpt-4o-override");
    }

    #[tokio::test]
    async fn test_llm_timeout() {
        use tokio::net::TcpListener;
        use crate::agents::provider::types::ApiFlavor;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Spawn a task that accepts one connection but never responds
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                // Keep the connection open but don't send anything
                tokio::time::sleep(Duration::from_secs(40)).await;
                use tokio::io::AsyncWriteExt;
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                    .await;
            }
        });

        let config = ModelProviderConfig {
            provider_mode: ProviderMode::Local,
            api_flavor: ApiFlavor::OpenAICompatible,
            provider_label: "test-timeout".to_string(),
            model: "test-model".to_string(),
            api_key: None,
            base_url: Some(format!("http://{}", addr)),
            target: ProviderTarget::GenericOpenAICompatible,
        };
        let client = ModelProviderClient::new(config);

        let start = std::time::Instant::now();
        let result = client.generate_text("system", "user").await;
        let elapsed = start.elapsed();

        assert!(result.is_err(), "Expected error but got: {:?}", result);
        let err_msg = format!("{:?}", result.err().unwrap());
        assert!(
            err_msg.contains("timed out") || err_msg.contains("timeout"),
            "Error message '{}' did not contain 'timed out' or 'timeout'",
            err_msg
        );
        // Should be around 30s
        assert!(
            elapsed.as_secs() >= 30 && elapsed.as_secs() < 40,
            "Elapsed time was {}s, expected around 30s",
            elapsed.as_secs()
        );
    }
}
