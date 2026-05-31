//! CLI application state

use super::Command;
use clap::Parser;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use xavier::agents::rate_limit::RateLimitManager;
use xavier::app::proxy_use_case::ProxyUseCase;
use xavier::coordination::{KeyLendingEngine, XavierEventBus};
use xavier::embedding::Embedder;
use xavier::memory::session_store::SessionStore;
use xavier::memory::store::MemoryStore;
use xavier::ports::inbound::{AgentLifecyclePort, MemoryQueryPort, InputSecurityPort, SecurityScanPort};
use xavier::tasks::store::{InMemoryTaskStore, TaskService};
use xavier::time::TimeMetricsStore;

#[derive(Clone)]
pub struct CliState {
    pub memory: Arc<dyn MemoryQueryPort>,
    pub store: Arc<dyn MemoryStore>,
    pub workspace_id: String,
    pub workspace_dir: PathBuf,
    pub code_db: Arc<::code_graph::db::CodeGraphDB>,
    pub code_indexer: Arc<::code_graph::indexer::Indexer>,
    pub code_query: Arc<::code_graph::query::QueryEngine>,
    pub security: Arc<dyn InputSecurityPort>,
    #[allow(dead_code)]
    pub security_scan: Arc<dyn SecurityScanPort>,
    pub _time_store: Option<Arc<TimeMetricsStore>>,
    pub agent_registry: Arc<dyn AgentLifecyclePort>,
    pub panel_store: Arc<SessionStore>,
    pub secrets_engine: Arc<KeyLendingEngine>,
    #[allow(dead_code)]
    // Note: Wire event_bus into event-driven architecture (e.g. system3 event bus integration)
    pub event_bus: XavierEventBus,
    #[allow(dead_code)]
    // Note: Migrate tasks from InMemoryTaskStore to persistent SQLite store
    pub tasks: Arc<TaskService<InMemoryTaskStore>>,
    pub rate_manager: Arc<RateLimitManager>,
    #[allow(dead_code)]
    // Note: Implement structured prompt caching (keyed by session+model, auto-expire TTL)
    pub prompt_cache: Arc<Mutex<HashMap<String, Vec<String>>>>,
    #[allow(dead_code)]
    // Note: Use http_client for background provider health checks (model status, rate limits)
    pub http_client: reqwest::Client,
    pub proxy_use_case: Arc<ProxyUseCase>,
    #[allow(dead_code)]
    // Note: Wire embedder into memory embedding pipeline (currently unused)
    pub embedder: Arc<dyn Embedder>,
    pub agent_indexer: Arc<crate::memory::agent_indexer::AgentIndexer>,
}

#[derive(Parser)]
#[command(name = "xavier", version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Xavier - Fast Vector Memory for AI Agents", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Option<Command>,
}
