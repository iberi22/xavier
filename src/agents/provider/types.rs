// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Provider-related type definitions and enums.
//!
//! Defines shared types used across LLM provider implementations,
//! including API flavors, provider modes, and request/response models.

use crate::domain::proxy::types::ProviderQuota;
use serde::Serialize;
use std::time::Duration;

/// Global timeout for LLM provider requests.
pub const LLM_TIMEOUT: Duration = Duration::from_secs(30);

/// Represents the operational mode of a model provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderMode {
    /// Provider is running locally (e.g., Ollama).
    Local,
    /// Managed local llama-server.
    ManagedLocal,
    /// Provider is a cloud-based API (e.g., OpenAI, Anthropic).
    Cloud,
    /// Provider is disabled.
    Disabled,
}

/// Indicates the current reachability status of a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderReachability {
    /// The provider is configured and can be successfully contacted.
    ConfiguredAndReachable,
    /// The provider is configured but unreachable (e.g. network error, timeout).
    ConfiguredAndUnreachable,
    /// The provider is not properly configured.
    NotConfigured,
}

/// Supported API flavors for different model providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiFlavor {
    /// OpenAI-compatible API structure.
    OpenAICompatible,
    /// Anthropic-compatible API structure.
    AnthropicCompatible,
}

impl ApiFlavor {
    pub(crate) fn from_env(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "openai" | "openai-compatible" => Some(Self::OpenAICompatible),
            "anthropic" | "anthropic-compatible" => Some(Self::AnthropicCompatible),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAICompatible => "openai-compatible",
            Self::AnthropicCompatible => "anthropic-compatible",
        }
    }
}

/// Internal target mapping for different API implementation styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderTarget {
    GenericOpenAICompatible,
    AnthropicMessages,
    GeminiLegacy,
    MiniMaxLegacy,
    OpenCodeCLI,
}

/// Response from an LLM provider.
#[derive(Debug, Clone, Serialize)]
pub struct LlmResponse {
    pub text: String,
    pub quota: Option<ProviderQuota>,
}

/// Current status of a model provider.
#[derive(Debug, Clone, Serialize)]
pub struct ModelProviderStatus {
    /// The provider label.
    pub provider: String,
    /// The model being used.
    pub model: String,
    /// Whether the provider is correctly configured.
    pub configured: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_flavor_from_env() {
        assert_eq!(
            ApiFlavor::from_env("openai"),
            Some(ApiFlavor::OpenAICompatible)
        );
        assert_eq!(
            ApiFlavor::from_env("openai-compatible"),
            Some(ApiFlavor::OpenAICompatible)
        );
        assert_eq!(
            ApiFlavor::from_env("anthropic"),
            Some(ApiFlavor::AnthropicCompatible)
        );
        assert_eq!(
            ApiFlavor::from_env("anthropic-compatible"),
            Some(ApiFlavor::AnthropicCompatible)
        );
        assert_eq!(ApiFlavor::from_env("unknown"), None);
    }
}
