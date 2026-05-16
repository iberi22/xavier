use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

const DEFAULT_CONFIG_PATH: &str = "config/xavier.config.json";

#[derive(Debug, Clone, Deserialize, Default)]
pub struct XavierSettings {
    #[serde(default)]
    pub server: ServerSettings,
    #[serde(default)]
    pub workspace: WorkspaceSettings,
    #[serde(default)]
    pub memory: MemorySettings,
    #[serde(default)]
    pub models: ModelSettings,
    #[serde(default)]
    pub retrieval: RetrievalSettings,
    #[serde(default)]
    pub sync: SyncSettings,
    #[serde(skip)]
    pub auth_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerSettings {
    pub host: String,
    pub port: u16,
    pub log_level: String,
    pub code_graph_db_path: String,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8006,
            log_level: "info".to_string(),
            code_graph_db_path: "data/code_graph.db".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceSettings {
    pub default_workspace_id: String,
    pub default_plan: String,
    pub storage_limit_bytes: Option<u64>,
    pub request_limit: Option<usize>,
    pub request_unit_limit: Option<u64>,
    pub embedding_provider_mode: String,
    pub managed_google_embeddings: bool,
    pub sync_policy: String,
}

impl Default for WorkspaceSettings {
    fn default() -> Self {
        Self {
            default_workspace_id: "default".to_string(),
            default_plan: "community".to_string(),
            storage_limit_bytes: None,
            request_limit: None,
            request_unit_limit: None,
            embedding_provider_mode: "bring_your_own".to_string(),
            managed_google_embeddings: false,
            sync_policy: "local_only".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MemorySettings {
    pub backend: String,
    pub data_dir: String,
    pub embedding_dimensions: usize,
    pub workspace_dir: String,
    pub file_path: String,
    pub sqlite_path: String,
    pub vec_path: String,
}

impl Default for MemorySettings {
    fn default() -> Self {
        let data_dir = XavierSettings::resolve_data_dir();
        Self {
            backend: "vec".to_string(),
            data_dir: data_dir.to_string_lossy().to_string(),
            embedding_dimensions: 768,
            workspace_dir: data_dir.join("workspaces").to_string_lossy().to_string(),
            file_path: data_dir
                .join("workspaces")
                .join("default")
                .join("memory-store.json")
                .to_string_lossy()
                .to_string(),
            sqlite_path: data_dir.join("memory-store.sqlite3").to_string_lossy().to_string(),
            vec_path: data_dir.join("vec-store.sqlite3").to_string_lossy().to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelSettings {
    pub provider: String,
    pub api_flavor: String,
    pub local_llm_url: String,
    pub local_llm_model: String,
    pub embedding_url: String,
    pub embedding_model: String,
    pub router_retrieved_model: String,
    pub router_complex_model: String,
    pub router_fast_model: String,
    pub router_quality_model: String,
}

impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            provider: "local".to_string(),
            api_flavor: "openai-compatible".to_string(),
            local_llm_url: "http://localhost:11434/v1".to_string(),
            local_llm_model: "qwen3-coder".to_string(),
            embedding_url: "http://localhost:11434/v1".to_string(),
            embedding_model: "embeddinggemma".to_string(),
            router_retrieved_model: String::new(),
            router_complex_model: String::new(),
            router_fast_model: "opencode/minimax-m2.7".to_string(),
            router_quality_model: "opencode/deepseek-v4-pro".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RetrievalSettings {
    pub disable_hyde: bool,
}

impl Default for RetrievalSettings {
    fn default() -> Self {
        Self { disable_hyde: true }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SyncSettings {
    pub interval_ms: u64,
    pub lag_threshold_ms: u64,
    pub save_ok_rate_threshold: f32,
    pub max_retries: u32,
    pub retry_delay_ms: u64,
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            interval_ms: 300_000,
            lag_threshold_ms: 30_000,
            save_ok_rate_threshold: 0.95,
            max_retries: 3,
            retry_delay_ms: 1_000,
        }
    }
}

impl XavierSettings {
    pub fn resolve_config_path() -> PathBuf {
        if let Ok(env_path) = std::env::var("XAVIER_CONFIG_PATH") {
            return PathBuf::from(env_path);
        }

        if let Some(config_dir) = dirs::config_dir() {
            // Priority 1: ~/.config/xavier/xavier.toml (or OS equivalent)
            let xavier_toml = config_dir.join("xavier").join("xavier.toml");
            if xavier_toml.exists() {
                return xavier_toml;
            }
            // Priority 2: ~/.config/xavier/xavier.config.json
            let xavier_json = config_dir.join("xavier").join("xavier.config.json");
            if xavier_json.exists() {
                return xavier_json;
            }
        }

        // Fallback to local project config
        PathBuf::from(DEFAULT_CONFIG_PATH)
    }

    pub fn resolve_data_dir() -> PathBuf {
        if let Ok(env_path) = std::env::var("XAVIER_DATA_DIR") {
            return PathBuf::from(env_path);
        }

        if let Some(data_dir) = dirs::data_dir() {
            return data_dir.join("xavier");
        }

        PathBuf::from("data")
    }

    pub fn load() -> Result<Option<Self>> {
        let path = Self::resolve_config_path();

        if !path.exists() {
            return Ok(None);
        }

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config file at {}", path.display()))?;
        
        // Handle both JSON and YAML/TOML if we want, but for now stick to what we have
        let parsed = if path.extension().is_some_and(|ext| ext == "toml") {
            // We need a TOML parser if we want to support .toml
            // But for now, let's assume JSON as per current implementation
            serde_json::from_str::<Self>(&raw)
                .with_context(|| format!("failed to parse TOML config (as JSON) at {}", path.display()))?
        } else {
            serde_json::from_str::<Self>(&raw)
                .with_context(|| format!("failed to parse config file at {}", path.display()))?
        };

        Ok(Some(parsed))
    }

    pub fn apply_to_env(&self) {
        if let Some(token) = &self.auth_token {
            set_if_absent("XAVIER_TOKEN", token);
        }
        set_if_absent("XAVIER_HOST", &self.server.host);
        set_if_absent("XAVIER_PORT", &self.server.port.to_string());
        set_if_absent("XAVIER_LOG_LEVEL", &self.server.log_level);
        set_if_absent("XAVIER_CODE_GRAPH_DB_PATH", &self.server.code_graph_db_path);

        set_if_absent(
            "XAVIER_DEFAULT_WORKSPACE_ID",
            &self.workspace.default_workspace_id,
        );
        set_if_absent("XAVIER_DEFAULT_PLAN", &self.workspace.default_plan);
        set_optional_if_absent(
            "XAVIER_STORAGE_LIMIT_BYTES",
            self.workspace.storage_limit_bytes.map(|v| v.to_string()),
        );
        set_optional_if_absent(
            "XAVIER_REQUEST_LIMIT",
            self.workspace.request_limit.map(|v| v.to_string()),
        );
        set_optional_if_absent(
            "XAVIER_REQUEST_UNIT_LIMIT",
            self.workspace.request_unit_limit.map(|v| v.to_string()),
        );
        set_if_absent(
            "XAVIER_EMBEDDING_PROVIDER_MODE",
            &self.workspace.embedding_provider_mode,
        );
        set_if_absent(
            "XAVIER_MANAGED_GOOGLE_EMBEDDINGS",
            if self.workspace.managed_google_embeddings {
                "1"
            } else {
                "0"
            },
        );
        set_if_absent("XAVIER_SYNC_POLICY", &self.workspace.sync_policy);

        set_if_absent("XAVIER_MEMORY_BACKEND", &self.memory.backend);
        set_if_absent("XAVIER_DATA_DIR", &self.memory.data_dir);
        set_if_absent(
            "XAVIER_EMBEDDING_DIMENSIONS",
            &self.memory.embedding_dimensions.to_string(),
        );
        set_if_absent("XAVIER_WORKSPACE_DIR", &self.memory.workspace_dir);
        set_if_absent("XAVIER_MEMORY_FILE_PATH", &self.memory.file_path);
        set_if_absent("XAVIER_MEMORY_SQLITE_PATH", &self.memory.sqlite_path);
        set_if_absent("XAVIER_MEMORY_VEC_PATH", &self.memory.vec_path);

        set_if_absent("XAVIER_MODEL_PROVIDER", &self.models.provider);
        set_if_absent("XAVIER_API_FLAVOR", &self.models.api_flavor);
        set_if_absent("XAVIER_LOCAL_LLM_URL", &self.models.local_llm_url);
        set_if_absent("XAVIER_LOCAL_LLM_MODEL", &self.models.local_llm_model);
        set_if_absent("XAVIER_EMBEDDING_URL", &self.models.embedding_url);
        set_if_absent("XAVIER_EMBEDDING_MODEL", &self.models.embedding_model);
        set_optional_if_absent(
            "XAVIER_ROUTER_RETRIEVED_MODEL",
            non_empty(&self.models.router_retrieved_model),
        );
        set_optional_if_absent(
            "XAVIER_ROUTER_COMPLEX_MODEL",
            non_empty(&self.models.router_complex_model),
        );
        set_if_absent("XAVIER_ROUTER_FAST_MODEL", &self.models.router_fast_model);
        set_if_absent("XAVIER_ROUTER_QUALITY_MODEL", &self.models.router_quality_model);

        set_if_absent(
            "XAVIER_DISABLE_HYDE",
            if self.retrieval.disable_hyde {
                "1"
            } else {
                "0"
            },
        );

        set_if_absent(
            "XAVIER_SYNC_INTERVAL_MS",
            &self.sync.interval_ms.to_string(),
        );
        set_if_absent(
            "XAVIER_SYNC_LAG_THRESHOLD_MS",
            &self.sync.lag_threshold_ms.to_string(),
        );
        set_if_absent(
            "XAVIER_SYNC_SAVE_OK_RATE_THRESHOLD",
            &self.sync.save_ok_rate_threshold.to_string(),
        );
        set_if_absent(
            "XAVIER_SYNC_MAX_RETRIES",
            &self.sync.max_retries.to_string(),
        );
        set_if_absent(
            "XAVIER_SYNC_RETRY_DELAY_MS",
            &self.sync.retry_delay_ms.to_string(),
        );
    }

    pub fn current() -> Self {
        let mut settings = Self::load().ok().flatten().unwrap_or_default();
        settings.auth_token = std::env::var("XAVIER_TOKEN").ok();
        settings
    }

    pub fn client_base_url(&self) -> String {
        let host = match self.server.host.as_str() {
            "0.0.0.0" | "::" => "127.0.0.1",
            other => other,
        };
        format!("http://{}:{}", host, self.server.port)
    }
}

fn set_if_absent(key: &str, value: &str) {
    if std::env::var_os(key).is_none() {
        std::env::set_var(key, value);
    }
}

fn set_optional_if_absent(key: &str, value: Option<String>) {
    if let Some(value) = value {
        set_if_absent(key, &value);
    }
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
