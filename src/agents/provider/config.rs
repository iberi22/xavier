//! Provider configuration and model routing.
//!
//! Defines provider-specific configuration constants, model-to-provider
//! mappings, and helper functions for constructing API endpoints and
//! routing requests to the correct LLM backend.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use crate::agents::provider::types::{
    ApiFlavor, ProviderMode, ProviderReachability, ProviderTarget,
};
use crate::domain::proxy::SecretInjectionStrategy;
use crate::secrets::vault::HardwareVault;

/// Cached probe result: (reachable, model_listed, probed_at).
type ReachabilityEntry = (bool, Option<bool>, Instant);

static REACHABILITY_CACHE: LazyLock<Mutex<HashMap<String, ReachabilityEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) const DEFAULT_LOCAL_BASE_URL: &str = "http://127.0.0.1:11435/v1";
pub(crate) const DEFAULT_LOCAL_ANTHROPIC_BASE_URL: &str = "http://localhost:11434";
pub(crate) const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
pub(crate) const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com/v1";
pub(crate) const DEFAULT_DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com/v1";
pub(crate) const DEFAULT_MINIMAX_BASE_URL: &str = "https://api.minimax.chat/v1";
pub(crate) const DEFAULT_GROQ_BASE_URL: &str = "https://api.groq.com/openai/v1";
pub(crate) const DEFAULT_ZAI_BASE_URL: &str = "https://api.z.ai/v1";

/// Read an env var, treating missing *and* blank values as unset.
///
/// CI deterministic-gate: several `local_from_env` tests mutate global ENV.
/// A blank `XAVIER_*` left over from another test (or from the runner
/// environment) must not shadow the settings/default fallback chain.
fn nonempty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

/// Clone a `String` settings field, treating blank values as unset.
fn nonempty_str(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Configuration for model provider key leasing.
#[derive(Clone)]
pub struct KeyLeaseConfig {
    /// The name of the secret in the vault.
    pub secret_name: String,
    /// The agent requesting the lease.
    pub agent_id: String,
    /// Time-to-live for the lease in seconds.
    pub ttl_secs: u64,
}

impl std::fmt::Debug for KeyLeaseConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyLeaseConfig")
            .field("secret_name", &self.secret_name)
            .field("agent_id", &self.agent_id)
            .field("ttl_secs", &self.ttl_secs)
            .finish()
    }
}

/// Configuration for a model provider.
#[derive(Clone)]
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

impl std::fmt::Debug for ModelProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redacted_key = self.api_key.as_ref().map(|_| "<redacted>");
        let redacted_token = self.lease_token.as_ref().map(|_| "<redacted>");
        f.debug_struct("ModelProviderConfig")
            .field("provider_mode", &self.provider_mode)
            .field("api_flavor", &self.api_flavor)
            .field("provider_label", &self.provider_label)
            .field("model", &self.model)
            .field("api_key", &redacted_key)
            .field("base_url", &self.base_url)
            .field("target", &self.target)
            .field("lease_config", &self.lease_config)
            .field("secret_injection_strategy", &self.secret_injection_strategy)
            .field("lease_token", &redacted_token)
            .finish()
    }
}

/// Result of probing an LLM provider endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmProbe {
    pub reachability: ProviderReachability,
    pub model_listed: Option<bool>,
}

impl ModelProviderConfig {
    /// Loads the configuration from environment variables.
    pub fn from_env() -> Self {
        Self::from_label(&Self::resolve_provider_label())
    }

    /// Resolves the selected provider label without building the full config
    /// (no API key or vault lookups), so status checks can call it cheaply.
    ///
    /// Precedence: `XAVIER_MODEL_PROVIDER`, then the deprecated `XAVIER_PROVIDER`,
    /// then OpenRouter when its key is set, then `models.provider` from settings,
    /// then `local`.
    pub fn resolve_provider_label() -> String {
        nonempty_env("XAVIER_MODEL_PROVIDER")
            .or_else(|| nonempty_env("XAVIER_PROVIDER"))
            .or_else(|| {
                if std::env::var("XAVIER_OPENROUTER_API_KEY").is_ok()
                    || std::env::var("OPENROUTER_API_KEY").is_ok()
                {
                    Some("openrouter".to_string())
                } else {
                    // Vault-free lookup: avoid initializing global settings which triggers
                    // HardwareVault (keyring) lookups for Telegram secrets.
                    crate::settings::configured_model_provider()
                }
            })
            .map(|value| value.trim().to_ascii_lowercase())
            .unwrap_or_else(|| "local".to_string())
    }

    /// Checks if a provider label is recognized.
    pub fn is_known_label(label: &str) -> bool {
        matches!(
            label.trim().to_ascii_lowercase().as_str(),
            "local"
                | "managedlocal"
                | "managed-local"
                | "managed_local"
                | "cloud"
                | "disabled"
                | "anthropic"
                | "openai"
                | "deepseek"
                | "minimax"
                | "gemini"
                | "groq"
                | "z.ai"
                | "zai"
                | "opencode"
                | "openrouter"
        )
    }

    /// Creates a configuration from a provider label.
    pub fn from_label(label: &str) -> Self {
        match label.trim().to_ascii_lowercase().as_str() {
            "local" => Self::local_from_env(),
            "managedlocal" | "managed-local" | "managed_local" => Self::managed_local_from_env(),
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
            "openrouter" => Self::openrouter_cloud_from_env(),
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

    /// Local from env.
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
                model: nonempty_env("XAVIER_LOCAL_LLM_MODEL")
                    .or_else(|| nonempty_env("XAVIER_LLM_MODEL"))
                    .or_else(|| nonempty_str(&settings.models.local_llm_model))
                    .or_else(|| {
                        settings
                            .models
                            .llm_model
                            .clone()
                            .filter(|s| !s.trim().is_empty())
                    })
                    .unwrap_or_default(),
                api_key: nonempty_env("XAVIER_LOCAL_LLM_API_KEY")
                    .or_else(|| {
                        settings
                            .models
                            .local_llm_api_key
                            .clone()
                            .filter(|s| !s.trim().is_empty())
                    })
                    .or_else(|| {
                        HardwareVault::new("xavier")
                            .get_secret("XAVIER_LOCAL_LLM_API_KEY")
                            .ok()
                    })
                    .or_else(|| Some("ollama".to_string())),
                base_url: Some(
                    nonempty_env("XAVIER_LOCAL_LLM_URL")
                        .or_else(|| nonempty_str(&settings.models.local_llm_url))
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
                model: nonempty_env("XAVIER_LOCAL_LLM_MODEL")
                    .or_else(|| nonempty_env("XAVIER_LLM_MODEL"))
                    .or_else(|| nonempty_str(&settings.models.local_llm_model))
                    .or_else(|| {
                        settings
                            .models
                            .llm_model
                            .clone()
                            .filter(|s| !s.trim().is_empty())
                    })
                    .unwrap_or_default(),
                api_key: nonempty_env("XAVIER_LOCAL_LLM_API_KEY")
                    .or_else(|| {
                        settings
                            .models
                            .local_llm_api_key
                            .clone()
                            .filter(|s| !s.trim().is_empty())
                    })
                    .or_else(|| {
                        HardwareVault::new("xavier")
                            .get_secret("XAVIER_LOCAL_LLM_API_KEY")
                            .ok()
                    })
                    .or_else(|| Some("ollama".to_string())),
                base_url: Some(
                    nonempty_env("XAVIER_LOCAL_ANTHROPIC_URL")
                        .or_else(|| nonempty_env("XAVIER_LOCAL_LLM_URL"))
                        .or_else(|| {
                            settings
                                .models
                                .local_anthropic_url
                                .clone()
                                .filter(|s| !s.trim().is_empty())
                        })
                        .or_else(|| nonempty_str(&settings.models.local_llm_url))
                        .unwrap_or_else(|| DEFAULT_LOCAL_ANTHROPIC_BASE_URL.to_string()),
                ),
                target: ProviderTarget::AnthropicMessages,
                lease_config: None,
                secret_injection_strategy: None,
                lease_token: None,
            },
        }
    }

    /// Managed local from env.
    pub(crate) fn managed_local_from_env() -> Self {
        let settings = crate::settings::XavierSettings::current();
        let api_flavor = std::env::var("XAVIER_API_FLAVOR")
            .ok()
            .and_then(|value| ApiFlavor::from_env(&value))
            .unwrap_or_else(|| {
                ApiFlavor::from_env(&settings.models.api_flavor)
                    .unwrap_or(ApiFlavor::OpenAICompatible)
            });

        Self {
            provider_mode: ProviderMode::ManagedLocal,
            api_flavor,
            provider_label: "managed-local".to_string(),
            model: nonempty_env("XAVIER_LOCAL_LLM_MODEL")
                .or_else(|| nonempty_env("XAVIER_LLM_MODEL"))
                .or_else(|| nonempty_str(&settings.models.local_llm_model))
                .or_else(|| {
                    settings
                        .models
                        .llm_model
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                })
                .unwrap_or_default(),
            api_key: nonempty_env("XAVIER_LOCAL_LLM_API_KEY")
                .or_else(|| {
                    settings
                        .models
                        .local_llm_api_key
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                })
                .or_else(|| {
                    HardwareVault::new("xavier")
                        .get_secret("XAVIER_LOCAL_LLM_API_KEY")
                        .ok()
                })
                .or_else(|| Some("ollama".to_string())),
            base_url: Some(
                nonempty_env("XAVIER_LOCAL_LLM_URL")
                    .or_else(|| nonempty_str(&settings.models.local_llm_url))
                    .unwrap_or_else(|| DEFAULT_LOCAL_BASE_URL.to_string()),
            ),
            target: ProviderTarget::GenericOpenAICompatible,
            lease_config: None,
            secret_injection_strategy: None,
            lease_token: None,
        }
    }

    /// Cloud from env.
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
                model: nonempty_env("XAVIER_CLOUD_LLM_MODEL")
                    .or_else(|| nonempty_env("XAVIER_LLM_MODEL"))
                    .or_else(|| {
                        settings
                            .models
                            .cloud_llm_model
                            .clone()
                            .filter(|s| !s.trim().is_empty())
                    })
                    .or_else(|| {
                        settings
                            .models
                            .llm_model
                            .clone()
                            .filter(|s| !s.trim().is_empty())
                    })
                    .unwrap_or_default(),
                api_key: nonempty_env("XAVIER_LLM_API_KEY")
                    .or_else(|| nonempty_env("OPENAI_API_KEY"))
                    .or_else(|| {
                        settings
                            .models
                            .llm_api_key
                            .clone()
                            .filter(|s| !s.trim().is_empty())
                    }),
                base_url: Some(
                    nonempty_env("XAVIER_CLOUD_LLM_URL")
                        .or_else(|| nonempty_env("OPENAI_BASE_URL"))
                        .or_else(|| {
                            settings
                                .models
                                .cloud_llm_url
                                .clone()
                                .filter(|s| !s.trim().is_empty())
                        })
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

    /// Openai cloud from env.
    pub(crate) fn openai_cloud_from_env() -> Self {
        let settings = crate::settings::XavierSettings::current();
        Self {
            provider_mode: ProviderMode::Cloud,
            api_flavor: ApiFlavor::OpenAICompatible,
            provider_label: "openai".to_string(),
            model: nonempty_env("XAVIER_LLM_MODEL")
                .or_else(|| nonempty_env("OPENAI_MODEL"))
                .or_else(|| {
                    settings
                        .models
                        .llm_model
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                })
                .unwrap_or_default(),
            api_key: nonempty_env("OPENAI_API_KEY")
                .or_else(|| {
                    HardwareVault::new("xavier")
                        .get_secret("OPENAI_API_KEY")
                        .ok()
                        .filter(|s| !s.trim().is_empty())
                })
                .or_else(|| {
                    settings
                        .models
                        .llm_api_key
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                }),
            base_url: Some(
                nonempty_env("OPENAI_BASE_URL")
                    .or_else(|| {
                        settings
                            .models
                            .cloud_llm_url
                            .clone()
                            .filter(|s| !s.trim().is_empty())
                    })
                    .unwrap_or_else(|| DEFAULT_OPENAI_BASE_URL.to_string()),
            ),
            target: ProviderTarget::GenericOpenAICompatible,
            lease_config: None,
            secret_injection_strategy: None,
            lease_token: None,
        }
    }

    /// OpenRouter cloud from env.
    pub(crate) fn openrouter_cloud_from_env() -> Self {
        let settings = crate::settings::XavierSettings::current();
        Self {
            provider_mode: ProviderMode::Cloud,
            api_flavor: ApiFlavor::OpenAICompatible,
            provider_label: "openrouter".to_string(),
            model: nonempty_env("XAVIER_OPENROUTER_MODEL")
                .or_else(|| nonempty_env("XAVIER_LLM_MODEL"))
                .or_else(|| nonempty_env("OPENROUTER_MODEL"))
                .or_else(|| {
                    settings
                        .models
                        .cloud_llm_model
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                })
                .or_else(|| {
                    settings
                        .models
                        .llm_model
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                })
                .unwrap_or_default(),
            api_key: nonempty_env("XAVIER_OPENROUTER_API_KEY")
                .or_else(|| nonempty_env("OPENROUTER_API_KEY"))
                .or_else(|| {
                    HardwareVault::new("xavier")
                        .get_secret("OPENROUTER_API_KEY")
                        .ok()
                        .filter(|s| !s.trim().is_empty())
                })
                .or_else(|| {
                    settings
                        .models
                        .llm_api_key
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                }),
            base_url: Some(
                nonempty_env("XAVIER_OPENROUTER_URL")
                    .or_else(|| nonempty_env("OPENROUTER_BASE_URL"))
                    .or_else(|| {
                        settings
                            .models
                            .cloud_llm_url
                            .clone()
                            .filter(|s| !s.trim().is_empty())
                    })
                    .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string()),
            ),
            target: ProviderTarget::GenericOpenAICompatible,
            lease_config: None,
            secret_injection_strategy: None,
            lease_token: None,
        }
    }

    /// Groq cloud from env.
    pub(crate) fn groq_cloud_from_env() -> Self {
        let settings = crate::settings::XavierSettings::current();
        Self {
            provider_mode: ProviderMode::Cloud,
            api_flavor: ApiFlavor::OpenAICompatible,
            provider_label: "groq".to_string(),
            model: nonempty_env("XAVIER_LLM_MODEL")
                .or_else(|| nonempty_env("GROQ_MODEL"))
                .or_else(|| {
                    settings
                        .models
                        .llm_model
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                })
                .unwrap_or_default(),
            api_key: nonempty_env("GROQ_API_KEY")
                .or_else(|| {
                    HardwareVault::new("xavier")
                        .get_secret("GROQ_API_KEY")
                        .ok()
                        .filter(|s| !s.trim().is_empty())
                })
                .or_else(|| {
                    settings
                        .models
                        .llm_api_key
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                }),
            base_url: Some(
                nonempty_env("GROQ_BASE_URL")
                    .or_else(|| {
                        settings
                            .models
                            .cloud_llm_url
                            .clone()
                            .filter(|v| !v.trim().is_empty())
                    })
                    .unwrap_or_else(|| DEFAULT_GROQ_BASE_URL.to_string()),
            ),
            target: ProviderTarget::GenericOpenAICompatible,
            lease_config: None,
            secret_injection_strategy: None,
            lease_token: None,
        }
    }

    /// Deepseek cloud from env.
    pub(crate) fn deepseek_cloud_from_env() -> Self {
        let settings = crate::settings::XavierSettings::current();
        Self {
            provider_mode: ProviderMode::Cloud,
            api_flavor: ApiFlavor::OpenAICompatible,
            provider_label: "deepseek".to_string(),
            model: nonempty_env("XAVIER_LLM_MODEL")
                .or_else(|| nonempty_env("DEEPSEEK_MODEL"))
                .or_else(|| {
                    settings
                        .models
                        .llm_model
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                })
                .unwrap_or_default(),
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
            model: nonempty_env("XAVIER_LLM_MODEL")
                .or_else(|| nonempty_env("ANTHROPIC_MODEL"))
                .or_else(|| {
                    settings
                        .models
                        .llm_model
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                })
                .unwrap_or_default(),
            api_key: nonempty_env("ANTHROPIC_API_KEY")
                .or_else(|| {
                    HardwareVault::new("xavier")
                        .get_secret("ANTHROPIC_API_KEY")
                        .ok()
                        .filter(|s| !s.trim().is_empty())
                })
                .or_else(|| nonempty_env("XAVIER_LLM_API_KEY"))
                .or_else(|| {
                    settings
                        .models
                        .llm_api_key
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                }),
            base_url: Some(
                nonempty_env("ANTHROPIC_BASE_URL")
                    .or_else(|| nonempty_env("XAVIER_CLOUD_LLM_URL"))
                    .or_else(|| {
                        settings
                            .models
                            .cloud_llm_url
                            .clone()
                            .filter(|s| !s.trim().is_empty())
                    })
                    .unwrap_or_else(|| DEFAULT_ANTHROPIC_BASE_URL.to_string()),
            ),
            target: ProviderTarget::AnthropicMessages,
            lease_config: None,
            secret_injection_strategy: None,
            lease_token: None,
        }
    }

    /// Minimax cloud from env.
    pub(crate) fn minimax_cloud_from_env() -> Self {
        let settings = crate::settings::XavierSettings::current();
        Self {
            provider_mode: ProviderMode::Cloud,
            api_flavor: ApiFlavor::OpenAICompatible,
            provider_label: "minimax".to_string(),
            model: nonempty_env("XAVIER_LLM_MODEL")
                .or_else(|| nonempty_env("MINIMAX_MODEL"))
                .or_else(|| {
                    settings
                        .models
                        .llm_model
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                })
                .unwrap_or_default(),
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

    /// Gemini cloud from env.
    pub(crate) fn gemini_cloud_from_env() -> Self {
        let settings = crate::settings::XavierSettings::current();
        Self {
            provider_mode: ProviderMode::Cloud,
            api_flavor: ApiFlavor::OpenAICompatible,
            provider_label: "gemini".to_string(),
            model: nonempty_env("XAVIER_LLM_MODEL")
                .or_else(|| nonempty_env("GEMINI_MODEL"))
                .or_else(|| {
                    settings
                        .models
                        .llm_model
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                })
                .unwrap_or_default(),
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

    /// Zai cloud from env.
    pub(crate) fn zai_cloud_from_env() -> Self {
        let settings = crate::settings::XavierSettings::current();
        Self {
            provider_mode: ProviderMode::Cloud,
            api_flavor: ApiFlavor::OpenAICompatible,
            provider_label: "z.ai".to_string(),
            model: nonempty_env("XAVIER_ZAI_MODEL")
                .or_else(|| nonempty_env("ZAI_MODEL"))
                .or_else(|| {
                    settings
                        .models
                        .zai_model
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                })
                .unwrap_or_default(),
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

    /// Opencode from env.
    pub(crate) fn opencode_from_env() -> Self {
        let settings = crate::settings::XavierSettings::current();
        let api_key = std::env::var("OPENCODE_API_KEY").ok();
        #[cfg(not(test))]
        let api_key = api_key
            .or_else(|| {
                HardwareVault::new("xavier")
                    .get_secret("OPENCODE_API_KEY")
                    .ok()
            })
            .or_else(|| HardwareVault::new("xavier").get_secret("ZAI_API_KEY").ok());

        Self {
            provider_mode: ProviderMode::Local,
            api_flavor: ApiFlavor::OpenAICompatible,
            provider_label: "opencode".to_string(),
            model: nonempty_env("XAVIER_OPENCODE_MODEL")
                .or_else(|| nonempty_env("OPENCODE_MODEL"))
                .or_else(|| {
                    settings
                        .models
                        .opencode_model
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                })
                .unwrap_or_default(),
            api_key,
            base_url: None,
            target: ProviderTarget::OpenCodeCLI,
            lease_config: None,
            secret_injection_strategy: None,
            lease_token: None,
        }
    }

    /// Disabled.
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

    /// Gets the base URL, resolving dynamically for ManagedLocal if a managed server is in memory.
    pub fn get_resolved_base_url(&self) -> Option<String> {
        if self.provider_mode == ProviderMode::ManagedLocal {
            if let Some(port) = crate::agents::provider::get_managed_server_port() {
                return Some(format!("http://127.0.0.1:{}/v1", port));
            }
        }
        self.base_url.clone()
    }

    /// Checks if the provider is correctly configured.
    pub fn is_configured(&self) -> bool {
        match self.provider_mode {
            ProviderMode::Disabled => false,
            ProviderMode::Local | ProviderMode::ManagedLocal => {
                if self.target == ProviderTarget::OpenCodeCLI {
                    return !self.model.trim().is_empty();
                }
                let has_model = !self.model.trim().is_empty();
                let has_url = self
                    .get_resolved_base_url()
                    .as_ref()
                    .is_some_and(|value| !value.trim().is_empty());
                has_model && has_url
            }
            ProviderMode::Cloud => {
                let has_model = !self.model.trim().is_empty();
                let has_url = if self.target == ProviderTarget::GeminiLegacy {
                    true
                } else {
                    self.get_resolved_base_url()
                        .as_ref()
                        .is_some_and(|value| !value.trim().is_empty())
                };
                let has_key = self
                    .api_key
                    .as_ref()
                    .is_some_and(|value| !value.trim().is_empty());
                let has_lease = self.lease_config.is_some();

                has_model && has_url && (has_key || has_lease)
            }
        }
    }

    /// Returns "set" or "unset" for the API key state.
    pub fn api_key_state(&self) -> &'static str {
        match &self.api_key {
            Some(k) if !k.trim().is_empty() && k.trim() != "ollama" => "set",
            _ => "unset",
        }
    }

    /// Returns the reason why this configuration is not considered configured.
    pub fn unconfigured_reason(&self) -> Option<String> {
        if self.is_configured() {
            return None;
        }

        if self.provider_mode == ProviderMode::Disabled {
            return Some("LLM provider is disabled (XAVIER_MODEL_PROVIDER=disabled).".to_string());
        }

        let label = &self.provider_label;
        if label == "local" || label == "managed-local" {
            if self.model.trim().is_empty() {
                return Some("no LLM model configured for provider 'local'. Set XAVIER_LOCAL_LLM_MODEL (and XAVIER_LOCAL_LLM_URL for any OpenAI-compatible server), or pick a hosted provider: XAVIER_MODEL_PROVIDER=openrouter|groq|anthropic|openai with XAVIER_LLM_MODEL and its API key env var. Config keys: models.provider, models.local_llm_model / models.llm_model. See docs/reference/ENV_VARS.md.".to_string());
            }
            return Some("no LLM provider configured".to_string());
        }

        let (model_vars, key_vars) = Self::hint_vars(label);
        if self.model.trim().is_empty() {
            let joined = model_vars.join(" or ");
            return Some(format!(
                "no LLM model configured for provider '{label}'. Set {joined}. See docs/reference/ENV_VARS.md."
            ));
        }

        let has_key = self
            .api_key
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty());
        let has_lease = self.lease_config.is_some();
        if !has_key && !has_lease {
            let joined = key_vars.join(" or ");
            return Some(format!(
                "provider '{label}' has no API key. Set {joined} in the environment."
            ));
        }

        Some("no LLM provider configured".to_string())
    }

    fn hint_vars(label: &str) -> (&'static [&'static str], &'static [&'static str]) {
        match label.trim().to_ascii_lowercase().as_str() {
            "openrouter" => (
                &[
                    "XAVIER_OPENROUTER_MODEL",
                    "XAVIER_LLM_MODEL",
                    "OPENROUTER_MODEL",
                ],
                &["XAVIER_OPENROUTER_API_KEY", "OPENROUTER_API_KEY"],
            ),
            "groq" => (&["XAVIER_LLM_MODEL", "GROQ_MODEL"], &["GROQ_API_KEY"]),
            "anthropic" => (
                &["XAVIER_LLM_MODEL", "ANTHROPIC_MODEL"],
                &["ANTHROPIC_API_KEY"],
            ),
            "openai" => (&["XAVIER_LLM_MODEL", "OPENAI_MODEL"], &["OPENAI_API_KEY"]),
            "deepseek" => (
                &["XAVIER_LLM_MODEL", "DEEPSEEK_MODEL"],
                &["DEEPSEEK_API_KEY"],
            ),
            "minimax" => (&["XAVIER_LLM_MODEL", "MINIMAX_MODEL"], &["MINIMAX_API_KEY"]),
            "gemini" => (&["XAVIER_LLM_MODEL", "GEMINI_MODEL"], &["GEMINI_API_KEY"]),
            "z.ai" | "zai" => (&["XAVIER_ZAI_MODEL", "ZAI_MODEL"], &["ZAI_API_KEY"]),
            "opencode" => (
                &[
                    "XAVIER_OPENCODE_MODEL",
                    "OPENCODE_MODEL",
                    "models.opencode_model",
                ],
                &["OPENCODE_API_KEY"],
            ),
            _ => (
                &["XAVIER_LOCAL_LLM_MODEL", "XAVIER_LLM_MODEL"],
                &["XAVIER_LOCAL_LLM_API_KEY"],
            ),
        }
    }

    /// Probes the provider endpoint for reachability and whether the configured model is listed.
    pub async fn probe(&self) -> LlmProbe {
        if !self.is_configured() {
            return LlmProbe {
                reachability: ProviderReachability::NotConfigured,
                model_listed: None,
            };
        }

        let base_url = match self.get_resolved_base_url() {
            Some(url) => url,
            None => {
                // If there's no base_url but it's configured (e.g. OpenCodeCLI),
                // it shells out locally.
                return LlmProbe {
                    reachability: ProviderReachability::ConfiguredAndReachable,
                    model_listed: None,
                };
            }
        };

        let now = Instant::now();
        let ttl = Duration::from_secs(15);
        let cache_key = format!("{}|{}", base_url, self.model);

        // Check cache first
        {
            if let Ok(cache) = REACHABILITY_CACHE.lock() {
                if let Some((reachable, model_listed, timestamp)) = cache.get(&cache_key) {
                    if now.duration_since(*timestamp) < ttl {
                        let reachability = if *reachable {
                            ProviderReachability::ConfiguredAndReachable
                        } else {
                            ProviderReachability::ConfiguredAndUnreachable
                        };
                        return LlmProbe {
                            reachability,
                            model_listed: *model_listed,
                        };
                    }
                }
            }
        }

        let url = format!("{}/models", base_url.trim_end_matches('/'));

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let mut req = client.get(&url);
        if let Some(key) = &self.api_key {
            let key_trimmed = key.trim();
            if !key_trimmed.is_empty() && key_trimmed != "ollama" {
                if self.target == ProviderTarget::AnthropicMessages {
                    req = req
                        .header("x-api-key", key_trimmed)
                        .header("anthropic-version", "2023-06-01");
                } else {
                    req = req.bearer_auth(key_trimmed);
                }
            }
        }

        let resp_res = req.send().await;
        let (reachable, model_listed) = match resp_res {
            Ok(resp) if resp.status().is_success() => {
                let body_res = resp.json::<serde_json::Value>().await;
                let listed = match body_res {
                    Ok(val) => {
                        if let Some(arr) = val.get("data").and_then(|d| d.as_array()) {
                            let want = self.model.trim();
                            let found = arr.iter().any(|item| {
                                item.get("id").and_then(|id| id.as_str()).is_some_and(|id| {
                                    id == want || id.strip_suffix(":latest") == Some(want)
                                })
                            });
                            // Paginated listings (e.g. Anthropic `has_more`) can't prove absence.
                            let more = val
                                .get("has_more")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            if found {
                                Some(true)
                            } else if more {
                                None
                            } else {
                                Some(false)
                            }
                        } else {
                            None
                        }
                    }
                    Err(_) => None,
                };
                (true, listed)
            }
            _ => (false, None),
        };

        if let Ok(mut cache) = REACHABILITY_CACHE.lock() {
            cache.insert(cache_key, (reachable, model_listed, now));
        }

        LlmProbe {
            reachability: if reachable {
                ProviderReachability::ConfiguredAndReachable
            } else {
                ProviderReachability::ConfiguredAndUnreachable
            },
            model_listed,
        }
    }

    /// Checks the reachability of the provider.
    pub async fn is_reachable(&self) -> ProviderReachability {
        self.probe().await.reachability
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
    #[allow(dead_code)]
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
        let prev_config_path = std::env::var("XAVIER_CONFIG_PATH").ok();
        std::env::set_var(
            "XAVIER_CONFIG_PATH",
            "/tmp/nonexistent-xavier-ci-defaults.json",
        );
        std::env::set_var("XAVIER_LOCAL_LLM_MODEL", "test-model");
        std::env::remove_var("XAVIER_LLM_MODEL");
        std::env::set_var("XAVIER_LOCAL_LLM_URL", "http://test-url/v1");
        std::env::remove_var("XAVIER_API_FLAVOR");

        let config = ModelProviderConfig::local_from_env();

        match prev_config_path {
            Some(v) => std::env::set_var("XAVIER_CONFIG_PATH", v),
            None => std::env::remove_var("XAVIER_CONFIG_PATH"),
        }

        assert_eq!(config.model, "test-model");
        assert_eq!(config.base_url, Some("http://test-url/v1".to_string()));
        assert_eq!(config.provider_mode, ProviderMode::Local);
        assert_eq!(config.api_flavor, ApiFlavor::OpenAICompatible);
    }

    #[test]
    fn test_local_provider_defaults() {
        let _guard = env_lock().lock().expect("test assertion");
        // Deterministic gate: isolate from the repo's config/xavier.config.json
        // (workspace-pinned model/url evolve with merged PRs) and from
        // leftover blank env vars. With everything unset, `local_from_env`
        // must fall through to the compiled `XavierSettings::default()`
        // values — assert against those, so this test survives defaults
        // bumps without coupling to any literal model string.
        let prev_config_path = std::env::var("XAVIER_CONFIG_PATH").ok();
        let prev_api_key = std::env::var("XAVIER_LOCAL_LLM_API_KEY").ok();
        let prev_anthropic_url = std::env::var("XAVIER_LOCAL_ANTHROPIC_URL").ok();
        std::env::set_var(
            "XAVIER_CONFIG_PATH",
            "/tmp/nonexistent-xavier-ci-defaults.json",
        );
        std::env::remove_var("XAVIER_LOCAL_LLM_MODEL");
        std::env::remove_var("XAVIER_LLM_MODEL");
        std::env::remove_var("XAVIER_LOCAL_LLM_URL");
        std::env::remove_var("XAVIER_LOCAL_LLM_API_KEY");
        std::env::remove_var("XAVIER_LOCAL_ANTHROPIC_URL");
        std::env::remove_var("XAVIER_API_FLAVOR");

        let config = ModelProviderConfig::local_from_env();

        match prev_config_path {
            Some(v) => std::env::set_var("XAVIER_CONFIG_PATH", v),
            None => std::env::remove_var("XAVIER_CONFIG_PATH"),
        }
        match prev_api_key {
            Some(v) => std::env::set_var("XAVIER_LOCAL_LLM_API_KEY", v),
            None => std::env::remove_var("XAVIER_LOCAL_LLM_API_KEY"),
        }
        match prev_anthropic_url {
            Some(v) => std::env::set_var("XAVIER_LOCAL_ANTHROPIC_URL", v),
            None => std::env::remove_var("XAVIER_LOCAL_ANTHROPIC_URL"),
        }

        assert!(config.model.is_empty());
        let expected = crate::settings::XavierSettings::default();
        assert_eq!(config.base_url, Some(expected.models.local_llm_url.clone()));
        let expected_key = expected
            .models
            .local_llm_api_key
            .clone()
            .unwrap_or_else(|| "ollama".to_string());
        assert_eq!(config.api_key, Some(expected_key));
    }

    #[test]
    fn test_local_unconfigured_without_model() {
        let _guard = env_lock().lock().expect("test assertion");
        let prev_config_path = std::env::var("XAVIER_CONFIG_PATH").ok();
        std::env::set_var(
            "XAVIER_CONFIG_PATH",
            "/tmp/nonexistent-xavier-ci-defaults.json",
        );
        std::env::remove_var("XAVIER_LOCAL_LLM_MODEL");
        std::env::remove_var("XAVIER_LLM_MODEL");
        std::env::remove_var("XAVIER_LOCAL_LLM_URL");
        std::env::remove_var("XAVIER_LOCAL_LLM_API_KEY");
        std::env::remove_var("XAVIER_LOCAL_ANTHROPIC_URL");
        std::env::remove_var("XAVIER_API_FLAVOR");

        let config = ModelProviderConfig::local_from_env();

        match prev_config_path {
            Some(v) => std::env::set_var("XAVIER_CONFIG_PATH", v),
            None => std::env::remove_var("XAVIER_CONFIG_PATH"),
        }

        assert_eq!(config.model, "");
        assert!(!config.is_configured());
        let reason = config.unconfigured_reason().unwrap_or_default();
        assert!(
            reason.contains("XAVIER_LOCAL_LLM_MODEL"),
            "reason should contain XAVIER_LOCAL_LLM_MODEL, got: {reason}"
        );
    }

    #[tokio::test]
    async fn test_local_unconfigured_is_reachable_returns_not_configured() {
        let config = {
            let _guard = env_lock().lock().expect("test assertion");
            let prev_config_path = std::env::var("XAVIER_CONFIG_PATH").ok();
            std::env::set_var(
                "XAVIER_CONFIG_PATH",
                "/tmp/nonexistent-xavier-ci-defaults.json",
            );
            std::env::remove_var("XAVIER_LOCAL_LLM_MODEL");
            std::env::remove_var("XAVIER_LLM_MODEL");

            let config = ModelProviderConfig::local_from_env();

            match prev_config_path {
                Some(v) => std::env::set_var("XAVIER_CONFIG_PATH", v),
                None => std::env::remove_var("XAVIER_CONFIG_PATH"),
            }
            config
        };

        assert_eq!(
            config.is_reachable().await,
            ProviderReachability::NotConfigured
        );
    }

    #[test]
    fn test_hosted_provider_missing_key_reason() {
        let _guard = env_lock().lock().expect("test assertion");
        std::env::remove_var("GROQ_API_KEY");
        let mut config = ModelProviderConfig::groq_cloud_from_env();
        config.model = "test-model".to_string();
        config.api_key = None;

        let reason = config.unconfigured_reason().unwrap_or_default();
        assert!(
            reason.contains("GROQ_API_KEY"),
            "reason should mention GROQ_API_KEY: {reason}"
        );
    }

    #[test]
    fn test_debug_redacts_api_key() {
        let mut config = ModelProviderConfig::for_provider("openai");
        config.api_key = Some("sk-secret-key-123".to_string());
        config.lease_token = Some("sk-secret-lease-456".to_string());
        let debug_str = format!("{config:?}");
        assert!(!debug_str.contains("sk-secret-key-123"));
        assert!(!debug_str.contains("sk-secret-lease-456"));
        assert!(debug_str.contains("<redacted>"));
    }

    #[tokio::test]
    async fn test_probe_anthropic_uses_x_api_key() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/models")
            .match_header("x-api-key", "test-anthropic-key")
            .match_header("anthropic-version", "2023-06-01")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":[{"id":"test-model"}]}"#)
            .create_async()
            .await;

        let config = ModelProviderConfig {
            provider_mode: ProviderMode::Cloud,
            api_flavor: ApiFlavor::AnthropicCompatible,
            provider_label: "anthropic".to_string(),
            model: "test-model".to_string(),
            api_key: Some("test-anthropic-key".to_string()),
            base_url: Some(server.url()),
            target: ProviderTarget::AnthropicMessages,
            lease_config: None,
            lease_token: None,
            secret_injection_strategy: None,
        };

        let probe = config.probe().await;
        mock.assert_async().await;
        assert_eq!(
            probe.reachability,
            ProviderReachability::ConfiguredAndReachable
        );
        assert_eq!(probe.model_listed, Some(true));
    }

    #[tokio::test]
    async fn test_probe_model_not_listed() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/not-listed/models")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":[{"id":"other-model"}]}"#)
            .create_async()
            .await;

        let config = ModelProviderConfig {
            provider_mode: ProviderMode::Cloud,
            api_flavor: ApiFlavor::OpenAICompatible,
            provider_label: "openai".to_string(),
            model: "test-model".to_string(),
            api_key: Some("test-key".to_string()),
            base_url: Some(format!("{}/not-listed", server.url())),
            target: ProviderTarget::GenericOpenAICompatible,
            lease_config: None,
            lease_token: None,
            secret_injection_strategy: None,
        };

        let probe = config.probe().await;
        mock.assert_async().await;
        assert_eq!(
            probe.reachability,
            ProviderReachability::ConfiguredAndReachable
        );
        assert_eq!(probe.model_listed, Some(false));
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

    #[tokio::test]
    async fn test_is_reachable_not_configured() {
        let config = ModelProviderConfig::disabled();
        let reachability = config.is_reachable().await;
        assert_eq!(reachability, ProviderReachability::NotConfigured);
    }

    #[tokio::test]
    async fn test_is_reachable_opencode() {
        let mut config = ModelProviderConfig::opencode_from_env();
        config.model = "opencode/deepseek-v4-flash".to_string();
        let reachability = config.is_reachable().await;
        assert_eq!(reachability, ProviderReachability::ConfiguredAndReachable);
    }

    #[tokio::test]
    async fn test_is_reachable_success() {
        let mut server = mockito::Server::new_async().await;

        let mut config = ModelProviderConfig::local_from_env();
        config.model = "test-model".to_string();
        config.base_url = Some(server.url());
        // force unique url so cache doesn't hit
        config.base_url = Some(format!("{}/success", server.url()));

        let mock = server
            .mock("GET", "/success/models")
            .with_status(200)
            .create_async()
            .await;

        let reachability = config.is_reachable().await;
        assert_eq!(reachability, ProviderReachability::ConfiguredAndReachable);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_is_reachable_failure() {
        let mut server = mockito::Server::new_async().await;

        let mut config = ModelProviderConfig::local_from_env();
        config.model = "test-model".to_string();
        config.base_url = Some(server.url());
        // force unique url so cache doesn't hit
        config.base_url = Some(format!("{}/failure", server.url()));

        let mock = server
            .mock("GET", "/failure/models")
            .with_status(500)
            .create_async()
            .await;

        let reachability = config.is_reachable().await;
        assert_eq!(reachability, ProviderReachability::ConfiguredAndUnreachable);
        mock.assert_async().await;
    }
}
