//! HTTP server and WebSocket handlers

use anyhow::{anyhow, Result};
use axum::{
    extract::DefaultBodyLimit,
    middleware::{self},
    routing::{get, post},
    Router,
};
use axum_server::tls_rustls::RustlsConfig;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tracing::info;

use crate::cli::config::{
    code_graph_db_path, resolve_base_url_for_port, resolve_http_bind_host, resolve_http_token,
    state_panel_root,
};
use crate::cli::state::CliState;
use crate::observability::middleware::{request_logger, ObservabilityState};
use crate::settings::XavierSettings;
use xavier::adapters::inbound::http::routes::{
    sync_check_handler, time_metric_handler, verify_save_handler,
};
use xavier::adapters::outbound::http_health_adapter::HttpHealthAdapter;
use xavier::agents::rate_limit::RateLimitManager;
use xavier::app::proxy_use_case::ProxyUseCase;
use xavier::app::qmd_memory_adapter::QmdMemoryAdapter;
use xavier::app::security_service::SecurityService as AppSecurityService;
use xavier::codebase::connection_manager::ConnectionManager;
use xavier::codebase::conversations_db::ConversationsDb;
use xavier::coordination::SimpleAgentRegistry;
use xavier::coordination::{KeyLendingEngine, XavierEventBus};
use xavier::embedding::build_embedder_from_env;
use xavier::memory::qmd_memory::{MemoryDocument, QmdMemory};
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use xavier::memory::store::{MemoryRecord, MemoryStore};
use xavier::ports::inbound::{
    AgentLifecyclePort, InputSecurityPort, MemoryQueryPort, SecurityScanPort, TimeMetricsPort,
};
use xavier::security::sessions::SessionManager;
use xavier::security::threat_store::SecurityThreatStore;
use xavier::server::panel::{
    get_graph, list_bookmarks, list_widgets, panel_asset, panel_index, save_bookmark, save_graph,
    save_widget,
};
use xavier::tasks::session_sync_task::SessionSyncTask;
use xavier::tasks::store::{InMemoryTaskStore, TaskService};
use xavier::time::TimeMetricsStore;

pub use crate::cli::handlers::*;
pub use crate::cli::http_setup::*;
pub use crate::cli::types::*;
pub use crate::cli::websocket::*;

pub static START_TIME: once_cell::sync::Lazy<Instant> = once_cell::sync::Lazy::new(Instant::now);

pub async fn start_http_server(port: u16) -> Result<()> {
    let settings = XavierSettings::current();
    settings.apply_to_env();
    std::env::set_var("XAVIER_PORT", port.to_string());

    let bind_host = resolve_http_bind_host();
    let bind_addr = format!("{}:{}", bind_host, port);
    info!("Starting Xavier HTTP server on {}", bind_addr);
    let token = resolve_http_token()?;
    std::env::set_var("XAVIER_TOKEN", &token);

    let cm = ConnectionManager::global();

    let config = VecSqliteStoreConfig::from_env();
    if let Some(parent) = config.path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    cm.connect("memory", ".")?;
    cm.connect("vec_store", ".")?;
    cm.connect("metrics", ".")?;
    cm.set_active("default", ".").await?;

    let mut store_inner = VecSqliteMemoryStore::new(config.clone()).await?;
    let (event_tx, _) = tokio::sync::broadcast::channel(100);
    store_inner.set_event_tx(event_tx);
    let store = Arc::new(store_inner);

    let time_store = Arc::new(TimeMetricsStore::new());
    let audit_logger = Arc::new(xavier::secrets::audit::QmdAuditLogger::new());
    let rate_manager = Arc::new(RateLimitManager::new());
    let threat_store = Arc::new(SecurityThreatStore::new());

    time_store.init_schema_async().await?;
    audit_logger.init_schema_async().await?;
    rate_manager.init_schema_async().await?;
    threat_store.init_schema_async().await?;

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
            let _ = ConnectionManager::global()
                .with_conn("vec_store", |conn| {
                    conn.execute("PRAGMA incremental_vacuum(100)", ())?;
                    Ok(())
                })
                .await;
        }
    });

    let workspace_id = XavierSettings::current().workspace.default_workspace_id;
    let durable_state = store.load_workspace_state(&workspace_id).await?;
    let docs = Arc::new(RwLock::new(
        durable_state
            .memories
            .iter()
            .map(MemoryRecord::to_document)
            .collect::<Vec<MemoryDocument>>(),
    ));
    let memory = Arc::new(QmdMemory::new_with_workspace(docs, workspace_id.clone()));
    let dyn_store: Arc<dyn MemoryStore> = store.clone();
    memory.set_store(dyn_store.clone()).await;
    memory.init().await?;
    let memory_port =
        Arc::new(QmdMemoryAdapter::new(Arc::clone(&memory))) as Arc<dyn MemoryQueryPort>;

    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("xavier-server/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| anyhow!("Failed to build HTTP client: {}", e))?;

    let embedder = build_embedder_from_env()
        .await
        .map_err(|e| anyhow!("Failed to build embedder: {}", e))?;

    use xavier::adapters::inbound::http::routes::{init_health_port, init_time_store};
    use xavier::adapters::inbound::http::time_metrics_adapter::TimeMetricsAdapter;
    let health_adapter = Arc::new(HttpHealthAdapter::new(
        resolve_base_url_for_port(port),
        http_client.clone(),
    ));
    let time_adapter =
        Arc::new(TimeMetricsAdapter::new(Arc::clone(&time_store))) as Arc<dyn TimeMetricsPort>;
    init_time_store(time_adapter);
    init_health_port(health_adapter.clone());

    let code_db_path = code_graph_db_path();
    if let Some(parent) = code_db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let code_db = Arc::new(::code_graph::db::CodeGraphDB::new(&code_db_path)?);
    let code_indexer = Arc::new(::code_graph::indexer::Indexer::new(Arc::clone(&code_db)));
    let code_query = Arc::new(::code_graph::query::QueryEngine::new(Arc::clone(&code_db)));

    let workspace_dir = PathBuf::from(XavierSettings::current().memory.workspace_dir);
    info!(
        "Workspace root for path security: {}",
        workspace_dir.display()
    );

    let panel_root = state_panel_root(&workspace_dir, &workspace_id);
    let panel_store = Arc::new(ConversationsDb::open("default").await?);
    panel_store.create_schema().await?;
    panel_store.migrate_legacy_sessions(&panel_root).await?;

    let prompt_cache = Arc::new(parking_lot::Mutex::new(HashMap::new()));
    let security_service = Arc::new(AppSecurityService::new());
    let proxy_use_case = Arc::new(
        ProxyUseCase::new(rate_manager.clone(), prompt_cache.clone())
            .with_threat_detector(security_service.clone()),
    );
    let secrets_engine = Arc::new(KeyLendingEngine::new(Box::new(
        xavier::secrets::audit::QmdAuditLogger::new(),
    )));
    let event_bus = XavierEventBus::new(100);
    let tasks = Arc::new(
        TaskService::new(Arc::new(InMemoryTaskStore::new())).with_event_bus(event_bus.clone()),
    );

    let secrets_engine_for_bus = secrets_engine.clone();
    let mut receiver = event_bus.subscribe();
    tokio::spawn(async move {
        info!("Secrets engine listening for task events...");
        while let Ok(event) = receiver.recv().await {
            if let xavier::coordination::events::XavierEvent::TaskCompleted { task } = event {
                if let Some(agent_id) = &task.assignee {
                    info!(
                        "Task {} completed by agent {}. Revoking ephemeral keys...",
                        task.id, agent_id
                    );
                    secrets_engine_for_bus
                        .revoke_for_agent(agent_id, "Task Completed")
                        .await;
                }
            }
        }
    });

    let secrets_engine_for_cleanup = secrets_engine.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            let removed = secrets_engine_for_cleanup.cleanup_expired().await;
            if removed > 0 {
                info!("Cleaned up {} expired secret leases", removed);
            }
        }
    });

    let state = CliState {
        memory: memory_port,
        qmd_memory: Arc::clone(&memory),
        session_manager: Arc::new(SessionManager::new(60)),
        store,
        workspace_id,
        workspace_dir,
        code_db: code_db.clone(),
        code_indexer: code_indexer.clone(),
        code_query,
        security: security_service.clone() as Arc<dyn InputSecurityPort>,
        security_scan: security_service.clone() as Arc<dyn SecurityScanPort>,
        _time_store: Some(time_store),
        agent_registry: SimpleAgentRegistry::new() as Arc<dyn AgentLifecyclePort>,
        panel_store,
        secrets_engine,
        event_bus,
        tasks,
        rate_manager: rate_manager.clone(),
        prompt_cache,
        proxy_use_case,
        http_client,
        provider_router: Arc::new(tokio::sync::RwLock::new(
            xavier::agents::provider::router::ProviderRouter::new(
                xavier::agents::provider::router::ProviderKind::OpenAI,
            ),
        )),
        embedder,
        agent_indexer: Arc::new(crate::memory::agent_indexer::AgentIndexer::new(
            crate::memory::file_indexer::FileIndexer::new(
                crate::memory::file_indexer::FileIndexerConfig::default(),
                Some(code_indexer.clone()),
            ),
        )),
    };

    info!(
        "Memory store initialized for workspace: {}",
        state.workspace_id
    );

    let protected_routes = Router::new()
        .route("/memory/search", post(search_handler))
        .route("/memory/update", post(update_handler))
        .route("/memory/delete", post(delete_handler))
        .route("/memory/stats", get(stats_handler))
        .route("/memory/export", get(export_handler))
        .route("/memory/decay", post(decay_handler))
        .route("/memory/consolidate", post(consolidate_handler))
        .route("/memory/evict", axum::routing::delete(evict_handler))
        .route("/memory/manage", post(manage_handler))
        .route("/memory/timeline/query", post(timeline_query_handler))
        .route("/v1/memories", post(add_handler).get(stats_handler))
        .route("/v1/memories/search", post(search_handler))
        .route("/agents", get(agent_list_handler))
        .route("/workspace/default", get(workspace_info_handler))
        .route(
            "/v1/onboarding/suggestions",
            get(onboarding_suggestions_handler),
        )
        .route("/mcp/tools", get(mcp_tools_handler))
        .route("/code/find", post(code_find_handler))
        .route("/code/context", post(code_context_handler))
        .route("/code/stats", get(code_stats_handler))
        .route("/code/dependencies", post(code_dependencies_handler))
        .route(
            "/code/reverse-dependencies",
            post(code_reverse_dependencies_handler),
        )
        .route("/code/call-chain", post(code_call_chain_handler))
        .route("/code/hubs", get(code_hubs_handler))
        .route("/code/hotspots", get(code_hotspots_handler))
        .route("/v1/account/usage", get(account_usage_handler))
        .route("/v1/embeddings", post(embed_handler))
        .route("/v1/auth/session", post(session_create_handler))
        .route("/security/scan", post(security_scan_handler))
        .route("/memory/query", post(memory_query_handler))
        .route("/session/compact", post(session_compact_handler))
        .route("/api/skill/dispatch", post(xavier::api::skills::dispatch_skill))
        .route("/api/skill/list", get(xavier::api::skills::list_skills))
        .route("/api/memory/health", get(xavier::api::skills::memory_health))
        .route("/api/timeline/slice", post(xavier::api::timeline::get_time_slice))
        .route("/xavier/events/session", post(session_event_handler))
        .route("/xavier/time/metric", post(time_metric_handler))
        .route("/xavier/agents/register", post(agent_register_handler))
        .route("/xavier/agents/active", get(agent_active_handler))
        .route(
            "/xavier/agents/{id}/heartbeat",
            post(agent_heartbeat_handler),
        )
        .route("/xavier/agents/{id}/push", post(agent_push_context_handler))
        .route(
            "/xavier/agents/{id}/unregister",
            post(agent_unregister_handler),
        )
        .route("/xavier/sync/check", post(sync_check_handler))
        .route("/xavier/sync/check", get(sync_check_handler))
        .route("/xavier/verify/save", post(verify_save_handler))
        .route(
            "/panel/api/threads",
            get(panel_list_threads).post(panel_create_thread),
        )
        .route(
            "/panel/api/threads/{thread_id}",
            get(panel_get_thread).delete(panel_delete_thread),
        )
        .route(
            "/panel/api/bookmarks",
            get(list_bookmarks).post(save_bookmark),
        )
        .route("/panel/api/widgets", get(list_widgets).post(save_widget))
        .route("/panel/api/graph", get(get_graph).post(save_graph))
        .route("/secrets/lend", post(lend_handler))
        .route("/secrets/leases", get(leases_handler))
        .route("/secrets/revoke", post(revoke_handler))
        .route("/secrets/status/{token}", get(status_handler))
        .route(
            "/v1/proxy/chat/completions",
            post(crate::cli::proxy::chat_proxy),
        )
        .route(
            "/v1/proxy/chat/completions/batch",
            post(crate::cli::proxy::chat_batch_proxy),
        )
        .route("/v1/proxy/request", post(crate::cli::proxy::generic_proxy))
        .route("/v1/security/approve", post(security_approve_handler))
        .route("/v1/usage/status/{provider}", get(usage_status_handler))
        .route("/v1/usage/update", post(usage_update_handler))
        .route("/v1/usage/cooldown", post(usage_cooldown_handler))
        .route("/v1/usage/track", post(usage_track_handler))
        .route("/v1/usage/summary/{provider}", get(usage_summary_handler))
        .route(
            "/v1/providers/quota",
            get(crate::cli::handlers::quota::v1_providers_quota),
        )
        // ── Headless API (issue #624) ─────────────────────────────────────
        .route(
            "/v1/system/health",
            get(crate::cli::handlers::headless_api::headless_health),
        )
        .route(
            "/v1/system/scan",
            get(crate::cli::handlers::headless_api::headless_system_scan),
        )
        .route(
            "/v1/system/info",
            get(crate::cli::handlers::headless_api::headless_system_info),
        )
        .route(
            "/v1/chat/completions",
            post(crate::cli::handlers::headless_api::headless_chat),
        )
        .route(
            "/v1/providers",
            get(crate::cli::handlers::headless_api::headless_providers),
        )
        .route(
            "/v1/providers/status",
            get(crate::cli::handlers::headless_api::headless_provider_status),
        )
        .route(
            "/v1/providers/switch",
            post(crate::cli::handlers::headless_api::headless_switch_provider),
        )
        .route(
            "/v1/quota",
            get(crate::cli::handlers::headless_api::headless_quota),
        )
        .route(
            "/v1/usage",
            get(crate::cli::handlers::headless_api::headless_usage),
        )
        .route(
            "/v1/agents",
            get(crate::cli::handlers::headless_api::headless_agents),
        )
        .route(
            "/v1/agents/spawn",
            post(crate::cli::handlers::headless_api::headless_spawn),
        )
        .route(
            "/v1/memory/search",
            post(crate::cli::handlers::headless_api::headless_memory_search),
        )
        .route(
            "/v1/memory/add",
            post(crate::cli::handlers::headless_api::headless_memory_add),
        )
        .route(
            "/v1/memory/export",
            get(crate::cli::handlers::headless_api::headless_memory_export),
        )
        // ── Mesh API ──────────────────────────────────────────────────────
        .route("/v1/mesh/identity", get(xavier::server::v1_api::v1_mesh_identity))
        .route("/v1/mesh/handshake", post(xavier::server::v1_api::v1_mesh_handshake))
        .route("/v1/mesh/manifest", get(xavier::server::v1_api::v1_mesh_manifest))
        .route("/v1/mesh/chunks/request", post(xavier::server::v1_api::v1_mesh_chunks_request))
        .route("/v1/mesh/chunks/push", post(xavier::server::v1_api::v1_mesh_chunks_push))
        // ── Headless E2E API (New Structure) ──────────────────────────────
        .route(
            "/headless/health",
            get(crate::cli::handlers::headless_e2e::health),
        )
        .route(
            "/headless/context",
            get(crate::cli::handlers::headless_e2e::context),
        )
        .route(
            "/headless/memory/search",
            post(crate::cli::handlers::headless_e2e::memory_search),
        )
        .route(
            "/headless/tools",
            get(crate::cli::handlers::headless_e2e::tools),
        )
        .route(
            "/headless/tools/{name}",
            post(crate::cli::handlers::headless_e2e::execute_tool),
        )
        .route(
            "/headless/provider/status",
            get(crate::cli::handlers::headless_e2e::provider_status),
        )
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let large_body_routes = Router::new()
        .route("/memory/add", post(add_handler))
        .route("/memory/export-pack", post(export_pack_handler))
        .route("/panel/api/chat", post(panel_process_chat))
        .route("/code/scan", post(code_scan_handler))
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    #[cfg(feature = "enterprise")]
    let protected_routes = {
        use xavier::adapters::inbound::http::routes::{
            plugins_health_handler, plugins_sync_handler,
        };
        protected_routes
            .route("/plugins/health", get(plugins_health_handler))
            .route("/plugins/sync", post(plugins_sync_handler))
    };

    use axum::Extension;
    use xavier::agents::RuntimeConfig;
    use xavier::workspace::{WorkspaceConfig, WorkspaceContext, WorkspaceState};

    let workspace_config = WorkspaceConfig::from_env();
    let runtime_config = RuntimeConfig::from_env();
    let workspace_state = Arc::new(
        WorkspaceState::new(
            workspace_config,
            runtime_config,
            state.workspace_dir.clone(),
        )
        .await?,
    );
    let workspace_ctx = WorkspaceContext {
        workspace_id: state.workspace_id.clone(),
        workspace: workspace_state,
    };

    let app = Router::new()
        .route("/health", get(health_handler))
        .route(
            "/system/alerts",
            get(crate::cli::handlers::system::system_alerts_handler),
        )
        .route("/v1/version", get(version_handler))
        .route("/build", get(build_handler))
        .route("/ready", get(readiness_handler))
        .route("/readiness", get(readiness_handler))
        .route("/panel", get(panel_index))
        .route("/panel/assets/{*path}", get(panel_asset))
        .merge(protected_routes)
        .merge(large_body_routes)
        .layer(Extension(workspace_ctx))
        .layer(CorsLayer::permissive());

    // ── Observability middleware ──────────────────────────────────
    let obs_state = Arc::new(ObservabilityState::new());
    let obs_state_clone = obs_state.clone();

    // Initialize file logger (only once)
    let log_dir = std::path::PathBuf::from(std::env::var("XAVIER_LOG_DIR").unwrap_or_else(|_| {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_else(|_| ".".to_string());
        format!("{}/.xavier/logs", home)
    }));
    let log_level = std::env::var("XAVIER_LOG_LEVEL").unwrap_or_else(|_| "info".to_string());
    crate::observability::init_logger(&log_dir, &log_level);

    // Start the error pattern detector in background
    let detector_store = obs_state_clone.store.clone();
    tokio::spawn(async move {
        if let Some(ref _store) = detector_store {
            let detector = crate::observability::detector::LogDetector::new().await;
            if let Ok(d) = detector {
                let d = std::sync::Arc::new(d);
                tokio::spawn(async move {
                    d.spawn();
                });
            }
        }
    });

    // Log startup
    let notifier = crate::observability::notifier::Notifier::new();
    notifier.notify_startup();

    let agent_indexer_cron = state.agent_indexer.clone();
    let memory_port_cron = state.memory.clone();

    // ── Add observability middleware + routes ──────────────────
    let app = app
        .route(
            "/monitor/stats",
            get({
                let obs = obs_state.clone();
                axum::routing::get(move || {
                    let obs = obs.clone();
                    async move {
                        if let Some(ref store) = obs.store {
                            match store.get_stats().await {
                                Ok(stats) => (
                                    axum::http::StatusCode::OK,
                                    axum::Json(serde_json::json!({
                                        "status": "ok",
                                        "uptime_seconds": obs.uptime_seconds(),
                                        "stats": stats,
                                    })),
                                ),
                                Err(e) => (
                                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                                    axum::Json(serde_json::json!({
                                        "status": "error",
                                        "message": e.to_string(),
                                    })),
                                ),
                            }
                        } else {
                            (
                                axum::http::StatusCode::OK,
                                axum::Json(serde_json::json!({
                                    "status": "ok",
                                    "note": "ServiceLogStore not available (running without DB)",
                                    "uptime_seconds": obs.uptime_seconds(),
                                })),
                            )
                        }
                    }
                })
            }),
        )
        .route(
            "/monitor/errors",
            get({
                let obs = obs_state.clone();
                axum::routing::get(
                    move |axum::extract::Query(params): axum::extract::Query<
                        std::collections::HashMap<String, String>,
                    >| {
                        let obs = obs.clone();
                        async move {
                            let limit = params
                                .get("limit")
                                .and_then(|l| l.parse::<u32>().ok())
                                .unwrap_or(20);
                            if let Some(ref store) = obs.store {
                                match store.search_logs("error", limit).await {
                                    Ok(entries) => (
                                        axum::http::StatusCode::OK,
                                        axum::Json(serde_json::json!({
                                            "status": "ok",
                                            "count": entries.len(),
                                            "entries": entries,
                                        })),
                                    ),
                                    Err(e) => (
                                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                                        axum::Json(serde_json::json!({
                                            "status": "error",
                                            "message": e.to_string(),
                                        })),
                                    ),
                                }
                            } else {
                                (
                                    axum::http::StatusCode::OK,
                                    axum::Json(serde_json::json!({
                                        "status": "ok",
                                        "entries": serde_json::Value::Array(vec![]),
                                    })),
                                )
                            }
                        }
                    },
                )
            }),
        )
        .route(
            "/monitor/patterns",
            get({
                let obs = obs_state.clone();
                axum::routing::get(move || {
                    let obs = obs.clone();
                    async move {
                        if let Some(ref store) = obs.store {
                            match store.detect_patterns(60, 3).await {
                                Ok(patterns) => (
                                    axum::http::StatusCode::OK,
                                    axum::Json(serde_json::json!({
                                        "status": "ok",
                                        "patterns": patterns,
                                    })),
                                ),
                                Err(e) => (
                                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                                    axum::Json(serde_json::json!({
                                        "status": "error",
                                        "message": e.to_string(),
                                    })),
                                ),
                            }
                        } else {
                            (
                                axum::http::StatusCode::OK,
                                axum::Json(serde_json::json!({
                                    "status": "ok",
                                    "patterns": serde_json::Value::Array(vec![]),
                                    "note": "ServiceLogStore not available",
                                })),
                            )
                        }
                    }
                })
            }),
        )
        .layer(axum::middleware::from_fn_with_state(
            obs_state.clone(),
            request_logger,
        ));

    let app = app.with_state(state.clone());

    #[cfg(feature = "enterprise")]
    let app = {
        use std::sync::{Arc, Mutex};
        use xavier::enterprise::http::{enterprise_router, EnterpriseState};
        let enterprise_state = Arc::new(Mutex::new(EnterpriseState::init_default()));
        app.merge(
            enterprise_router(enterprise_state).layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
    };

    let listener = TcpListener::bind(&bind_addr).await?;
    let bound_addr = listener.local_addr()?;

    info!("Xavier HTTP server listening on http://{}", bound_addr);
    println!("Xavier HTTP server listening on http://{}", bound_addr);
    println!("Press Ctrl+C to stop");

    #[cfg(feature = "enterprise")]
    {
        use xavier::adapters::inbound::http::routes::init_plugin_registry;
        init_plugin_registry();
        info!("Enterprise plugin system initialized");
    }

    let sync_task = SessionSyncTask::with_storage(health_adapter, Some(dyn_store));
    let sync_shutdown = sync_task.spawn_cron_once();
    if sync_shutdown.is_some() {
        info!("SessionSyncTask cron started");
    } else {
        info!("SessionSyncTask cron already running; skipped duplicate start");
    }

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(6 * 3600));
        loop {
            interval.tick().await;
            info!("Running scheduled Agentic Scanner pass...");
            if let Ok(indexed_files) = agent_indexer_cron.index_agents().await {
                for file in indexed_files {
                    let record = xavier::memory::store::MemoryRecord {
                        id: uuid::Uuid::new_v4().to_string(),
                        workspace_id: "default".to_string(),
                        path: file.path,
                        content: file.content,
                        metadata: serde_json::json!({
                            "source": "agent_scanner",
                            "last_modified": file.last_modified,
                            "size": file.size,
                        }),
                        embedding: vec![],
                        created_at: chrono::Utc::now(),
                        updated_at: chrono::Utc::now(),
                        revision: 1,
                        primary: true,
                        parent_id: None,
                        cluster_id: None,
                        level: Default::default(),
                        relation: None,
                        clearance: Default::default(),
                        revisions: vec![],
                    };
                    let _ = memory_port_cron.add(record).await;
                }
            }
        }
    });

    if let (Ok(cert), Ok(key)) = (
        std::env::var("XAVIER_TLS_CERT"),
        std::env::var("XAVIER_TLS_KEY"),
    ) {
        info!("TLS 1.3 encryption enabled");
        let rustls_config = RustlsConfig::from_pem_file(cert, key).await?;

        let handle = axum_server::Handle::<std::net::SocketAddr>::new();
        let _shutdown_handle = handle.clone();

        let addr = listener.local_addr()?;
        axum_server::bind_rustls(addr, rustls_config)
            .serve(app.into_make_service())
            .await?;
    } else {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                if let Err(error) = tokio::signal::ctrl_c().await {
                    info!("Failed to listen for Ctrl+C shutdown signal: {}", error);
                }
                if let Some(shutdown) = sync_shutdown {
                    shutdown.shutdown();
                    shutdown.wait_for_shutdown(Duration::from_secs(5)).await;
                }
            })
            .await?;
    }

    Ok(())
}
