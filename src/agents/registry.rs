//! Agent registry for lifecycle management and agent configuration loading.
//!
//! Provides data structures and loading/validation mechanisms for `agent-registry.toml`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Error type for agent registry operations and validation.
#[derive(Debug, Error)]
pub enum RegistryError {
    /// File IO error.
    #[error("Failed to read agent registry file at '{path}': {source}")]
    Io {
        /// Path to file.
        path: PathBuf,
        /// Underlying IO error.
        #[source]
        source: std::io::Error,
    },
    /// TOML parsing error.
    #[error("Failed to parse TOML agent registry at '{path}': {source}")]
    TomlParse {
        /// Path to file.
        path: PathBuf,
        /// Underlying TOML error.
        #[source]
        source: toml::de::Error,
    },
    /// Registry validation failure (e.g. duplicate IDs or empty fields).
    #[error("Validation error in agent registry: {0}")]
    Validation(String),
}

/// Routing configuration settings in `agent-registry.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub struct RoutingConfig {
    /// Strategy used for routing tasks to agents (e.g., "CapabilityMatch").
    pub strategy: Option<String>,
}

/// Rate limit configuration for a provider.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub struct RateLimitConfig {
    /// Requests per minute limit.
    pub rpm: Option<u64>,
    /// Tokens per minute limit.
    pub tpm: Option<u64>,
}

/// Configuration for an individual provider entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub struct ProviderEntry {
    /// Base URL endpoint for the provider.
    pub base_url: Option<String>,
    /// Rate limit settings for the provider.
    pub rate_limit: Option<RateLimitConfig>,
}

/// Individual agent entry defined in `agent-registry.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct AgentEntry {
    /// Unique agent identifier name.
    pub name: String,
    /// Model provider label (e.g. "local", "openai").
    pub provider: String,
    /// Specific model identifier name.
    pub model: String,
    /// Agent type classification (e.g. "Assistant", "Api").
    #[serde(default, rename = "type")]
    pub r#type: Option<String>,
    /// List of agent capabilities.
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// Root agent registry structure matching `agent-registry.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub struct AgentRegistryFile {
    /// Global routing rules.
    pub routing: Option<RoutingConfig>,
    /// Registered agent list.
    #[serde(default)]
    pub agents: Vec<AgentEntry>,
    /// Configured provider entries.
    #[serde(default)]
    pub providers: HashMap<String, ProviderEntry>,
}

/// Alias for `AgentRegistryFile`.
pub type AgentRegistry = AgentRegistryFile;

impl AgentRegistryFile {
    /// Gets an agent entry by name.
    pub fn get_agent(&self, name: &str) -> Option<&AgentEntry> {
        self.agents.iter().find(|a| a.name == name)
    }

    /// Lists all registered agents.
    pub fn list_agents(&self) -> &[AgentEntry] {
        &self.agents
    }
}

/// Validates loaded registry data for required fields and uniqueness.
fn validate(registry: &AgentRegistryFile) -> Result<(), RegistryError> {
    let mut seen_names = std::collections::HashSet::new();
    for agent in &registry.agents {
        if agent.name.trim().is_empty() {
            return Err(RegistryError::Validation(
                "Agent entry contains an empty name".to_string(),
            ));
        }
        if agent.provider.trim().is_empty() {
            return Err(RegistryError::Validation(format!(
                "Agent '{}' contains an empty provider",
                agent.name
            )));
        }
        if agent.model.trim().is_empty() {
            return Err(RegistryError::Validation(format!(
                "Agent '{}' contains an empty model",
                agent.name
            )));
        }
        if !seen_names.insert(&agent.name) {
            return Err(RegistryError::Validation(format!(
                "Duplicate agent name found: '{}'",
                agent.name
            )));
        }
    }
    Ok(())
}

/// Parses an agent registry from a TOML content string and validates it.
pub fn load_from_str(content: &str) -> Result<AgentRegistry, RegistryError> {
    let registry: AgentRegistryFile =
        toml::from_str(content).map_err(|e| RegistryError::TomlParse {
            path: PathBuf::from("<string>"),
            source: e,
        })?;

    validate(&registry)?;
    Ok(registry)
}

/// Loads and validates an agent registry from the given file path.
pub fn load_from_path(path: &Path) -> Result<AgentRegistry, RegistryError> {
    let content = std::fs::read_to_string(path).map_err(|e| RegistryError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;

    let registry: AgentRegistryFile =
        toml::from_str(&content).map_err(|e| RegistryError::TomlParse {
            path: path.to_path_buf(),
            source: e,
        })?;

    validate(&registry)?;
    Ok(registry)
}

/// Loads agent registry with resolution order:
/// 1. `XAVIER_AGENT_REGISTRY` environment variable path override.
/// 2. `agent-registry.toml` in current repository/working directory.
/// 3. Built-in default empty registry if no file exists.
pub fn load_default() -> Result<AgentRegistry, RegistryError> {
    if let Ok(env_path) = std::env::var("XAVIER_AGENT_REGISTRY") {
        let path = Path::new(&env_path);
        return load_from_path(path);
    }

    let default_path = Path::new("agent-registry.toml");
    if default_path.exists() {
        return load_from_path(default_path);
    }

    Ok(AgentRegistry::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    const SAMPLE_TOML: &str = r#"
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
    fn test_load_from_str_valid() {
        let registry = load_from_str(SAMPLE_TOML).expect("Failed to parse valid sample TOML");
        assert_eq!(
            registry
                .routing
                .as_ref()
                .and_then(|r| r.strategy.as_deref()),
            Some("CapabilityMatch")
        );
        assert_eq!(registry.agents.len(), 2);

        let hermes = registry.get_agent("hermes").unwrap();
        assert_eq!(hermes.provider, "local");
        assert_eq!(hermes.model, "muse-spark-1.2-contributor");
        assert_eq!(hermes.r#type.as_deref(), Some("Assistant"));
        assert_eq!(hermes.capabilities, vec!["code", "orchestration"]);

        let provider = registry.providers.get("local").unwrap();
        assert_eq!(provider.base_url.as_deref(), Some("http://localhost:8081"));
        assert_eq!(provider.rate_limit.as_ref().and_then(|rl| rl.rpm), Some(60));
    }

    #[test]
    fn test_validation_duplicate_names() {
        let invalid_toml = r#"
[[agents]]
name = "hermes"
provider = "local"
model = "model-a"

[[agents]]
name = "hermes"
provider = "openai"
model = "model-b"
"#;
        let err = load_from_str(invalid_toml).unwrap_err();
        match err {
            RegistryError::Validation(msg) => {
                assert!(msg.contains("Duplicate agent name found: 'hermes'"));
            }
            _ => panic!("Expected Validation error"),
        }
    }

    #[test]
    fn test_validation_empty_fields() {
        let empty_name_toml = r#"
[[agents]]
name = ""
provider = "local"
model = "model-a"
"#;
        assert!(matches!(
            load_from_str(empty_name_toml).unwrap_err(),
            RegistryError::Validation(_)
        ));

        let empty_provider_toml = r#"
[[agents]]
name = "test"
provider = "  "
model = "model-a"
"#;
        assert!(matches!(
            load_from_str(empty_provider_toml).unwrap_err(),
            RegistryError::Validation(_)
        ));
    }

    #[test]
    fn test_load_from_path() {
        use std::io::Write;
        let mut tmp_file = NamedTempFile::new().unwrap();
        write!(tmp_file, "{}", SAMPLE_TOML).unwrap();

        let registry = load_from_path(tmp_file.path()).unwrap();
        assert_eq!(registry.agents.len(), 2);
    }

    #[test]
    fn test_env_var_override() {
        use std::io::Write;
        let mut tmp_file = NamedTempFile::new().unwrap();
        write!(tmp_file, "{}", SAMPLE_TOML).unwrap();

        std::env::set_var("XAVIER_AGENT_REGISTRY", tmp_file.path().to_str().unwrap());

        let registry = load_default().unwrap();
        assert_eq!(registry.agents.len(), 2);

        std::env::remove_var("XAVIER_AGENT_REGISTRY");
    }
}
