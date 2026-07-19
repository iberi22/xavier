//! CLI application state

use super::Command;
use clap::Parser;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use xavier::agents::rate_limit::RateLimitManager;
use xavier::app::proxy_use_case::ProxyUseCase;
use xavier::codebase::conversations_db::ConversationsDb;
use xavier::coordination::{KeyLendingEngine, XavierEventBus};
use xavier::embedding::Embedder;
use xavier::memory::qmd_memory::QmdMemory;
use xavier::memory::store::MemoryStore;
use xavier::ports::inbound::{
    AgentLifecyclePort, InputSecurityPort, MemoryQueryPort, SecurityScanPort,
};
use xavier::security::auth_store::AuthStore;
use xavier::security::sessions::SessionManager;
use xavier::tasks::store::{InMemoryTaskStore, TaskService};
use xavier::time::TimeMetricsStore;

#[derive(Clone)]
pub struct CodeGraphState {
    pub db: Arc<::code_graph::db::CodeGraphDB>,
    pub indexer: Arc<::code_graph::indexer::Indexer>,
    pub query: Arc<::code_graph::query::QueryEngine>,
}

#[derive(Clone)]
pub struct CliState {
    pub memory: Arc<dyn MemoryQueryPort>,
    pub qmd_memory: Arc<QmdMemory>,
    pub store: Arc<dyn MemoryStore>,
    pub workspace_id: String,
    pub workspace_dir: PathBuf,
    pub code_graph: Arc<tokio::sync::RwLock<CodeGraphState>>,
    pub security: Arc<dyn InputSecurityPort>,
    #[expect(
        dead_code,
        reason = "Reserved for future security scanning pipeline; wired via SecurityScanPort"
    )]
    pub security_scan: Arc<dyn SecurityScanPort>,
    pub _time_store: Option<Arc<TimeMetricsStore>>,
    pub agent_registry: Arc<dyn AgentLifecyclePort>,
    pub panel_store: Arc<ConversationsDb>,
    pub secrets_engine: Arc<KeyLendingEngine>,
    pub event_bus: XavierEventBus,
    pub tasks: Arc<TaskService<InMemoryTaskStore>>,
    pub rate_manager: Arc<RateLimitManager>,
    #[expect(
        dead_code,
        reason = "Implement structured prompt caching (keyed by session+model, auto-expire TTL)"
    )]
    pub prompt_cache: Arc<Mutex<HashMap<String, Vec<String>>>>,
    pub http_client: reqwest::Client,
    pub proxy_use_case: Arc<ProxyUseCase>,
    /// Process-local LLM proxy usage counters (shared with `proxy_use_case`).
    pub usage_counters: Arc<xavier::observability::UsageCounters>,
    pub session_manager: Arc<SessionManager>,
    pub provider_router: Arc<tokio::sync::RwLock<xavier::agents::provider::router::ProviderRouter>>,
    #[allow(dead_code)]
    pub embedder: Arc<dyn Embedder>,
    pub agent_indexer: Arc<crate::memory::agent_indexer::AgentIndexer>,
    pub auth_store: Option<Arc<AuthStore>>,
    pub openclaw_indexer: Arc<crate::memory::openclaw_indexer::OpenClawAgentIndexer>,
    pub system_scan_cache:
        Arc<tokio::sync::RwLock<Option<crate::cli::handlers::system_scan::SystemScanResult>>>,
}

impl CliState {
    pub fn auth_store(&self) -> Option<Arc<AuthStore>> {
        self.auth_store.clone()
    }

    pub async fn tgd_engine(&self) -> Option<xavier::tgd::TgdEngine> {
        let router = self.provider_router.read().await;
        let p_kind = router.active_mode();
        let config =
            xavier::agents::provider::ModelProviderConfig::from_label(&format!("{:?}", p_kind));
        let provider = xavier::agents::provider::ModelProviderClient::new(config);
        Some(xavier::tgd::TgdEngine::new(provider))
    }
}

#[derive(Parser)]
#[command(name = "xavier", version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Xavier - Fast Vector Memory for AI Agents", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Option<Command>,
}
