//! Domain model for proxy configuration
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyChatCommand {
    pub model: String,
    pub messages: Vec<serde_json::Value>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletion {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatChoice {
    pub index: usize,
    pub message: ChatMessage,
    pub finish_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericProxyRequest {
    pub url: String,
    pub method: String,
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub body: Option<serde_json::Value>,
    /// Optional lease token to resolve and inject as a secret.
    /// If provided, the resolved secret will be injected based on `secret_injection_strategy`.
    pub lease_token: Option<String>,
    pub secret_injection_strategy: Option<SecretInjectionStrategy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretInjectionStrategy {
    /// Injects "Bearer <secret>" into Authorization header.
    BearerToken,
    /// Injects the raw secret into X-API-Key header.
    XApiKey,
    /// Injects "token <secret>" into Authorization header (GitHub style).
    GitHubToken,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericProxyResponse {
    pub status: u16,
    pub headers: std::collections::HashMap<String, String>,
    pub body: serde_json::Value,
}

pub mod types;
pub use types::*;

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("All providers are rate-limited")]
    RateLimited,
    #[error("Provider error: {0}")]
    ProviderError(String),
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Secret resolution failed: {0}")]
    SecretError(String),
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
}
