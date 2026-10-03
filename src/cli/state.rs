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
    pub state_dir: PathBuf,
    pub auth_db: Option<Arc<Mutex<xavier::auth2::db::AuthDb>>>,
    pub code_graph: Arc<tokio::sync::RwLock<CodeGraphState>>,
    pub security: Arc<dyn InputSecurityPort>,
    #[allow(dead_code)]
    // Reserved for future security scanning pipeline; wired via SecurityScanPort
    pub security_scan: Arc<dyn SecurityScanPort>,
    pub _time_store: Option<Arc<TimeMetricsStore>>,
    pub agent_registry: Arc<dyn AgentLifecyclePort>,
    pub panel_store: Arc<ConversationsDb>,
    pub secrets_engine: Arc<KeyLendingEngine>,
    pub event_bus: XavierEventBus,
    pub tasks: Arc<TaskService<InMemoryTaskStore>>,
    pub rate_manager: Arc<RateLimitManager>,
    #[allow(dead_code)]
    // Implement structured prompt caching (keyed by session+model, auto-expire TTL)
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
    pub multi_db: xavier::storage::multi_db::MultiDbManager,
    pub maloca: Arc<xavier::maloca::MalocaStore>,
}

impl CliState {
    /// Auth store.
    pub fn auth_store(&self) -> Option<Arc<AuthStore>> {
        self.auth_store.clone()
    }

    /// Tgd engine.
    pub async fn tgd_engine(&self) -> Option<xavier::tgd::TgdEngine> {
        let router = self.provider_router.read().await;
        let p_kind = router.active_mode();
        let config =
            xavier::agents::provider::ModelProviderConfig::from_label(&format!("{:?}", p_kind));
        let provider = xavier::agents::provider::ModelProviderClient::new(config);
        Some(xavier::tgd::TgdEngine::new(provider))
    }
}

impl xavier::auth2::HasAuthDb for CliState {
    fn auth_db(&self) -> Option<Arc<Mutex<xavier::auth2::db::AuthDb>>> {
        self.auth_db.clone()
    }
}

/// `XAVIER_SPACES=off|0|false|disabled|no` turns the espacio API off
/// (routes answer 503, `xsp_` tokens 401). Default: on.
pub fn spaces_enabled() -> bool {
    match std::env::var("XAVIER_SPACES") {
        Ok(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "off" | "0" | "false" | "no" | "disabled"
        ),
        Err(_) => true,
    }
}

/// Open the node's one long-lived `SpaceManager` under `root` (the daemon
/// data dir; spaces live in `{root}/spaces/`), with the node master key as
/// KEK. Never fails boot: on a disabled switch or a key-init error it logs
/// and returns `None` and espacio stays off.
pub fn open_space_manager(root: &std::path::Path) -> Option<Arc<xavier::espacio::SpaceManager>> {
    if !spaces_enabled() {
        tracing::info!("espacio disabled (XAVIER_SPACES=off)");
        return None;
    }
    match xavier::espacio::MasterNodeKek::load_or_init() {
        Ok(kek) => open_space_manager_with_kek(root, Arc::new(kek)),
        Err(e) => {
            tracing::error!("espacio disabled: node key init failed: {e}");
            None
        }
    }
}

/// Same as [`open_space_manager`] with an injected KEK (tests).
pub fn open_space_manager_with_kek(
    root: &std::path::Path,
    kek: Arc<dyn xavier::espacio::NodeKek>,
) -> Option<Arc<xavier::espacio::SpaceManager>> {
    if !spaces_enabled() {
        tracing::info!("espacio disabled (XAVIER_SPACES=off)");
        return None;
    }
    if let Err(e) = std::fs::create_dir_all(root) {
        tracing::error!("espacio disabled: cannot create {}: {e}", root.display());
        return None;
    }
    let manager = Arc::new(xavier::espacio::SpaceManager::open_with_keys(root, kek));
    tracing::info!("espacio enabled, root {}", root.display());
    Some(manager)
}

/// The production `/api/v1/espacio` sub-router with the same middleware
/// stack as the rest of the protected API (rate limit, clearance, auth).
/// Pair it with [`install_space_manager`] on the OUTER router.
pub fn guarded_espacio_router(state: &CliState) -> axum::Router<CliState> {
    use axum::middleware;
    axum::Router::new()
        .nest(
            "/api/v1/espacio",
            xavier::adapters::inbound::http::routes::espacio_production_routes(),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            super::http_setup::rate_limit_middleware,
        ))
        .layer(middleware::from_fn(
            xavier::adapters::inbound::http::middleware::clearance::clearance_session_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            super::http_setup::auth_middleware,
        ))
}

/// Install the `Arc<SpaceManager>` request extension on the whole app. It
/// must sit OUTSIDE every `auth_middleware` layer: that middleware reads it
/// to verify `xsp_` tokens (without it every `xsp_` token is a 401). With
/// `None` espacio routes answer 503. Space tokens still only pass the narrow
/// own-space allowlist, so `/memory` and `/mcp` stay 403 for them.
///
/// The manager installed here also becomes the global one the MCP tools read
/// (`get_space_manager`), replacing any earlier instance, so the HTTP routes
/// and the tools never hold two divergent managers. `None` clears the global.
pub fn install_space_manager(
    app: axum::Router<CliState>,
    manager: Option<Arc<xavier::espacio::SpaceManager>>,
) -> axum::Router<CliState> {
    match manager {
        Some(m) => {
            xavier::adapters::inbound::http::routes::init_space_manager(m.clone());
            app.layer(axum::Extension(m))
        }
        None => {
            xavier::adapters::inbound::http::routes::clear_space_manager();
            app
        }
    }
}

#[derive(Parser)]
#[command(name = "xavier", version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Xavier - Fast Vector Memory for AI Agents", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Option<Command>,
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_cli_debug_assert() {
        use clap::CommandFactory;

        crate::cli::state::Cli::command().debug_assert();
    }
}
