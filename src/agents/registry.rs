//! Agent registry for lifecycle management and configuration.
//!
//! Provides the implementation and data structures for loading, validating,
//! and managing agents from `agent-registry.toml`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors that can occur during registry loading and validation.
#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("Failed to read registry file at '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to parse registry TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("Validation error: duplicate agent name '{0}'")]
    DuplicateAgentName(String),
    #[error("Validation error: agent '{name}' has empty required field '{field}'")]
    EmptyField { name: String, field: &'static str },
}

/// Configuration for rate limiting a provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RateLimitConfig {
    pub rpm: Option<u64>,
    pub tpm: Option<u64>,
}

/// Provider details defined in the registry.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderConfig {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub rate_limit: Option<RateLimitConfig>,
}

/// Routing strategy options defined in the registry.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoutingConfig {
    pub strategy: Option<String>,
}

/// Represents an agent entry in `agent-registry.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentEntry {
    pub name: String,
    pub provider: String,
    pub model: String,
    #[serde(rename = "type")]
    pub agent_type: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// Raw representation of the agent registry TOML file structure.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentRegistryFile {
    pub routing: Option<RoutingConfig>,
    #[serde(default)]
    pub agents: Vec<AgentEntry>,
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
}

/// In-memory representation of loaded and validated agent entries.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentRegistry {
    pub routing: Option<RoutingConfig>,
    pub agents: Vec<AgentEntry>,
    pub providers: HashMap<String, ProviderConfig>,
}

impl AgentRegistry {
    /// Parses an `AgentRegistry` from a TOML string and performs validation.
    pub fn from_toml_str(content: &str) -> Result<Self, RegistryError> {
        let file: AgentRegistryFile = toml::from_str(content)?;
        Self::from_file_struct(file)
    }

    /// Validates and converts an `AgentRegistryFile` into an `AgentRegistry`.
    pub fn from_file_struct(file: AgentRegistryFile) -> Result<Self, RegistryError> {
        let mut seen_names = std::collections::HashSet::new();

        for agent in &file.agents {
            if agent.name.trim().is_empty() {
                return Err(RegistryError::EmptyField {
                    name: agent.name.clone(),
                    field: "name",
                });
            }
            if agent.provider.trim().is_empty() {
                return Err(RegistryError::EmptyField {
                    name: agent.name.clone(),
                    field: "provider",
                });
            }
            if agent.model.trim().is_empty() {
                return Err(RegistryError::EmptyField {
                    name: agent.name.clone(),
                    field: "model",
                });
            }
            if !seen_names.insert(&agent.name) {
                return Err(RegistryError::DuplicateAgentName(agent.name.clone()));
            }
        }

        Ok(Self {
            routing: file.routing,
            agents: file.agents,
            providers: file.providers,
        })
    }

    /// Loads an `AgentRegistry` from a specified path.
    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Self, RegistryError> {
        let path_ref = path.as_ref();
        let content = std::fs::read_to_string(path_ref).map_err(|source| RegistryError::Io {
            path: path_ref.to_path_buf(),
            source,
        })?;
        Self::from_toml_str(&content)
    }

    /// Resolves the registry path from:
    /// 1. `XAVIER_AGENT_REGISTRY` environment variable if set.
    /// 2. `agent-registry.toml` in working directory if present.
    /// 3. Built-in default (empty `AgentRegistry`) if no file exists.
    pub fn resolve_and_load() -> Result<Self, RegistryError> {
        if let Ok(env_path) = std::env::var("XAVIER_AGENT_REGISTRY") {
            if !env_path.trim().is_empty() {
                return Self::load_from_path(Path::new(&env_path));
            }
        }

        let default_path = Path::new("agent-registry.toml");
        if default_path.exists() {
            Self::load_from_path(default_path)
        } else {
            Ok(Self::default())
        }
    }

    /// Retrieves an agent entry by name.
    pub fn get_agent(&self, name: &str) -> Option<&AgentEntry> {
        self.agents.iter().find(|a| a.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_TOML: &str = r#"
[routing]
strategy = "CapabilityMatch"

[[agents]]
name = "hermes"
provider = "local"
model = "muse-spark-1.2-contributor"
type = "Assistant"
capabilities = ["code", "orchestration"]

[[agents]]
name = "jules"
provider = "local"
model = "gemini"
type = "Api"
capabilities = ["code", "test"]

[providers.local]
base_url = "http://localhost:8081"
[providers.local.rate_limit]
rpm = 60
tpm = 100000
"#;

    #[test]
    fn test_load_valid_toml() {
        let registry = AgentRegistry::from_toml_str(TEST_TOML).expect("failed to parse test TOML");
        assert_eq!(registry.agents.len(), 2);
        let hermes = registry.get_agent("hermes").unwrap();
        assert_eq!(hermes.provider, "local");
        assert_eq!(hermes.model, "muse-spark-1.2-contributor");
        assert_eq!(hermes.agent_type.as_deref(), Some("Assistant"));
        assert_eq!(hermes.capabilities, vec!["code", "orchestration"]);

        let jules = registry.get_agent("jules").unwrap();
        assert_eq!(jules.provider, "local");
        assert_eq!(jules.model, "gemini");

        let local_provider = registry.providers.get("local").unwrap();
        assert_eq!(local_provider.base_url.as_deref(), Some("http://localhost:8081"));
        let rate_limit = local_provider.rate_limit.as_ref().unwrap();
        assert_eq!(rate_limit.rpm, Some(60));
        assert_eq!(rate_limit.tpm, Some(100000));
    }

    #[test]
    fn test_duplicate_agent_id_validation() {
        let duplicate_toml = r#"
[[agents]]
name = "hermes"
provider = "local"
model = "model-a"

[[agents]]
name = "hermes"
provider = "local"
model = "model-b"
"#;
        let err = AgentRegistry::from_toml_str(duplicate_toml).unwrap_err();
        match err {
            RegistryError::DuplicateAgentName(name) => assert_eq!(name, "hermes"),
            _ => panic!("expected DuplicateAgentName error, got {:?}", err),
        }
    }

    #[test]
    fn test_empty_fields_validation() {
        let empty_name_toml = r#"
[[agents]]
name = ""
provider = "local"
model = "model-a"
"#;
        let err = AgentRegistry::from_toml_str(empty_name_toml).unwrap_err();
        match err {
            RegistryError::EmptyField { name, field } => {
                assert_eq!(name, "");
                assert_eq!(field, "name");
            }
            _ => panic!("expected EmptyField error, got {:?}", err),
        }

        let empty_provider_toml = r#"
[[agents]]
name = "agent1"
provider = "   "
model = "model-a"
"#;
        let err = AgentRegistry::from_toml_str(empty_provider_toml).unwrap_err();
        match err {
            RegistryError::EmptyField { name, field } => {
                assert_eq!(name, "agent1");
                assert_eq!(field, "provider");
            }
            _ => panic!("expected EmptyField error, got {:?}", err),
        }
    }

    #[test]
    fn test_resolve_and_load_default() {
        // Test loading default file or fallback
        let registry = AgentRegistry::resolve_and_load().expect("resolve_and_load failed");
        // agent-registry.toml exists in cwd, so it should load hermes and jules
        if Path::new("agent-registry.toml").exists() {
            assert!(registry.get_agent("hermes").is_some());
        }
    }
}
