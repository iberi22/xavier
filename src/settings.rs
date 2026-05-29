use std::fmt;
use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

const DEFAULT_CONFIG_PATH: &str = "config/xavier.config.json";

#[derive(Clone, Deserialize, Default)]
pub struct XavierSettings {
    #[serde(default)]
    pub server: ServerSettings,
    #[serde(default)]
    pub workspace: WorkspaceSettings,
    #[serde(default)]
    pub memory: MemorySettings,
    #[serde(default)]
    pub memory_layers: MemoryLayersSettings,
    #[serde(default)]
    pub models: ModelSettings,
    #[serde(default)]
    pub retrieval: RetrievalSettings,
    #[serde(default)]
    pub sync: SyncSettings,
    #[serde(default)]
    pub embedding: EmbeddingSettings,
    #[serde(default)]
    pub security: SecuritySettings,
    #[serde(default)]
    pub telegram: TelegramSettings,
    #[serde(default)]
    pub router: RouterSettings,
    #[serde(default)]
    pub chronicle: ChronicleSettings,
    #[serde(default)]
    pub enterprise: EnterpriseSettings,
    #[serde(default)]
    pub agents: AgentSettings,
    #[serde(default)]
    pub advanced: AdvancedSettings,
    #[serde(skip)]
    pub auth_token: Option<String>,
}

impl fmt::Debug for XavierSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("XavierSettings")
            .field("server", &self.server)
            .field("workspace", &self.workspace)
            .field("memory", &self.memory)
            .field("memory_layers", &self.memory_layers)
            .field("models", &self.models)
            .field("retrieval", &self.retrieval)
            .field("sync", &self.sync)
            .field("embedding", &self.embedding)
            .field("security", &self.security)
            .field("telegram", &self.telegram)
            .field("router", &self.router)
            .field("chronicle", &self.chronicle)
            .field("enterprise", &self.enterprise)
            .field("agents", &self.agents)
            .field("advanced", &self.advanced)
            .field("auth_token", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerSettings {
    #[serde(default = "XavierSettings::default_host")]
    pub host: String,
    #[serde(default = "XavierSettings::default_port")]
    pub port: u16,
    #[serde(default = "XavierSettings::default_log_level")]
    pub log_level: String,
    #[serde(default = "XavierSettings::default_code_graph_db_path")]
    pub code_graph_db_path: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub config_path: Option<String>,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8006,
            log_level: "info".to_string(),
            code_graph_db_path: "data/code_graph.db".to_string(),
            url: String::new(),
            config_path: None,
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
            sqlite_path: data_dir
                .join("memory-store.sqlite3")
                .to_string_lossy()
                .to_string(),
            vec_path: data_dir
                .join("vec-store.sqlite3")
                .to_string_lossy()
                .to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkingMemoryLayerConfig {
    pub capacity: usize,
    pub lru_exempt_access_threshold: u32,
    pub bm25_k1: f32,
    pub bm25_b: f32,
}

impl Default for WorkingMemoryLayerConfig {
    fn default() -> Self {
        Self {
            capacity: 100,
            lru_exempt_access_threshold: 2,
            bm25_k1: 1.5,
            bm25_b: 0.75,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct EpisodicMemoryLayerConfig {
    pub summary_window: usize,
    pub max_sessions: usize,
    pub min_event_importance: f32,
}

impl Default for EpisodicMemoryLayerConfig {
    fn default() -> Self {
        Self {
            summary_window: 10,
            max_sessions: 50,
            min_event_importance: 0.5,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct MemoryLayersSettings {
    pub working: WorkingMemoryLayerConfig,
    pub episodic: EpisodicMemoryLayerConfig,
}

#[derive(Clone, Deserialize)]
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
    #[serde(default)]
    pub llm_model: Option<String>,
    #[serde(default)]
    pub llm_api_key: Option<String>,
    #[serde(default)]
    pub cloud_llm_model: Option<String>,
    #[serde(default)]
    pub cloud_llm_url: Option<String>,
    #[serde(default)]
    pub local_llm_api_key: Option<String>,
    #[serde(default)]
    pub local_anthropic_url: Option<String>,
}

impl fmt::Debug for ModelSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelSettings")
            .field("provider", &self.provider)
            .field("api_flavor", &self.api_flavor)
            .field("local_llm_url", &self.local_llm_url)
            .field("local_llm_model", &self.local_llm_model)
            .field("embedding_url", &self.embedding_url)
            .field("embedding_model", &self.embedding_model)
            .field("router_retrieved_model", &self.router_retrieved_model)
            .field("router_complex_model", &self.router_complex_model)
            .field("router_fast_model", &self.router_fast_model)
            .field("router_quality_model", &self.router_quality_model)
            .field("llm_model", &self.llm_model)
            .field("llm_api_key", &"[REDACTED]")
            .field("cloud_llm_model", &self.cloud_llm_model)
            .field("cloud_llm_url", &self.cloud_llm_url)
            .field("local_llm_api_key", &"[REDACTED]")
            .field("local_anthropic_url", &self.local_anthropic_url)
            .finish()
    }
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
            llm_model: None,
            llm_api_key: None,
            cloud_llm_model: None,
            cloud_llm_url: None,
            local_llm_api_key: None,
            local_anthropic_url: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RetrievalSettings {
    pub disable_hyde: bool,
    pub rrf_k: Option<u32>, // XAVIER_RRF_K
    pub zone_boost_multiplier: Option<f32>,
    pub zone_penalty_multiplier: Option<f32>,
}

impl Default for RetrievalSettings {
    fn default() -> Self {
        Self {
            disable_hyde: true,
            rrf_k: None,
            zone_boost_multiplier: None,
            zone_penalty_multiplier: None,
        }
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

#[derive(Clone, Deserialize)]
pub struct EmbeddingSettings {
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub embedder: String,
    #[serde(default)]
    pub gllm_model: String,
    #[serde(default)]
    pub api_flavor: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub gllm_dimension: Option<usize>,
    #[serde(default = "default_cache_enabled")]
    pub cache_enabled: bool,
    #[serde(default = "default_cache_size")]
    pub cache_size: u64,
    #[serde(default = "default_cache_ttl_hours")]
    pub cache_ttl_hours: u64,
    #[serde(default = "default_cache_db_path")]
    pub cache_db_path: String,
}

fn default_cache_enabled() -> bool {
    true
}
fn default_cache_size() -> u64 {
    10_000
}
fn default_cache_ttl_hours() -> u64 {
    24
}
fn default_cache_db_path() -> String {
    "data/embedding_cache.db".to_string()
}

impl fmt::Debug for EmbeddingSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EmbeddingSettings")
            .field("endpoint", &self.endpoint)
            .field("embedder", &self.embedder)
            .field("gllm_model", &self.gllm_model)
            .field("api_flavor", &self.api_flavor)
            .field("api_key", &"[REDACTED]")
            .field("gllm_dimension", &self.gllm_dimension)
            .finish()
    }
}

impl Default for EmbeddingSettings {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            embedder: String::new(),
            gllm_model: String::new(),
            api_flavor: "openai-compatible".to_string(),
            api_key: None,
            gllm_dimension: None,
            cache_enabled: true,
            cache_size: 10_000,
            cache_ttl_hours: 24,
            cache_db_path: "data/embedding_cache.db".to_string(),
        }
    }
}

#[derive(Clone, Deserialize, Default)]
pub struct SecuritySettings {
    pub allowed_domains: String,
    #[serde(default)]
    pub token_secret: Option<String>,
}

impl fmt::Debug for SecuritySettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecuritySettings")
            .field("allowed_domains", &self.allowed_domains)
            .field("token_secret", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Deserialize, Default)]
pub struct TelegramSettings {
    pub enabled: bool,
    #[serde(default)]
    pub bot_token: Option<String>,
}

impl fmt::Debug for TelegramSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelegramSettings")
            .field("enabled", &self.enabled)
            .field("bot_token", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouterSettings {
    pub policy_path: String,
    pub policy_refresh_secs: u64,
}

impl Default for RouterSettings {
    fn default() -> Self {
        Self {
            policy_path: String::new(),
            policy_refresh_secs: 300,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ChronicleSettings {
    pub model: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnterpriseSettings {
    pub db_path: String,
}

impl Default for EnterpriseSettings {
    fn default() -> Self {
        Self {
            db_path: "data/enterprise.db".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AgentSettings {
    pub weekly_budget: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AdvancedSettings {
    pub qjl_threshold: usize,
    pub entity_extraction_enabled: bool,
    pub audit_chain_enabled: bool,
    pub panel_store_dir: String,
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        Self {
            qjl_threshold: 500,
            entity_extraction_enabled: true,
            audit_chain_enabled: true,
            panel_store_dir: String::new(),
        }
    }
}

impl XavierSettings {
    /// Default values for serde field defaults
    fn default_host() -> String {
        "127.0.0.1".into()
    }
    fn default_port() -> u16 {
        8006
    }
    fn default_log_level() -> String {
        "info".into()
    }
    fn default_code_graph_db_path() -> String {
        "data/code_graph.db".into()
    }

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
            serde_json::from_str::<Self>(&raw).with_context(|| {
                format!(
                    "failed to parse TOML config (as JSON) at {}",
                    path.display()
                )
            })?
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
        set_optional_if_absent("XAVIER_URL", non_empty(&self.server.url));
        set_optional_if_absent("XAVIER_CONFIG_PATH", self.server.config_path.clone());

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
        set_optional_if_absent("XAVIER_RRF_K", self.retrieval.rrf_k.map(|v| v.to_string()));
        set_optional_if_absent(
            "XAVIER_ZONE_BOOST",
            self.retrieval.zone_boost_multiplier.map(|v| v.to_string()),
        );
        set_optional_if_absent(
            "XAVIER_ZONE_PENALTY",
            self.retrieval
                .zone_penalty_multiplier
                .map(|v| v.to_string()),
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

        // Memory layers
        set_if_absent(
            "XAVIER_WORKING_MEMORY_CAPACITY",
            &self.memory_layers.working.capacity.to_string(),
        );
        set_if_absent(
            "XAVIER_WORKING_LRU_THRESHOLD",
            &self
                .memory_layers
                .working
                .lru_exempt_access_threshold
                .to_string(),
        );
        set_if_absent(
            "XAVIER_WORKING_BM25_K1",
            &self.memory_layers.working.bm25_k1.to_string(),
        );
        set_if_absent(
            "XAVIER_WORKING_BM25_B",
            &self.memory_layers.working.bm25_b.to_string(),
        );
        set_if_absent(
            "XAVIER_EPISODIC_SUMMARY_WINDOW",
            &self.memory_layers.episodic.summary_window.to_string(),
        );
        set_if_absent(
            "XAVIER_MAX_EPISODIC_SESSIONS",
            &self.memory_layers.episodic.max_sessions.to_string(),
        );
        set_if_absent(
            "XAVIER_EPISODIC_MIN_EVENT_IMPORTANCE",
            &self.memory_layers.episodic.min_event_importance.to_string(),
        );

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
        set_if_absent(
            "XAVIER_ROUTER_QUALITY_MODEL",
            &self.models.router_quality_model,
        );
        set_optional_if_absent("XAVIER_LLM_MODEL", self.models.llm_model.clone());
        set_optional_if_absent("XAVIER_LLM_API_KEY", self.models.llm_api_key.clone());
        set_optional_if_absent(
            "XAVIER_CLOUD_LLM_MODEL",
            self.models.cloud_llm_model.clone(),
        );
        set_optional_if_absent("XAVIER_CLOUD_LLM_URL", self.models.cloud_llm_url.clone());
        set_optional_if_absent(
            "XAVIER_LOCAL_LLM_API_KEY",
            self.models.local_llm_api_key.clone(),
        );
        set_optional_if_absent(
            "XAVIER_LOCAL_ANTHROPIC_URL",
            self.models.local_anthropic_url.clone(),
        );

        set_if_absent(
            "XAVIER_DISABLE_HYDE",
            if self.retrieval.disable_hyde {
                "1"
            } else {
                "0"
            },
        );
        set_optional_if_absent(
            "XAVIER_ZONE_BOOST",
            self.retrieval.zone_boost_multiplier.map(|v| v.to_string()),
        );
        set_optional_if_absent(
            "XAVIER_ZONE_PENALTY",
            self.retrieval
                .zone_penalty_multiplier
                .map(|v| v.to_string()),
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

        // Embedding settings
        set_optional_if_absent(
            "XAVIER_EMBEDDING_ENDPOINT",
            non_empty(&self.embedding.endpoint),
        );
        set_optional_if_absent("XAVIER_EMBEDDER", non_empty(&self.embedding.embedder));
        set_optional_if_absent("XAVIER_GLLM_MODEL", non_empty(&self.embedding.gllm_model));
        set_optional_if_absent(
            "XAVIER_EMBEDDING_API_FLAVOR",
            non_empty(&self.embedding.api_flavor),
        );
        set_optional_if_absent("XAVIER_EMBEDDING_API_KEY", self.embedding.api_key.clone());
        set_optional_if_absent(
            "XAVIER_GLLM_DIMENSION",
            self.embedding.gllm_dimension.map(|v| v.to_string()),
        );

        // Embedding cache settings
        set_if_absent(
            "XAVIER_EMBEDDING_CACHE_ENABLED",
            if self.embedding.cache_enabled {
                "true"
            } else {
                "false"
            },
        );
        set_if_absent(
            "XAVIER_EMBEDDING_CACHE_SIZE",
            &self.embedding.cache_size.to_string(),
        );
        set_if_absent(
            "XAVIER_EMBEDDING_CACHE_TTL_HOURS",
            &self.embedding.cache_ttl_hours.to_string(),
        );
        set_optional_if_absent(
            "XAVIER_EMBEDDING_CACHE_DB_PATH",
            non_empty(&self.embedding.cache_db_path),
        );

        // Security settings
        set_optional_if_absent(
            "XAVIER_ALLOWED_DOMAINS",
            non_empty(&self.security.allowed_domains),
        );
        set_optional_if_absent("XAVIER_TOKEN_SECRET", self.security.token_secret.clone());

        // Telegram settings
        set_if_absent(
            "XAVIER_TELEGRAM_ENABLED",
            if self.telegram.enabled {
                "true"
            } else {
                "false"
            },
        );
        set_optional_if_absent("XAVIER_TELEGRAM_TOKEN", self.telegram.bot_token.clone());

        // Router settings
        set_optional_if_absent(
            "XAVIER_ROUTER_POLICY_PATH",
            non_empty(&self.router.policy_path),
        );
        set_if_absent(
            "XAVIER_ROUTER_POLICY_REFRESH_SECS",
            &self.router.policy_refresh_secs.to_string(),
        );

        // Chronicle settings
        set_optional_if_absent("Xavier_CHRONICLE_MODEL", non_empty(&self.chronicle.model));

        // Enterprise settings
        set_if_absent("XAVIER_ENTERPRISE_DB_PATH", &self.enterprise.db_path);

        // Agent settings
        set_optional_if_absent(
            "XAVIER_WEEKLY_BUDGET",
            self.agents.weekly_budget.map(|v| v.to_string()),
        );

        // Advanced settings
        set_if_absent(
            "XAVIER_QJL_THRESHOLD",
            &self.advanced.qjl_threshold.to_string(),
        );
        set_if_absent(
            "XAVIER_ENTITY_EXTRACTION_ENABLED",
            if self.advanced.entity_extraction_enabled {
                "1"
            } else {
                "0"
            },
        );
        set_if_absent(
            "XAVIER_AUDIT_CHAIN_ENABLED",
            if self.advanced.audit_chain_enabled {
                "1"
            } else {
                "0"
            },
        );
        set_optional_if_absent(
            "XAVIER_PANEL_STORE_DIR",
            non_empty(&self.advanced.panel_store_dir),
        );

        // Aliases for backward compatibility
        set_optional_if_absent("XAVIER_API_URL", non_empty(&self.server.url));
        if let Some(token) = &self.auth_token {
            set_if_absent("XAVIER_AUTH_TOKEN", token);
        }
        set_if_absent("XAVIER_WORKSPACE_ID", &self.workspace.default_workspace_id);
    }

    pub fn current() -> Self {
        let mut settings = Self::load().ok().flatten().unwrap_or_default();
        settings.auth_token = std::env::var("XAVIER_TOKEN").ok();
        // Populate sensitive fields from env if not set via config file
        if settings.security.token_secret.is_none() {
            settings.security.token_secret = std::env::var("XAVIER_TOKEN_SECRET").ok();
        }
        if settings.telegram.bot_token.is_none() {
            settings.telegram.bot_token = std::env::var("XAVIER_TELEGRAM_TOKEN").ok();
        }
        if settings.models.llm_api_key.is_none() {
            settings.models.llm_api_key = std::env::var("XAVIER_LLM_API_KEY").ok();
        }
        if settings.models.local_llm_api_key.is_none() {
            settings.models.local_llm_api_key = std::env::var("XAVIER_LOCAL_LLM_API_KEY").ok();
        }
        if settings.embedding.api_key.is_none() {
            settings.embedding.api_key = std::env::var("XAVIER_EMBEDDING_API_KEY").ok();
        }
        // Retrieval fallbacks
        if settings.retrieval.rrf_k.is_none() {
            settings.retrieval.rrf_k = std::env::var("XAVIER_RRF_K")
                .ok()
                .and_then(|v| v.parse().ok());
        }
        if settings.retrieval.zone_boost_multiplier.is_none() {
            settings.retrieval.zone_boost_multiplier = std::env::var("XAVIER_ZONE_BOOST")
                .ok()
                .and_then(|v| v.parse().ok());
        }
        if settings.retrieval.zone_penalty_multiplier.is_none() {
            settings.retrieval.zone_penalty_multiplier = std::env::var("XAVIER_ZONE_PENALTY")
                .ok()
                .and_then(|v| v.parse().ok());
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{LazyLock, Mutex};

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    #[test]
    fn test_default_settings() {
        let settings = XavierSettings::default();
        assert_eq!(settings.server.port, 8006);
        assert_eq!(settings.server.host, "127.0.0.1");
        assert_eq!(settings.workspace.default_workspace_id, "default");
        assert_eq!(settings.memory.backend, "vec");
        assert_eq!(settings.models.provider, "local");
        assert!(settings.retrieval.disable_hyde);
        assert_eq!(settings.sync.interval_ms, 300_000);
        assert_eq!(settings.advanced.qjl_threshold, 500);
        assert!(settings.advanced.entity_extraction_enabled);
        assert!(settings.advanced.audit_chain_enabled);
        assert_eq!(settings.memory_layers.working.capacity, 100);
        assert_eq!(settings.memory_layers.episodic.max_sessions, 50);
        assert!(!settings.telegram.enabled);
        assert_eq!(settings.enterprise.db_path, "data/enterprise.db");
    }

    #[test]
    fn test_apply_to_env_sets_vars() {
        let _guard = ENV_LOCK.lock().expect("test assertion");

        // Clean env
        for (key, _) in std::env::vars() {
            if key.starts_with("XAVIER_") || key == "STRIPE_SECRET_KEY" {
                std::env::remove_var(&key);
            }
        }

        let settings = XavierSettings::default();
        settings.apply_to_env();

        assert_eq!(std::env::var("XAVIER_HOST").unwrap(), "127.0.0.1");
        assert_eq!(std::env::var("XAVIER_PORT").unwrap(), "8006");
        assert_eq!(
            std::env::var("XAVIER_WORKING_MEMORY_CAPACITY").unwrap(),
            "100"
        );
        assert_eq!(std::env::var("XAVIER_QJL_THRESHOLD").unwrap(), "500");
        assert_eq!(std::env::var("XAVIER_TELEGRAM_ENABLED").unwrap(), "false");
        assert_eq!(
            std::env::var("XAVIER_ENTERPRISE_DB_PATH").unwrap(),
            "data/enterprise.db"
        );

        // Clean up
        for (key, _) in std::env::vars() {
            if key.starts_with("XAVIER_") {
                std::env::remove_var(&key);
            }
        }
    }

    #[test]
    fn test_apply_to_env_respects_existing_vars() {
        let _guard = ENV_LOCK.lock().expect("test assertion");

        // Set an override
        std::env::set_var("XAVIER_PORT", "9999");
        std::env::set_var("XAVIER_RRF_K", "100");
        std::env::remove_var("XAVIER_HOST");

        let settings = XavierSettings::default();
        settings.apply_to_env();

        // Existing vars should NOT be overwritten
        assert_eq!(std::env::var("XAVIER_PORT").unwrap(), "9999");
        assert_eq!(std::env::var("XAVIER_RRF_K").unwrap(), "100");
        // Missing vars should be set
        assert_eq!(std::env::var("XAVIER_HOST").unwrap(), "127.0.0.1");

        // Clean up
        std::env::remove_var("XAVIER_HOST");
        std::env::remove_var("XAVIER_PORT");
        std::env::remove_var("XAVIER_RRF_K");
    }

    #[test]
    fn test_config_file_missing_returns_none() {
        let path = std::env::temp_dir().join("nonexistent_xavier_config.json");
        // Ensure it doesn't exist
        let _ = std::fs::remove_file(&path);

        let _old_path = XavierSettings::resolve_config_path();

        // Temporarily redirect config path
        std::env::set_var("XAVIER_CONFIG_PATH", path.to_str().unwrap());
        let result = XavierSettings::load().unwrap();
        assert!(result.is_none());
        std::env::remove_var("XAVIER_CONFIG_PATH");
    }

    #[test]
    fn test_current_falls_back_to_defaults() {
        let _guard = ENV_LOCK.lock().expect("test assertion");

        // Remove XAVIER_TOKEN if present
        std::env::remove_var("XAVIER_TOKEN");

        // Temporarily point config to a nonexistent path so defaults are used
        std::env::set_var("XAVIER_CONFIG_PATH", "/tmp/nonexistent-xavier-config.json");
        let settings = XavierSettings::current();
        // Without a real config file, host falls to default (127.0.0.1)
        assert_eq!(settings.server.host, "127.0.0.1");
        assert_eq!(settings.server.port, 8006);
        assert!(settings.auth_token.is_none());
        std::env::remove_var("XAVIER_CONFIG_PATH");
    }

    #[test]
    fn test_non_empty_helper() {
        assert_eq!(non_empty(""), None);
        assert_eq!(non_empty("   "), None);
        assert_eq!(non_empty("hello"), Some("hello".to_string()));
        assert_eq!(non_empty("  world  "), Some("world".to_string()));
    }

    #[test]
    fn test_client_base_url() {
        let settings = XavierSettings::default();
        let url = settings.client_base_url();
        assert!(url.starts_with("http://"));
        assert!(url.contains("127.0.0.1"));
        assert!(url.contains("8006"));
    }

    #[test]
    fn test_resolve_data_dir() {
        // Without env var, should return platform data dir or "data"
        let data_dir = XavierSettings::resolve_data_dir();
        assert!(!data_dir.as_os_str().is_empty());
    }

    #[test]
    fn test_apply_to_env_all_new_sections() {
        let _guard = ENV_LOCK.lock().expect("test assertion");

        // Clean all XAVIER_ vars
        for (key, _) in std::env::vars() {
            if key.starts_with("XAVIER_") {
                std::env::remove_var(&key);
            }
        }

        let settings = XavierSettings::default();
        settings.apply_to_env();

        // Memory layers
        assert_eq!(
            std::env::var("XAVIER_WORKING_MEMORY_CAPACITY").unwrap(),
            "100"
        );
        assert_eq!(std::env::var("XAVIER_WORKING_LRU_THRESHOLD").unwrap(), "2");
        assert_eq!(std::env::var("XAVIER_WORKING_BM25_K1").unwrap(), "1.5");
        assert_eq!(std::env::var("XAVIER_WORKING_BM25_B").unwrap(), "0.75");
        assert_eq!(
            std::env::var("XAVIER_EPISODIC_SUMMARY_WINDOW").unwrap(),
            "10"
        );
        assert_eq!(std::env::var("XAVIER_MAX_EPISODIC_SESSIONS").unwrap(), "50");
        assert_eq!(
            std::env::var("XAVIER_EPISODIC_MIN_EVENT_IMPORTANCE").unwrap(),
            "0.5"
        );

        // Advanced
        assert_eq!(std::env::var("XAVIER_QJL_THRESHOLD").unwrap(), "500");
        assert_eq!(
            std::env::var("XAVIER_ENTITY_EXTRACTION_ENABLED").unwrap(),
            "1"
        );
        assert_eq!(std::env::var("XAVIER_AUDIT_CHAIN_ENABLED").unwrap(), "1");

        // Router
        assert_eq!(
            std::env::var("XAVIER_ROUTER_POLICY_REFRESH_SECS").unwrap(),
            "300"
        );

        // Enterprise
        assert_eq!(
            std::env::var("XAVIER_ENTERPRISE_DB_PATH").unwrap(),
            "data/enterprise.db"
        );

        // Clean up
        for (key, _) in std::env::vars() {
            if key.starts_with("XAVIER_") {
                std::env::remove_var(&key);
            }
        }
    }
}
