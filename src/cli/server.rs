//! HTTP server and WebSocket handlers

use anyhow::{anyhow, Result};
use axum::{
    extract::DefaultBodyLimit,
    middleware::{self},
    routing::{delete, get, post, put},
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
use tracing::{debug, info};

use crate::cli::config::{
    code_graph_db_path, resolve_base_url_for_port, resolve_http_bind_host, resolve_http_token,
    state_panel_root,
};
use crate::cli::state::CliState;
use xavier::api::graph::{
    memory_graph_entity, memory_graph_list_entities, memory_graph_relations, memory_graph_view,
};
use xavier::security::auth_store::AuthStore;

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
pub use xavier::auth2::auth_routes;

pub static START_TIME: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now);

/// Handler for Prometheus metrics endpoint
pub async fn metrics_handler() -> axum::response::Response {
    use axum::response::IntoResponse;
    match autometrics::encode_global_metrics() {
        Ok(metrics) => metrics.into_response(),
        Err(err) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to encode metrics: {:?}", err),
        )
            .into_response(),
    }
}

pub async fn start_http_server(port: u16, mcp_port: Option<u16>) -> Result<()> {
    // Initialize Prometheus exporter
    let _ = autometrics::global_metrics_exporter();

    // Initial health check run to populate the static HEALTH instance
    tokio::spawn(async {
        let _ = xavier::observability::health::HEALTH.run_checks().await;
    });
    Arc::clone(&*xavier::observability::health::HEALTH).spawn();

    let settings = XavierSettings::current();
    settings.apply_to_env();

    // Validate opencode CLI if active
    if settings.models.provider.trim().to_ascii_lowercase() == "opencode" {
        use std::process::Command;
        let opencode_exists = if cfg!(windows) {
            Command::new("where")
                .arg("opencode")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        } else {
            Command::new("which")
                .arg("opencode")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        };

        if !opencode_exists {
            return Err(anyhow!("'opencode' binary not found in PATH. It is required when XAVIER_MODEL_PROVIDER=opencode.\nInstallation: npm install -g @opencode/cli"));
        }
    }

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
    cm.connect("metrics", ".")?;
    cm.connect("security", ".")?;
    cm.set_active("default", ".").await?;

    // VecSqliteMemoryStore::new registers sqlite-vec (vec_f32) via sqlite3_auto_extension
    // *before* opening its hashed pool. Do not open the vec pool earlier or connections
    // will lack vec_f32 and memory/add will 500.
    let mut store_inner = VecSqliteMemoryStore::new(config.clone()).await?;
    let (event_tx, _) = tokio::sync::broadcast::channel(100);
    store_inner.set_event_tx(event_tx);
    let store = Arc::new(store_inner);
    let vec_project_id_for_vacuum = store.connection_project_id().to_string();

    let time_store = Arc::new(TimeMetricsStore::new());
    let audit_logger = Arc::new(xavier::secrets::audit::QmdAuditLogger::new());
    let rate_manager = Arc::new(RateLimitManager::new());
    let threat_store = Arc::new(SecurityThreatStore::new());

    let auth_db_path = format!(
        "{}/.xavier/auth.db",
        std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
    );
    let auth_store = Arc::new(AuthStore::open(auth_db_path, [0u8; 32])?); // Use actual key in prod

    time_store.init_schema_async().await?;
    audit_logger.init_schema_async().await?;
    rate_manager.init_schema_async().await?;
    threat_store.init_schema_async().await?;
    xavier::security::tokens::TokenStore::new()
        .init_schema_async()
        .await?;

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
            let _ = ConnectionManager::global()
                .with_conn(&vec_project_id_for_vacuum, |conn| {
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

    // Update the global health monitor with the embedder
    xavier::observability::health::HEALTH
        .set_embedder(embedder.clone())
        .await;
    if let Ok(peers) = xavier::mesh::PeerRegistry::load() {
        xavier::observability::health::HEALTH
            .set_peer_registry(Arc::new(peers))
            .await;
    }

    let code_db_path = code_graph_db_path();
    if let Some(parent) = code_db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let code_db = Arc::new(::code_graph::db::CodeGraphDB::new(&code_db_path)?);
    let code_indexer = Arc::new(::code_graph::indexer::Indexer::new(Arc::clone(&code_db)));
    let code_query = Arc::new(::code_graph::query::QueryEngine::new(Arc::clone(&code_db)));
    let code_graph_state = Arc::new(tokio::sync::RwLock::new(
        crate::cli::state::CodeGraphState {
            db: code_db.clone(),
            indexer: code_indexer.clone(),
            query: code_query.clone(),
        },
    ));

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
    let event_bus = XavierEventBus::new(100);
    let secrets_engine = Arc::new(KeyLendingEngine::new(
        Box::new(xavier::secrets::audit::QmdAuditLogger::new()),
        Some(event_bus.clone()),
    ));
    let tasks = Arc::new(
        TaskService::new(Arc::new(InMemoryTaskStore::new())).with_event_bus(event_bus.clone()),
    );

    let secrets_engine_for_bus = secrets_engine.clone();
    let mut receiver = event_bus.subscribe();
    tokio::spawn(async move {
        info!("Secrets engine listening for task events...");
        while let Ok(event) = receiver.recv().await {
            match event {
                xavier::coordination::events::XavierEvent::TaskCompleted { task } => {
                    let _ = xavier::notifications::NOTIFICATIONS
                        .notify(
                            xavier::notifications::IslandId::Agents,
                            "Agent Task Complete",
                            &format!(
                                "Task {} completed by agent {}.",
                                task.id,
                                task.assignee.as_deref().unwrap_or("unknown")
                            ),
                            "success",
                        )
                        .await;

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
                xavier::coordination::events::XavierEvent::TaskFailed { task, reason } => {
                    let _ = xavier::notifications::NOTIFICATIONS
                        .notify(
                            xavier::notifications::IslandId::Errors,
                            "Agent Task Failed",
                            &format!("Task {} failed: {}.", task.id, reason),
                            "error",
                        )
                        .await;

                    if let Some(agent_id) = &task.assignee {
                        info!(
                            "Task {} failed for agent {}. Revoking ephemeral keys...",
                            task.id, agent_id
                        );
                        secrets_engine_for_bus
                            .revoke_for_agent(agent_id, "Task Failed")
                            .await;
                    }
                }
                xavier::coordination::events::XavierEvent::AgentTaskCompleted {
                    agent_id, ..
                } => {
                    info!(
                        "Agent {} task completed. Revoking ephemeral keys...",
                        agent_id
                    );
                    secrets_engine_for_bus
                        .revoke_for_agent(&agent_id, "Agent Task Completed")
                        .await;
                }
                xavier::coordination::events::XavierEvent::AgentTaskFailed {
                    agent_id,
                    reason,
                    ..
                } => {
                    info!(
                        "Agent {} task failed ({}). Revoking ephemeral keys...",
                        agent_id, reason
                    );
                    secrets_engine_for_bus
                        .revoke_for_agent(&agent_id, &format!("Agent Task Failed: {}", reason))
                        .await;
                }
                _ => {}
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

    let configured_providers =
        xavier::agents::provider::config::ModelProviderConfig::get_all_configured()
            .iter()
            .map(|c| xavier::agents::provider::router::ProviderKind::from_str(&c.provider_label))
            .filter_map(|p| p)
            .collect::<Vec<_>>();

    let fallback_chain = xavier::agents::provider::router::ProviderRouter::build_default_chain(
        &configured_providers,
    )
    .await;
    let initial_provider = fallback_chain
        .first()
        .cloned()
        .unwrap_or(xavier::agents::provider::router::ProviderKind::OpenAI);

    let provider_router = xavier::agents::provider::router::ProviderRouter::new(initial_provider);
    let mut provider_router = provider_router;
    provider_router.set_fallback_chain(fallback_chain);
    // Log the chain BEFORE moving `provider_router` into the Arc below.
    let chain_str = provider_router
        .fallback_chain()
        .iter()
        .map(|k| k.as_str())
        .collect::<Vec<_>>()
        .join(" → ");
    info!("Provider fallback chain: [{}]", chain_str);
    println!("Provider fallback chain: [{}]", chain_str);
    let provider_router_shared = Arc::new(tokio::sync::RwLock::new(provider_router));

    let usage_counters = Arc::new(xavier::observability::UsageCounters::new());
    let proxy_use_case = Arc::new(
        ProxyUseCase::new(rate_manager.clone(), prompt_cache.clone())
            .with_usage_counters(usage_counters.clone())
            .with_threat_detector(security_service.clone())
            .with_provider_router(provider_router_shared.clone()),
    );

    let state = CliState {
        memory: memory_port,
        qmd_memory: Arc::clone(&memory),
        session_manager: Arc::new(SessionManager::new(60)),
        store,
        workspace_id,
        workspace_dir,
        code_graph: code_graph_state,
        security: security_service.clone() as Arc<dyn InputSecurityPort>,
        security_scan: security_service.clone() as Arc<dyn SecurityScanPort>,
        _time_store: Some(time_store),
        agent_registry: SimpleAgentRegistry::new_with_engines(
            Some(secrets_engine.clone()),
            Some(event_bus.clone()),
        ) as Arc<dyn AgentLifecyclePort>,
        panel_store,
        secrets_engine,
        event_bus,
        tasks,
        rate_manager: rate_manager.clone(),
        prompt_cache,
        proxy_use_case,
        usage_counters,
        http_client,
        provider_router: provider_router_shared,
        embedder: embedder.clone(),
        agent_indexer: Arc::new(crate::memory::agent_indexer::AgentIndexer::new(
            crate::memory::file_indexer::FileIndexer::new(
                crate::memory::file_indexer::FileIndexerConfig::default(),
                Some(code_indexer.clone()),
            ),
        )),
        auth_store: Some(auth_store),
        openclaw_indexer: Arc::new(crate::memory::openclaw_indexer::OpenClawAgentIndexer::new(
            embedder.clone(),
        )),
        system_scan_cache: Arc::new(tokio::sync::RwLock::new(None)),
    };

    info!(
        "Memory store initialized for workspace: {}",
        state.workspace_id
    );

    let protected_routes = Router::new()
        .route("/memory/search", post(search_handler))
        .route("/memory/update", post(update_handler))
        .route("/memory/delete", post(delete_handler))
        .route("/memory/reindex", post(reindex_handler))
        .route("/memory/stats", get(stats_handler))
        .route("/memory/export", get(export_handler))
        .route("/memory/decay", post(decay_handler))
        .route("/memory/consolidate", post(consolidate_handler))
        .route("/memory/index-self", post(memory_index_self_handler))
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
        .route("/v1/auth/sessions", get(list_sessions_handler))
        .route("/v1/auth/sessions/:id", delete(revoke_session_handler))
        .route("/mcp/tools", get(mcp_tools_handler))
        // Memory Knowledge Graph (EntityGraph)
        .route("/memory/graph/entities", get(memory_graph_list_entities))
        .route(
            "/memory/graph/entities/{entity_id}",
            get(memory_graph_entity),
        )
        .route("/memory/graph/relations", get(memory_graph_relations))
        .route("/memory/graph/view", get(memory_graph_view))
        .route("/code/index", post(code_index_handler))
        .route("/code/find", post(code_find_handler))
        .route("/code/context", post(code_context_handler))
        .route("/code/stats", get(code_stats_handler))
        .route("/code/dump", post(code_dump_handler))
        .route("/code/load", post(code_load_handler))
        .route("/code/dependencies", post(code_dependencies_handler))
        .route(
            "/code/reverse-dependencies",
            post(code_reverse_dependencies_handler),
        )
        .route("/code/call-chain", post(code_call_chain_handler))
        // Code graph canvas projection
        .route("/code/graph/view", get(code_graph_view_handler))
        .route("/code/hubs", get(code_hubs_handler))
        .route("/code/hotspots", get(code_hotspots_handler))
        .route("/v1/account/usage", get(account_usage_handler))
        .route("/v1/embeddings", post(embed_handler))
        .route("/v1/auth/session", post(session_create_handler))
        .nest(
            "/v1/auth",
            Router::new()
                .route("/register", post(register_handler))
                .route("/login", post(login_handler))
                .route("/totp/verify", post(totp_verify_handler))
                .route("/refresh", post(refresh_handler))
                .route("/recover", post(recover_handler))
                .layer(middleware::from_fn_with_state(
                    state.clone(),
                    rate_limit_middleware,
                )),
        )
        .route("/security/scan", post(security_scan_handler))
        .route("/memory/query", post(memory_query_handler))
        .route("/session/compact", post(session_compact_handler))
        .route(
            "/api/skill/dispatch",
            post(xavier::api::skills::dispatch_skill),
        )
        .route("/api/skill/list", get(xavier::api::skills::list_skills))
        .route("/skills", get(xavier::api::skills::list_skills))
        .route(
            "/api/memory/health",
            get(xavier::api::skills::memory_health),
        )
        .route(
            "/api/timeline/slice",
            post(xavier::api::timeline::get_time_slice),
        )
        .route("/timeline", get(xavier::api::timeline::timeline_summary))
        .route(
            "/api/settings/cloud-node",
            get(xavier::api::settings::get_cloud_node)
                .post(xavier::api::settings::update_cloud_node),
        )
        .route(
            "/api/settings/discord",
            get(xavier::api::settings::get_discord_settings)
                .post(xavier::api::settings::update_discord_settings),
        )
        .route(
            "/api/settings/discord/test",
            post(xavier::api::settings::test_discord_connection),
        )
        .route(
            "/api/settings/telegram",
            get(xavier::api::settings::get_telegram_settings)
                .post(xavier::api::settings::update_telegram_settings),
        )
        .route(
            "/api/settings/telegram/test",
            post(xavier::api::settings::test_telegram_connection),
        )
        .route("/xavier/events/session", post(session_event_handler))
        .route("/xavier/time/metric", post(time_metric_handler))
        .route("/xavier/agents/register", post(agent_register_handler))
        .route("/xavier/agents/active", get(agent_active_handler))
        .route("/xavier/agents/scan", get(agent_scan_handler))
        .route("/xavier/agents/index", post(agent_index_handler))
        .route("/xavier/openclaw/scan", get(openclaw_scan_handler))
        .route("/xavier/openclaw/index", post(openclaw_index_handler))
        .route("/xavier/agents/sync", post(agent_sync_handler))
        .route(
            "/xavier/agents/{id}/heartbeat",
            post(agent_heartbeat_handler),
        )
        .route("/xavier/agents/{id}/push", post(agent_push_context_handler))
        .route(
            "/xavier/agents/{id}/unregister",
            post(agent_unregister_handler),
        )
        .route(
            "/xavier/agents/{id}/task/complete",
            post(agent_task_complete_handler),
        )
        .route(
            "/xavier/agents/{id}/task/failed",
            post(agent_task_failed_handler),
        )
        .route("/xavier/sync/check", post(sync_check_handler))
        .route("/xavier/sync/check", get(sync_check_handler))
        .route("/xavier/verify/save", post(verify_save_handler))
        .route(
            "/v1/context/regenerate",
            post(xavier::server::http::context::v1_context_regenerate),
        )
        .route(
            "/v1/context/deepen",
            post(xavier::server::http::context::v1_context_deepen),
        )
        .route(
            "/v1/context/stats",
            get(xavier::server::http::context::v1_context_stats),
        )
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
        .route("/secrets/history", get(history_handler))
        .route(
            "/secrets/revoke/{token}",
            post(crate::cli::proxy::revoke_lease_by_path),
        )
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
        .route(
            "/security/tokens",
            get(list_tokens_handler).post(create_token_handler),
        )
        .route("/security/tokens/{id}", delete(revoke_token_handler))
        .route("/security/tokens/{id}/rotate", post(rotate_token_handler))
        .route("/auth/recovery/seed/show", post(seed_show_handler))
        .route("/auth/recovery/seed/verify", post(seed_verify_handler))
        .route(
            "/auth/recovery/backup-codes",
            post(backup_codes_generate_handler),
        )
        .route("/auth/recovery/reset", post(password_reset_handler))
        .route("/auth/recovery/master-key", post(master_key_handler))
        .route("/v1/usage/status/{provider}", get(usage_status_handler))
        .route("/v1/usage/update", post(usage_update_handler))
        .route("/v1/usage/cooldown", post(usage_cooldown_handler))
        .route("/v1/tasks", get(tasks_list_handler))
        .route("/v1/tasks/sync", post(tasks_sync_handler))
        .route("/v1/tasks/{id}/run", post(tasks_run_handler))
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
        // ── Ollama API (Model pull/list/set-active) ─────────────────────
        .route(
            "/v1/ollama/models",
            get(crate::cli::handlers::ollama_models::list_models_handler),
        )
        .route(
            "/v1/ollama/pull",
            post(crate::cli::handlers::ollama_models::pull_model_handler),
        )
        .route(
            "/v1/ollama/active",
            get(crate::cli::handlers::ollama_models::get_active_handler)
                .post(crate::cli::handlers::ollama_models::set_active_handler),
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
            post(crate::cli::handlers::memory::search_handler),
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
        .route(
            "/v1/mesh/identity",
            get(xavier::server::v1_api::v1_mesh_identity),
        )
        .route(
            "/v1/mesh/handshake",
            post(xavier::server::v1_api::v1_mesh_handshake),
        )
        .route(
            "/v1/mesh/cloud",
            get(xavier::server::v1_api::v1_mesh_cloud_get)
                .put(xavier::server::v1_api::v1_mesh_cloud_update),
        )
        .route(
            "/v1/mesh/data_commons/opt_in",
            get(xavier::server::v1_api::v1_mesh_data_commons_get)
                .post(xavier::server::v1_api::v1_mesh_data_commons_opt_in),
        )
        .route(
            "/v1/mesh/manifest",
            get(xavier::server::v1_api::v1_mesh_manifest),
        )
        .route(
            "/v1/mesh/chunks/request",
            post(xavier::server::v1_api::v1_mesh_chunks_request),
        )
        .route(
            "/v1/mesh/chunks/push",
            post(xavier::server::v1_api::v1_mesh_chunks_push),
        )
        .route(
            "/v1/sessions/{session_id}/export",
            get(xavier::server::v1_api::v1_session_export),
        )
        .route(
            "/v1/sessions/import",
            post(xavier::server::v1_api::v1_session_import),
        )
        .route(
            "/v1/mesh/session/{session_id}/share",
            post(xavier::server::v1_api::v1_mesh_session_share),
        )
        .route(
            "/v1/mesh/status",
            get(crate::cli::handlers::mesh::v1_mesh_status_handler),
        )
        .route("/v1/mesh/peers", get(list_peers_handler))
        .route("/v1/mesh/peers/pair", post(pair_peer_handler))
        .route("/v1/mesh/peers/decode", post(decode_pairing_code_handler))
        .route(
            "/v1/mesh/peers/generate-code",
            post(generate_pairing_code_handler),
        )
        .route("/v1/mesh/peers/{node_id}/acl", put(update_peer_acl_handler))
        .route("/v1/mesh/peers/{node_id}", delete(remove_peer_handler))
        .route(
            "/v1/mesh/workspaces/share",
            post(crate::cli::handlers::mesh::share_workspace_handler),
        )
        .route(
            "/v1/mesh/workspaces/join",
            post(crate::cli::handlers::mesh::join_workspace_handler),
        )
        .route(
            "/v1/mesh/workspaces/query",
            post(crate::cli::handlers::mesh::query_workspace_handler),
        )
        .route(
            "/v1/mesh/consent/revoke",
            post(crate::cli::handlers::mesh::revoke_consent_handler),
        )
        .route(
            "/v1/mesh/consent/list",
            get(crate::cli::handlers::mesh::list_consents_handler),
        )
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
        // ── Navigation API ────────────────────────────────────────────────
        .route(
            "/v1/nav/ls",
            get(crate::cli::handlers::navigation::ls_handler),
        )
        .route(
            "/v1/nav/cd",
            post(crate::cli::handlers::navigation::cd_handler),
        )
        .route(
            "/v1/nav/pwd",
            get(crate::cli::handlers::navigation::pwd_handler),
        )
        .route(
            "/v1/nav/affected",
            get(crate::cli::handlers::navigation::affected_handler),
        )
        .route(
            "/v1/nav/visualize",
            get(crate::cli::handlers::navigation::visualize_handler),
        )
        .route(
            "/v1/nav/telemetry",
            get(crate::cli::handlers::navigation::telemetry_handler),
        )
        .route(
            "/notifications",
            get(crate::cli::handlers::notifications::list_notifications_handler),
        )
        .route(
            "/notifications/{id}/read",
            axum::routing::patch(
                crate::cli::handlers::notifications::mark_notification_read_handler,
            ),
        )
        .route(
            "/notifications/read-all",
            axum::routing::patch(
                crate::cli::handlers::notifications::mark_all_notifications_read_handler,
            ),
        )
        .route(
            "/notifications/all",
            axum::routing::delete(
                crate::cli::handlers::notifications::delete_all_notifications_handler,
            ),
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
        .nest("/auth", auth_routes::<CliState>())
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .route("/health/cloud", get(cloud_health_handler))
        .route(
            "/system/alerts",
            get(crate::cli::handlers::system::system_alerts_handler),
        )
        .route("/v1/version", get(version_handler))
        .route("/build", get(build_handler))
        .route("/ready", get(readiness_handler))
        .route("/readiness", get(readiness_handler))
        .route("/v1/health/ready", get(readiness_handler))
        // Panel UI: Vite production build uses absolute `/assets/*` paths.
        // Serve index + assets at both `/` and `/panel` so portable installs and
        // bookmarked `/panel` URLs both work.
        .route("/", get(panel_index))
        .route("/panel", get(panel_index))
        .route("/assets/{*path}", get(panel_asset))
        .route("/panel/assets/{*path}", get(panel_asset))
        .merge(protected_routes)
        .merge(large_body_routes)
        .layer(Extension(workspace_ctx.clone()))
        .layer(CorsLayer::permissive());

    let agent_indexer_cron = state.agent_indexer.clone();
    let memory_port_cron = state.memory.clone();
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

    tracing::info!(
        target: "xavier::boot",
        event = "server_ready",
        version = env!("CARGO_PKG_VERSION"),
        provider = %std::env::var("XAVIER_MODEL_PROVIDER").unwrap_or_default(),
        llm_model = %std::env::var("XAVIER_LOCAL_LLM_MODEL").unwrap_or_default(),
        embedding_model = %std::env::var("XAVIER_EMBEDDING_MODEL").unwrap_or_default(),
        port = %port,
        "Xavier server ready"
    );

    // Operational mode summary (Issue #1-12)
    let mut final_status = xavier::observability::health::HEALTH.run_checks().await;

    // Real reachability check against Ollama
    let is_ollama_reachable =
        xavier::agents::provider::router::ProviderRouter::is_ollama_reachable().await;
    let local_config = xavier::agents::provider::ModelProviderConfig::for_provider("local");
    let is_local_reachable = local_config.is_reachable().await
        == xavier::agents::provider::types::ProviderReachability::ConfiguredAndReachable;

    // Check discrepancy
    if is_ollama_reachable || is_local_reachable {
        if !final_status.llm.reachable {
            tracing::warn!("Discrepancy detected: final_status says LLM is unreachable, but direct reachability checks are successful.");
            xavier::server::alerts::SYSTEM_ALERTS.push_alert(
                "WARN",
                "Ollama is reachable despite health check report",
                "llm",
            );
            // Sync final_status with the actual reality for printing
            final_status.llm.reachable = true;
        }
    } else {
        // If Ollama/local provider should be up but doesn't respond
        let provider_setting =
            std::env::var("XAVIER_PROVIDER").unwrap_or_else(|_| "local".to_string());
        if provider_setting == "local" || provider_setting == "ollama" {
            xavier::server::alerts::SYSTEM_ALERTS.push_alert(
                "ERROR",
                "Local provider (Ollama) is configured but unreachable",
                "llm",
            );
            final_status.llm.reachable = false;
        }
    }

    // Refresh mode based on potentially updated system alerts
    final_status.mode = xavier::server::alerts::SYSTEM_ALERTS.get_mode();

    let mode_icon = match final_status.mode {
        xavier::server::alerts::OperationalMode::LocalHealthy => "🟢",
        xavier::server::alerts::OperationalMode::LocalDegraded => "🟡",
        xavier::server::alerts::OperationalMode::CloudFallback => "🔵",
        xavier::server::alerts::OperationalMode::Disabled => "🔴",
    };
    let mode_str = match final_status.mode {
        xavier::server::alerts::OperationalMode::LocalHealthy => "LOCAL",
        xavier::server::alerts::OperationalMode::LocalDegraded => "LOCAL (DEGRADED)",
        xavier::server::alerts::OperationalMode::CloudFallback => "CLOUD",
        xavier::server::alerts::OperationalMode::Disabled => "DISABLED",
    };
    println!("{} Xavier iniciado — modo: {}", mode_icon, mode_str);
    println!(
        "   LLM:        {}/{} @ {} [{}]",
        final_status.llm.provider,
        final_status.llm.model,
        final_status.llm.endpoint,
        if final_status.llm.reachable {
            "reachable"
        } else {
            "unreachable"
        }
    );
    println!(
        "   Embeddings: {}/{} @ {} [{}]",
        final_status.embedding.provider,
        final_status.embedding.model,
        if final_status.embedding.provider.to_lowercase() == "openai" {
            "api.openai.com"
        } else {
            "localhost:11434"
        },
        if final_status.embedding.status == xavier::observability::health::HealthLevel::Healthy {
            "reachable"
        } else {
            "unreachable"
        }
    );
    println!(
        "   Vector DB:  {} ({})",
        final_status.vector_db.backend, final_status.vector_db.path
    );

    // Compact single-line summary log for terminal outputs:
    let llm_reach_str = if final_status.llm.reachable {
        "reachable"
    } else {
        "unreachable"
    };
    let emb_reach_str =
        if final_status.embedding.status == xavier::observability::health::HealthLevel::Healthy {
            "reachable"
        } else {
            "unreachable"
        };
    println!(
        "{} Xavier iniciado — modo: {} | LLM: {}/{} [{}] | Embeddings: {}/{} [{}]",
        mode_icon,
        mode_str,
        final_status.llm.provider,
        final_status.llm.model,
        llm_reach_str,
        final_status.embedding.provider,
        final_status.embedding.model,
        emb_reach_str
    );

    println!("Press Ctrl+C to stop");

    // Spawn the MCP HTTP+SSE (Streamable HTTP) server alongside the main API.
    // It shares the same memory store as stdio `xavier mcp` and exposes the
    // identical JSON-RPC tool surface for remote agents. Non-fatal: a bind
    // failure (e.g. port in use) is logged without taking down the main server.
    let mcp_port = crate::cli::config::resolve_mcp_port(mcp_port);
    if mcp_port > 0 {
        info!("Starting MCP HTTP+SSE server on port {}", mcp_port);
        tokio::spawn(async move {
            if let Err(error) = crate::cli::mcp::start_mcp_http(mcp_port).await {
                tracing::error!("MCP HTTP+SSE server on port {} failed: {}", mcp_port, error);
            }
        });
    } else {
        info!("MCP HTTP+SSE server disabled (resolved port 0)");
    }

    let _ = xavier::notifications::NOTIFICATIONS
        .notify(
            xavier::notifications::IslandId::System,
            "Xavier Started",
            &format!(
                "Xavier backend v{} started on port {}.",
                env!("CARGO_PKG_VERSION"),
                port
            ),
            "info",
        )
        .await;

    // Background System Scan (Ollama detection)
    let scan_cache = state.system_scan_cache.clone();
    tokio::spawn(async move {
        let interval_secs = std::env::var("XAVIER_SCAN_INTERVAL_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(300);
        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));

        loop {
            interval.tick().await;
            debug!("Running background system scan...");
            let result = crate::cli::handlers::system_scan::scan_system(true).await;

            if result.ollama.running {
                info!(
                    "🦙 Ollama detected: {} models ({})",
                    result.ollama.models.len(),
                    result.ollama.models.join(", ")
                );

                let default_model = "qwen3-coder";
                if !result
                    .ollama
                    .models
                    .iter()
                    .any(|m| m.contains(default_model))
                {
                    tracing::warn!(
                        "⚠️ Default model '{}' not found in Ollama. Run: ollama pull {}",
                        default_model,
                        default_model
                    );
                }
            } else if result.ollama.installed {
                debug!("Ollama is installed but not running.");
            }

            let mut cache = scan_cache.write().await;
            *cache = Some(result);
            drop(cache);
        }
    });

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

    // TGD Consolidation Scheduler (Nightly)
    if settings.tgd.enabled {
        let tgd_engine = state.tgd_engine().await;
        let tgd_state_path = state
            .workspace_dir
            .join(".xavier")
            .join("tgd_consolidation_state.json");
        let tgd_scheduler = Arc::new(xavier::tgd::TgdConsolidationScheduler::new(
            workspace_ctx.clone(),
            tgd_engine,
            tgd_state_path,
        ));
        let cron_expr = settings.tgd.schedule.clone();
        tgd_scheduler.clone().spawn(cron_expr).await;

        // Register TGD progress with health monitor
        xavier::observability::health::HEALTH
            .set_tgd_progress(tgd_scheduler.progress())
            .await;
    }

    #[cfg(feature = "telegram")]
    {
        let memory_bot = state.memory.clone();
        let agents_bot = state.agent_registry.clone();
        let security_bot = state.security_scan.clone();
        tokio::spawn(async move {
            xavier::telegram::run_bot(memory_bot, agents_bot, security_bot).await;
        });
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
                        score: 0.0,
                        parent_id: None,
                        cluster_id: None,
                        level: Default::default(),
                        relation: None,
                        clearance: Default::default(),
                        revisions: vec![],
                        encrypted_dek: None,
                        content_iv: None,
                        metadata_iv: None,
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
        use std::net::SocketAddr;
        axum_server::bind_rustls(addr, rustls_config)
            .serve(app.into_make_service_with_connect_info::<SocketAddr>())
            .await?;
    } else {
        use std::net::SocketAddr;
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
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
