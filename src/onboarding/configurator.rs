//! Auto-configuration generator
//!
//! Generates optimal `xavier.config.json` based on system scan results.

//! Auto-configuration generator
//!
//! Generates optimal `xavier.config.json` based on system scan results.
//! Schema matches `XavierSettings` in `src/settings.rs`.

use crate::onboarding::scanner::{ProviderStatus, SystemCapabilities};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Full auto-generated configuration — matches XavierSettings schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    pub memory: MemoryConfig,
    pub workspace: WorkspaceConfig,
    #[serde(default)]
    pub security: serde_json::Value,
    #[serde(default)]
    pub retrieval: serde_json::Value,
    #[serde(default)]
    pub advanced: serde_json::Value,
    #[serde(default)]
    pub router: serde_json::Value,
    #[serde(default)]
    pub chronicle: serde_json::Value,
    #[serde(default)]
    pub agent: serde_json::Value,
    #[serde(default)]
    pub sync: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".into(),
            port: 8006,
            log_level: "info".into(),
        }
    }
}

fn default_log_level() -> String { "info".into() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    pub endpoint: String,
    pub embedder: String,
    #[serde(default = "default_gllm_model")]
    pub gllm_model: String,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            embedder: "bring_your_own".into(),
            gllm_model: "all-MiniLM-L6-v2".into(),
        }
    }
}

fn default_gllm_model() -> String { "all-MiniLM-L6-v2".into() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub backend: String,
    pub embedding_dimensions: usize,
    pub max_memories: usize,
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
}

fn default_data_dir() -> String { "data/memory".into() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub default_workspace_id: String,
    #[serde(default = "default_plan")]
    pub default_plan: String,
    pub storage_limit_bytes: Option<u64>,
}

fn default_plan() -> String { "standard".into() }

impl AutoConfig {
    pub fn summary(&self) -> String {
        format!(
            "{}:{} | {} | {}D | {} cache",
            self.server.host,
            self.server.port,
            self.embedding.embedder,
            self.memory.embedding_dimensions,
            0, // cache size not in settings
        )
    }
}

/// Memory dimensions for different embedders
fn embedder_dimensions(provider: &str) -> usize {
    match provider {
        p if p.contains("gllm") => 384,
        p if p.contains("ollama") => 768, // nomic-embed-text
        p if p.contains("openai") => 512, // text-embedding-3-small
        p if p.contains("google") => 768, // text-embedding-004
        _ => 0,
    }
}

/// Optimal batch size based on RAM
fn optimal_batch_size(ram_gb: f64) -> usize {
    if ram_gb >= 32.0 { 64 }
    else if ram_gb >= 16.0 { 32 }
    else if ram_gb >= 8.0 { 16 }
    else { 8 }
}

/// Max memories based on RAM and disk
fn max_memories(ram_gb: f64, disk_gb: f64) -> usize {
    let base = 10_000;
    let ram_factor = (ram_gb / 4.0) as usize;
    let disk_factor = (disk_gb / 10.0) as usize;
    (base * ram_factor.max(1)).min(base * disk_factor.max(1)).min(1_000_000)
}

/// Generate optimal config based on system scan
pub fn generate_config(
    system: &SystemCapabilities,
    providers: &[ProviderStatus],
    embedder: &str,
) -> AutoConfig {
    let dimensions = embedder_dimensions(embedder);
    let mem = max_memories(system.ram_gb, system.disk_free_gb);
    let _ = optimal_batch_size(system.ram_gb);
    let _ = providers;

    let (embedder_name, endpoint) = match embedder {
        s if s.contains("gllm") => {
            if system.has_avx2 {
                ("gllm (local, AVX2)", "".into())
            } else {
                ("gllm (local)", "".into())
            }
        }
        "ollama" => ("ollama", "http://localhost:11434".into()),
        s if s.contains("openai") => ("openai", "https://api.openai.com/v1".into()),
        s if s.contains("google") => ("google-gemini", "https://generativelanguage.googleapis.com".into()),
        "local-embed-server" => ("local-embed-server", "http://localhost:8080".into()),
        _ => ("bring_your_own", "".into()),
    };

    let gllm_model = match embedder {
        s if s.contains("gllm") => "all-MiniLM-L6-v2",
        _ => "",
    };

    let storage = Some((system.disk_free_gb as u64) * 1024 * 1024 * 1024 / 2);

    AutoConfig {
        server: ServerConfig {
            host: "0.0.0.0".into(),
            port: 8006,
            log_level: "info".into(),
        },
        embedding: EmbeddingConfig {
            endpoint,
            embedder: embedder_name.into(),
            gllm_model: gllm_model.into(),
        },
        memory: MemoryConfig {
            backend: "vec".into(),
            embedding_dimensions: dimensions,
            max_memories: mem,
            data_dir: "data/memory".into(),
        },
        workspace: WorkspaceConfig {
            default_workspace_id: "default".into(),
            default_plan: "standard".into(),
            storage_limit_bytes: storage,
        },
        security: serde_json::json!({}),
        retrieval: serde_json::json!({}),
        advanced: serde_json::json!({}),
        router: serde_json::json!({}),
        chronicle: serde_json::json!({}),
        agent: serde_json::json!({}),
        sync: serde_json::json!({}),
    }
}

/// Write config to xavier.config.json
pub fn apply_config(config: &AutoConfig) -> Result<(), String> {
    let config_path = find_config_path();
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {e}"))?;
    std::fs::write(&config_path, &json)
        .map_err(|e| format!("Failed to write config to {config_path:?}: {e}"))?;
    Ok(())
}

fn find_config_path() -> PathBuf {
    // Check common locations
    let candidates = [
        PathBuf::from("config/xavier.config.json"),
        PathBuf::from("../config/xavier.config.json"),
        dirs::config_dir()
            .map(|d| d.join("xavier").join("xavier.config.json"))
            .unwrap_or_default(),
    ];

    for path in &candidates {
        if path.exists() {
            return path.clone();
        }
    }

    // Default to project config dir
    PathBuf::from("config/xavier.config.json")
}
