//! Logical concern: Xavier settings type definitions.
//!
//! This module contains all the configuration structs and their Debug implementations.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Serialize, Deserialize, Default)]
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
    #[serde(default)]
    pub pgheart: PgHeartSettings,
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
            .field("pgheart", &self.pgheart)
            .field("auth_token", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemorySettings {
    pub backend: String,
    pub data_dir: String,
    pub embedding_dimensions: usize,
    pub workspace_dir: String,
    pub file_path: String,
    pub sqlite_path: String,
    pub vec_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkingMemoryLayerConfig {
    pub capacity: usize,
    pub lru_exempt_access_threshold: u32,
    pub bm25_k1: f32,
    pub bm25_b: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EpisodicMemoryLayerConfig {
    pub summary_window: usize,
    pub max_sessions: usize,
    pub min_event_importance: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MemoryLayersSettings {
    pub working: WorkingMemoryLayerConfig,
    pub episodic: EpisodicMemoryLayerConfig,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
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
    #[serde(default)]
    pub compaction_model: Option<String>,
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
            .field("compaction_model", &self.compaction_model)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RetrievalSettings {
    pub disable_hyde: bool,
    pub rrf_k: Option<u32>, // XAVIER_RRF_K
    pub zone_boost_multiplier: Option<f32>,
    pub zone_penalty_multiplier: Option<f32>,
    pub cache_warming_enabled: bool,
    pub cache_warming_threshold: Option<f32>,
    #[serde(default)]
    pub learned_policy: NavigationPolicyConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavigationPolicyConfig {
    pub working_weight: f32,
    pub episodic_weight: f32,
    pub semantic_weight: f32,
    // Traversal weights
    pub semantic_similarity_weight: f32,
    pub confidence_weight: f32,
    pub edge_weight: f32,
    pub recency_weight: f32,
    pub cross_layer_weight: f32,
    pub cross_dir_weight: f32,
    pub peripheral_hub_weight: f32,

    pub learning_rate: f32,
    pub update_count: u64,
}

impl Default for NavigationPolicyConfig {
    fn default() -> Self {
        Self {
            working_weight: 0.3,
            episodic_weight: 0.3,
            semantic_weight: 0.4,
            semantic_similarity_weight: 0.5,
            confidence_weight: 0.1,
            edge_weight: 0.1,
            recency_weight: 0.1,
            cross_layer_weight: 0.05,
            cross_dir_weight: 0.1,
            peripheral_hub_weight: 0.05,
            learning_rate: 0.01,
            update_count: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncSettings {
    pub interval_ms: u64,
    pub lag_threshold_ms: u64,
    pub save_ok_rate_threshold: f32,
    pub max_retries: u32,
    pub retry_delay_ms: u64,
    pub min_health_interval_ms: u64,
    pub timeout_ms: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
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

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
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

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RouterSettings {
    pub policy_path: String,
    pub policy_refresh_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ChronicleSettings {
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EnterpriseSettings {
    pub db_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AgentSettings {
    pub weekly_budget: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AdvancedSettings {
    pub qjl_threshold: usize,
    pub entity_extraction_enabled: bool,
    pub audit_chain_enabled: bool,
    pub panel_store_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PgHeartSettings {
    pub url: Option<String>,
    pub token: Option<String>,
    pub instance_id: Option<String>,
    pub sync_interval_ms: u64,
    pub auto_heartbeat: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderConfigV2 {
    pub active_provider: String,
    pub auto_strategy: String,
    pub fallback_chain: Vec<String>,
    pub headless: HeadlessConfig,
    pub notifications: NotificationSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HeadlessConfig {
    pub enabled: bool,
    pub port: u16,
    pub auth_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NotificationSettings {
    pub provider_limit_warning: bool,
    pub new_model_detected: bool,
    pub better_provider_available: bool,
}

impl XavierSettings {
    pub fn default_host() -> String {
        "0.0.0.0".into()
    }
    pub fn default_port() -> u16 {
        8006
    }
    pub fn default_log_level() -> String {
        "info".into()
    }
    pub fn default_code_graph_db_path() -> String {
        "data/code_graph.db".into()
    }
}
