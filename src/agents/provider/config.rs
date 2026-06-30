//! Provider configuration and model routing.
//!
//! Defines provider-specific configuration constants, model-to-provider
//! mappings, and helper functions for constructing API endpoints and
//! routing requests to the correct LLM backend.

use crate::agents::provider::types::{ApiFlavor, ProviderMode, ProviderTarget};
use crate::domain::proxy::SecretInjectionStrategy;
use crate::secrets::vault::HardwareVault;

pub(crate) const DEFAULT_LOCAL_BASE_URL: &str = "http://localhost:11434/v1";
pub(crate) const DEFAULT_LOCAL_ANTHROPIC_BASE_URL: &str = "http://localhost:11434";
pub(crate) const DEFAULT_LOCAL_MODEL: &str = "qwen3-coder";
pub(crate) const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
pub(crate) const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com/v1";
pub(crate) const DEFAULT_DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com/v1";
pub(crate) const DEFAULT_MINIMAX_BASE_URL: &str = "https://api.minimax.chat/v1";
pub(crate) const DEFAULT_GROQ_BASE_URL: &str = "https://api.groq.com/openai/v1";
pub(crate) const DEFAULT_ZAI_BASE_URL: &str = "https://api.z.ai/v1";

/// Configuration for model provider key leasing.
#[derive(Debug, Clone)]
pub struct KeyLeaseConfig {
    /// The name of the secret in the vault.
    pub secret_name: String,
    /// The agent requesting the lease.
    pub agent_id: String,
    /// Time-to-live for the lease in seconds.
    pub ttl_secs: u64,
}

/// Configuration for a model provider.
#[derive(Debug, Clone)]
pub struct ModelProviderConfig {
    /// Operational mode (Local, Cloud, Disabled).
    pub provider_mode: ProviderMode,
    /// API flavor (OpenAI, Anthropic).
    pub api_flavor: ApiFlavor,
    /// Human-readable label for the provider.
    pub provider_label: String,
    /// The model name.
    pub model: String,
    /// Optional API key.
    pub api_key: Option<String>,
    /// Optional base URL for the API.
    pub base_url: Option<String>,
    pub(crate) target: ProviderTarget,
    /// Optional configuration for key leasing.
    pub lease_config: Option<KeyLeaseConfig>,
    /// Strategy for injecting the secret into the request.
    pub secret_injection_strategy: Option<SecretInjectionStrategy>,
    /// Optional lease token to include in requests.
    pub lease_token: Option<String>,
}

impl ModelProviderConfig {
    /// Loads the configuration from environment variables.
    pub fn from_env() -> Self {
        let settings = crate::settings::XavierSettings::current();
        let provider = std::env::var("XAVIER_MODEL_PROVIDER")
            .ok()
            .or_else(|| Some(settings.models.provider.clone()))
            .map(|value| value.trim().to_ascii_lowercase());

        Self::from_label(provider.as_deref().unwrap_or("local"))
    }

    /// Creates a configuration from a provider label.
    pub fn from_label(label: &str) -> Self {
        match label.trim().to_ascii_lowercase().as_str() {
            "local" => Self::local_from_env(),
            "cloud" => Self::cloud_from_env(),
            "disabled" => Self::disabled(),
            "anthropic" => Self::anthropic_cloud_from_env(),
            "openai" => Self::openai_cloud_from_env(),
            "deepseek" => Self::deepseek_cloud_from_env(),
            "minimax" => Self::minimax_cloud_from_env(),
            "gemini" => Self::gemini_cloud_from_env(),
            "groq" => Self::groq_cloud_from_env(),
            "z.ai" | "zai" => Self::zai_cloud_from_env(),
            "opencode" => Self::opencode_from_env(),
            _ => Self::local_from_env(),
        }
    }

    /// Creates a configuration for a specific provider.
    pub fn for_provider(provider: &str) -> Self {
        Self::from_label(provider)
    }

    /// Creates a new configuration with explicit parameters.
    pub fn new_with_params(
        provider: &str,
        model: Option<String>,
        api_key: Option<String>,
        base_url: Option<String>,
    ) -> Self {
        Self::from_label(provider)
            .with_model_override(model)
            .with_api_key(api_key)
            .with_base_url(base_url)
    }

    pub(crate) fn local_from_env() -> Self {
        let settings = crate::settings::XavierSettings::current();
        let api_flavor = std::env::var("XAVIER_API_FLAVOR")
            .ok()
            .and_then(|value| ApiFlavor::from_env(&value))
            .unwrap_or_else(|| {
                ApiFlavor::from_env(&settings.models.api_flavor)
                    .unwrap_or(ApiFlavor::OpenAICompatible)
            });

        match api_flavor {
            ApiFlavor::OpenAICompatible => Self {
                provider_mode: ProviderMode::Local,
                api_flavor,
                provider_label: "local".to_string(),
                model: std::env::var("XAVIER_LOCAL_LLM_MODEL")
                    .or_else(|_| std::env::var("XAVIER_LLM_MODEL"))
                    .ok()
                    .or_else(|| Some(settings.models.local_llm_model.clone()))
                    .or_else(|| settings.models.llm_model.clone())
                    .unwrap_or_else(|| DEFAULT_LOCAL_MODEL.to_string()),
                api_key: std::env::var("XAVIER_LOCAL_LLM_API_KEY")
                    .ok()
                    .or_else(|| settings.models.local_llm_api_key.clone())
                    .or_else(|| Some("ollama".to_string())),
                base_url: Some(
                    std::env::var("XAVIER_LOCAL_LLM_URL")
                        .ok()
                        .or_else(|| Some(settings.models.local_llm_url.clone()))
                        .unwrap_or_else(|| DEFAULT_LOCAL_BASE_URL.to_string()),
                ),
                target: ProviderTarget::GenericOpenAICompatible,
                lease_config: None,
                secret_injection_strategy: None,
                lease_token: None,
            },
            ApiFlavor::AnthropicCompatible => Self {
                provider_mode: ProviderMode::Local,
                api_flavor,
                provider_label: "local".to_string(),
                model: std::env::var("XAVIER_LOCAL_LLM_MODEL")
                    .or_else(|_| std::env::var("XAVIER_LLM_MODEL"))
                    .ok()
                    .or_else(|| Some(settings.models.local_llm_model.clone()))
                    .or_else(|| settings.models.llm_model.clone())
                    .unwrap_or_else(|| DEFAULT_LOCAL_MODEL.to_string()),
                api_key: std::env::var("XAVIER_LOCAL_LLM_API_KEY")
                    .ok()
                    .or_else(|| settings.models.local_llm_api_key.clone())
                    .or_else(|| Some("ollama".to_string())),
                base_url: Some(
                    std::env::var("XAVIER_LOCAL_ANTHROPIC_URL")
                        .or_else(|_| std::env::var("XAVIER_LOCAL_LLM_URL"))
                        .ok()
                        .or_else(|| settings.models.local_anthropic_url.clone())
                        .or_else(|| Some(settings.models.local_llm_url.clone()))
                        .unwrap_or_else(|| DEFAULT_LOCAL_ANTHROPIC_BASE_URL.to_string()),
                ),
                target: ProviderTarget::AnthropicMessages,
                lease_config: None,
                secret_injection_strategy: None,
                lease_token: None,
            },
        }
    }

    pub(crate) fn cloud_from_env() -> Self {
        let settings = crate::settings::XavierSettings::current();
        let api_flavor = std::env::var("XAVIER_API_FLAVOR")
            .ok()
            .and_then(|value| ApiFlavor::from_env(&value))
            .unwrap_or_else(|| {
                ApiFlavor::from_env(&settings.models.api_flavor)
                    .unwrap_or(ApiFlavor::OpenAICompatible)
            });

        match api_flavor {
            ApiFlavor::OpenAICompatible => Self {
                provider_mode: ProviderMode::Cloud,
                api_flavor,
                provider_label: "cloud".to_string(),
                model: std::env::var("XAVIER_CLOUD_LLM_MODEL")
                    .or_else(|_| std::env::var("XAVIER_LLM_MODEL"))
                    .ok()
                    .or_else(|| settings.models.cloud_llm_model.clone())
                    .or_else(|| settings.models.llm_model.clone())
                    .unwrap_or_else(|| "gpt-4o-mini".to_string()),
                api_key: std::env::var("XAVIER_LLM_API_KEY")
                    .ok()
                    .or_else(|| std::env::var("OPENAI_API_KEY").ok())
                    .or_else(|| settings.models.llm_api_key.clone()),
                base_url: Some(
                    std::env::var("XAVIER_CLOUD_LLM_URL")
                        .or_else(|_| std::env::var("OPENAI_BASE_URL"))
                        .ok()
                        .or_else(|| settings.models.cloud_llm_url.clone())
                        .unwrap_or_else(|| DEFAULT_OPENAI_BASE_URL.to_string()),
                ),
                target: ProviderTarget::GenericOpenAICompatible,
                lease_config: None,
                secret_injection_strategy: None,
                lease_token: None,
            },
            ApiFlavor::AnthropicCompatible => Self::anthropic_cloud_from_env(),
        }
    }

    pub(crate) fn openai_cloud_from_env() -> Self {
        let settings = crate::settings::XavierSettings::current();
        Self {
            provider_mode: ProviderMode::Cloud,
            api_flavor: ApiFlavor::OpenAICompatible,
            provider_label: "openai".to_string(),
            model: std::env::var("XAVIER_LLM_MODEL")
                .or_else(|_| std::env::var("OPENAI_MODEL"))
                .ok()
                .or_else(|| settings.models.llm_model.clone())
                .unwrap_or_else(|| "gpt-4o-mini".to_string()),
            api_key: std::env::var("OPENAI_API_KEY")
                .ok()
                .or_else(|| {
                    HardwareVault::new("xavier")
                        .get_secret("OPENAI_API_KEY")
                        .ok()
                })
                .or_else(|| settings.models.llm_api_key.clone()),
            base_url: Some(
                std::env::var("OPENAI_BASE_URL")
                    .ok()
                    .or_else(|| settings.models.cloud_llm_url.clone())
                    .unwrap_or_else(|| DEFAULT_OPENAI_BASE_URL.to_string()),
            ),
            target: ProviderTarget::GenericOpenAICompatible,
            lease_config: None,
            secret_injection_strategy: None,
            lease_token: None,
        }
    }

    pub(crate) fn groq_cloud_from_env() -> Self {
        let settings = crate::settings::XavierSettings::current();
        Self {
            provider_mode: ProviderMode::Cloud,
            api_flavor: ApiFlavor::OpenAICompatible,
            provider_label: "groq".to_string(),
            model: std::env::var("XAVIER_LLM_MODEL")
                .or_else(|_| std::env::var("GROQ_MODEL"))
                .ok()
                .or_else(|| settings.models.llm_model.clone())
                .unwrap_or_else(|| "llama-3.3-70b-versatile".to_string()),
            api_key: std::env::var("GROQ_API_KEY")
                .ok()
                .or_else(|| HardwareVault::new("xavier").get_secret("GROQ_API_KEY").ok())
                .or_else(|| settings.models.llm_api_key.clone()),
            base_url: Some(
                std::env::var("GROQ_BASE_URL")
                    .ok()
                    .unwrap_or_else(|| DEFAULT_GROQ_BASE_URL.to_string()),
            ),
            target: ProviderTarget::GenericOpenAICompatible,
            lease_config: None,
            secret_injection_strategy: None,
            lease_token: None,
        }
    }

    pub(crate) fn deepseek_cloud_from_env() -> Self {
        let settings = crate::settings::XavierSettings::current();
        Self {
            provider_mode: ProviderMode::Cloud,
            api_flavor: ApiFlavor::OpenAICompatible,
            provider_label: "deepseek".to_string(),
            model: std::env::var("XAVIER_LLM_MODEL")
                .or_else(|_| std::env::var("DEEPSEEK_MODEL"))
                .ok()
                .or_else(|| settings.models.llm_model.clone())
                .unwrap_or_else(|| "deepseek-chat".to_string()),
            api_key: std::env::var("DEEPSEEK_API_KEY")
                .ok()
                .or_else(|| {
                    HardwareVault::new("xavier")
                        .get_secret("DEEPSEEK_API_KEY")
                        .ok()
                })
                .or_else(|| settings.models.llm_api_key.clone()),
            base_url: Some(
                std::env::var("DEEPSEEK_BASE_URL")
                    .ok()
                    .unwrap_or_else(|| DEFAULT_DEEPSEEK_BASE_URL.to_string()),
            ),
            target: ProviderTarget::GenericOpenAICompatible,
            lease_config: None,
            secret_injection_strategy: None,
            lease_token: None,
        }
    }

    /// z.ai (GLM) provider configuration.
    /// Endpoint: https://api.z.ai/api/coding/paas/v4 (OpenAI-compatible)
    /// Models: glm-5.2, glm-4.7, glm-5-turbo
    pub(crate) fn anthropic_cloud_from_env() -> Self {
        let settings = crate::settings::XavierSettings::current();
        Self {
            provider_mode: ProviderMode::Cloud,
            api_flavor: ApiFlavor::AnthropicCompatible,
            provider_label: "anthropic".to_string(),
            model: std::env::var("XAVIER_LLM_MODEL")
                .or_else(|_| std::env::var("ANTHROPIC_MODEL"))
                .ok()
                .or_else(|| settings.models.llm_model.clone())
                .unwrap_or_else(|| "claude-3-5-sonnet-latest".to_string()),
            api_key: std::env::var("ANTHROPIC_API_KEY")
                .ok()
                .or_else(|| {
                    HardwareVault::new("xavier")
                        .get_secret("ANTHROPIC_API_KEY")
                        .ok()
                })
                .or_else(|| std::env::var("XAVIER_LLM_API_KEY").ok())
                .or_else(|| settings.models.llm_api_key.clone()),
            base_url: Some(
                std::env::var("ANTHROPIC_BASE_URL")
                    .or_else(|_| std::env::var("XAVIER_CLOUD_LLM_URL"))
                    .ok()
                    .or_else(|| settings.models.cloud_llm_url.clone())
                    .unwrap_or_else(|| DEFAULT_ANTHROPIC_BASE_URL.to_string()),
            ),
            target: ProviderTarget::AnthropicMessages,
            lease_config: None,
            secret_injection_strategy: None,
            lease_token: None,
        }
    }

    pub(crate) fn minimax_cloud_from_env() -> Self {
        let settings = crate::settings::XavierSettings::current();
        Self {
            provider_mode: ProviderMode::Cloud,
            api_flavor: ApiFlavor::OpenAICompatible,
            provider_label: "minimax".to_string(),
            model: std::env::var("XAVIER_LLM_MODEL")
                .or_else(|_| std::env::var("MINIMAX_MODEL"))
                .ok()
                .or_else(|| settings.models.llm_model.clone())
                .unwrap_or_else(|| "MiniMax-Text-01".to_string()),
            api_key: std::env::var("MINIMAX_API_KEY")
                .ok()
                .or_else(|| {
                    HardwareVault::new("xavier")
                        .get_secret("MINIMAX_API_KEY")
                        .ok()
                })
                .or_else(|| settings.models.llm_api_key.clone()),
            base_url: Some(
                std::env::var("MINIMAX_BASE_URL")
                    .ok()
                    .unwrap_or_else(|| DEFAULT_MINIMAX_BASE_URL.to_string()),
            ),
            target: ProviderTarget::MiniMaxLegacy,
            lease_config: None,
            secret_injection_strategy: None,
            lease_token: None,
        }
    }

    pub(crate) fn gemini_cloud_from_env() -> Self {
        let settings = crate::settings::XavierSettings::current();
        Self {
            provider_mode: ProviderMode::Cloud,
            api_flavor: ApiFlavor::OpenAICompatible,
            provider_label: "gemini".to_string(),
            model: std::env::var("XAVIER_LLM_MODEL")
                .or_else(|_| std::env::var("GEMINI_MODEL"))
                .ok()
                .or_else(|| settings.models.llm_model.clone())
                .unwrap_or_else(|| "gemini-2.0-flash".to_string()),
            api_key: std::env::var("GEMINI_API_KEY")
                .ok()
                .or_else(|| {
                    HardwareVault::new("xavier")
                        .get_secret("GEMINI_API_KEY")
                        .ok()
                })
                .or_else(|| settings.models.llm_api_key.clone()),
            base_url: None,
            target: ProviderTarget::GeminiLegacy,
            lease_config: None,
            secret_injection_strategy: None,
            lease_token: None,
        }
    }

    pub(crate) fn zai_cloud_from_env() -> Self {
        let settings = crate::settings::XavierSettings::current();
        Self {
            provider_mode: ProviderMode::Cloud,
            api_flavor: ApiFlavor::OpenAICompatible,
            provider_label: "z.ai".to_string(),
            model: std::env::var("XAVIER_ZAI_MODEL")
                .or_else(|_| std::env::var("ZAI_MODEL"))
                .ok()
                .or_else(|| settings.models.zai_model.clone())
                .unwrap_or_else(|| "glm-5.1".to_string()),
            api_key: std::env::var("ZAI_API_KEY")
                .ok()
                .or_else(|| HardwareVault::new("xavier").get_secret("ZAI_API_KEY").ok())
                .or_else(|| settings.models.zai_api_key.clone()),
            base_url: Some(DEFAULT_ZAI_BASE_URL.to_string()),
            target: ProviderTarget::GenericOpenAICompatible,
            lease_config: None,
            secret_injection_strategy: None,
            lease_token: None,
        }
    }

    pub(crate) fn opencode_from_env() -> Self {
        let settings = crate::settings::XavierSettings::current();
        Self {
            provider_mode: ProviderMode::Local,
            api_flavor: ApiFlavor::OpenAICompatible,
            provider_label: "opencode".to_string(),
            model: std::env::var("XAVIER_OPENCODE_MODEL")
                .or_else(|_| std::env::var("OPENCODE_MODEL"))
                .ok()
                .or_else(|| settings.models.opencode_model.clone())
                .unwrap_or_else(|| "opencode/deepseek-v4-flash".to_string()),
            api_key: std::env::var("OPENCODE_API_KEY")
                .ok()
                .or_else(|| {
                    HardwareVault::new("xavier")
                        .get_secret("OPENCODE_API_KEY")
                        .ok()
                })
                .or_else(|| {
                    HardwareVault::new("xavier")
                        .get_secret("ZAI_API_KEY")
                        .ok()
                }),
            base_url: None,
            target: ProviderTarget::OpenCodeCLI,
            lease_config: None,
            secret_injection_strategy: None,
            lease_token: None,
        }
    }

    pub(crate) fn disabled() -> Self {
        Self {
            provider_mode: ProviderMode::Disabled,
            api_flavor: ApiFlavor::OpenAICompatible,
            provider_label: "disabled".to_string(),
            model: "disabled".to_string(),
            api_key: None,
            base_url: None,
            target: ProviderTarget::GenericOpenAICompatible,
            lease_config: None,
            secret_injection_strategy: None,
            lease_token: None,
        }
    }

    /// Checks if the provider is correctly configured.
    pub fn is_configured(&self) -> bool {
        match self.provider_mode {
            ProviderMode::Disabled => false,
            ProviderMode::Local => {
                if self.target == ProviderTarget::OpenCodeCLI {
                    return true;
                }
                self.base_url
                    .as_ref()
                    .is_some_and(|value| !value.trim().is_empty())
            }
            ProviderMode::Cloud => {
                let has_url = self
                    .base_url
                    .as_ref()
                    .is_some_and(|value| !value.trim().is_empty());
                let has_key = self
                    .api_key
                    .as_ref()
                    .is_some_and(|value| !value.trim().is_empty());
                let has_lease = self.lease_config.is_some();

                has_url && (has_key || has_lease)
            }
        }
    }

    /// Returns a list of all configured providers.
    pub fn get_all_configured() -> Vec<Self> {
        let mut configured = Vec::new();
        for config in [
            Self::local_from_env(),
            Self::openai_cloud_from_env(),
            Self::anthropic_cloud_from_env(),
            Self::deepseek_cloud_from_env(),
            Self::minimax_cloud_from_env(),
            Self::gemini_cloud_from_env(),
            Self::groq_cloud_from_env(),
            Self::zai_cloud_from_env(),
            Self::opencode_from_env(),
        ] {
            if config.is_configured() {
                configured.push(config);
            }
        }
        configured
    }

    /// Overrides the model name.
    pub fn with_model_override(mut self, model_override: Option<String>) -> Self {
        if let Some(model) = model_override
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            self.model = model;
        }
        self
    }

    /// Sets an explicit API key.
    pub fn with_api_key(mut self, api_key: Option<String>) -> Self {
        if let Some(key) = api_key.filter(|v| !v.trim().is_empty()) {
            self.api_key = Some(key);
        }
        self
    }

    /// Sets an explicit base URL.
    pub fn with_base_url(mut self, base_url: Option<String>) -> Self {
        if let Some(url) = base_url.filter(|v| !v.trim().is_empty()) {
            self.base_url = Some(url);
        }
        self
    }

    /// Sets key lease configuration.
    pub fn with_key_lease(mut self, secret_name: &str, agent_id: &str, ttl_secs: u64) -> Self {
        self.lease_config = Some(KeyLeaseConfig {
            secret_name: secret_name.to_string(),
            agent_id: agent_id.to_string(),
            ttl_secs,
        });
        self
    }

    /// Sets secret injection strategy.
    pub fn with_secret_injection_strategy(mut self, strategy: SecretInjectionStrategy) -> Self {
        self.secret_injection_strategy = Some(strategy);
        self
    }

    /// Internal method to set the lease token.
    pub(crate) fn with_lease_token(mut self, token: Option<String>) -> Self {
        self.lease_token = token;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::provider::types::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn test_local_provider_config() {
        let _guard = env_lock().lock().expect("test assertion");
        std::env::set_var("XAVIER_LOCAL_LLM_MODEL", "test-model");
        std::env::remove_var("XAVIER_LLM_MODEL");
        std::env::set_var("XAVIER_LOCAL_LLM_URL", "http://test-url/v1");
        std::env::remove_var("XAVIER_API_FLAVOR");

        let config = ModelProviderConfig::local_from_env();

        assert_eq!(config.model, "test-model");
        assert_eq!(config.base_url, Some("http://test-url/v1".to_string()));
        assert_eq!(config.provider_mode, ProviderMode::Local);
        assert_eq!(config.api_flavor, ApiFlavor::OpenAICompatible);
    }

    #[test]
    fn test_local_provider_defaults() {
        let _guard = env_lock().lock().expect("test assertion");
        std::env::remove_var("XAVIER_LOCAL_LLM_MODEL");
        std::env::remove_var("XAVIER_LLM_MODEL");
        std::env::remove_var("XAVIER_LOCAL_LLM_URL");
        std::env::remove_var("XAVIER_API_FLAVOR");

        let config = ModelProviderConfig::local_from_env();

        assert_eq!(config.model, DEFAULT_LOCAL_MODEL);
        assert_eq!(config.base_url, Some(DEFAULT_LOCAL_BASE_URL.to_string()));
        assert_eq!(config.api_key.as_deref(), Some("ollama"));
    }

    #[test]
    fn test_groq_provider_config() {
        let _guard = env_lock().lock().expect("test assertion");
        std::env::set_var("GROQ_API_KEY", "gsk_test");

        let config = ModelProviderConfig::groq_cloud_from_env();

        assert_eq!(config.provider_label, "groq");
        assert_eq!(config.api_key.as_deref(), Some("gsk_test"));
        assert_eq!(config.base_url, Some(DEFAULT_GROQ_BASE_URL.to_string()));
    }

    #[test]
    fn test_model_provider_config_from_label() {
        let config = ModelProviderConfig::from_label("openai");
        assert_eq!(config.provider_label, "openai");
        assert_eq!(config.provider_mode, ProviderMode::Cloud);

        let config = ModelProviderConfig::from_label("anthropic");
        assert_eq!(config.provider_label, "anthropic");
        assert_eq!(config.provider_mode, ProviderMode::Cloud);

        let config = ModelProviderConfig::from_label("disabled");
        assert_eq!(config.provider_mode, ProviderMode::Disabled);
    }
}
