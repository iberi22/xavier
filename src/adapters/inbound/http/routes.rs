//! HTTP route definitions for the Xavier API
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use axum::{
    extract::Json,
    response::IntoResponse,
    routing::{delete, get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

use crate::adapters::inbound::http::dto::TimeMetricDto;
use crate::adapters::outbound::http_health_adapter::HttpHealthAdapter;
use crate::agents::unregister_agent_handler;
use crate::coordination::SimpleAgentRegistry;
use crate::ports::inbound::{AgentLifecyclePort, TimeMetricsPort};
use crate::security::auth::Permission;
use crate::security::SecurityService;
use crate::session::event_mapper::PanelThreadEntry;
use crate::session::types::SessionEvent;
use crate::settings::XavierSettings;
use crate::tasks::session_sync_task::get_last_sync_result;
use crate::verification::auto_verifier::AutoVerifier;
use std::sync::LazyLock;
use std::time::Duration;

pub static LIB_HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("xavier-lib/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("failed to build library HTTP client")
});

// ─── Module-level TimeMetricsPort (initialized by CLI) ────────────────────────
static TIME_STORE: std::sync::OnceLock<Arc<dyn TimeMetricsPort>> = std::sync::OnceLock::new();

/// Module-level HttpHealthAdapter (initialized by CLI)
static HEALTH_PORT: std::sync::OnceLock<Arc<HttpHealthAdapter>> = std::sync::OnceLock::new();

/// Module-level SpaceManager (initialized by CLI or lazily)
static SPACE_MANAGER: std::sync::OnceLock<Arc<crate::espacio::SpaceManager>> =
    std::sync::OnceLock::new();

/// Module-level MalocaStore (initialized lazily)
static MALOCA_STORE: std::sync::OnceLock<Arc<crate::maloca::MalocaStore>> =
    std::sync::OnceLock::new();

/// Get the global maloca store, initializing with default path if needed
pub fn get_maloca_store() -> Arc<crate::maloca::MalocaStore> {
    MALOCA_STORE
        .get_or_init(|| crate::maloca::MalocaStore::open(std::path::Path::new("data/maloca")))
        .clone()
}

/// Initialize the global time metrics port (call once at startup)
pub fn init_time_store(port: Arc<dyn TimeMetricsPort>) {
    if TIME_STORE.set(port).is_err() {
        tracing::error!("TIME_STORE global already initialized (called init_time_store twice)");
    }
}

/// Initialize the global health check port (call once at startup)
pub fn init_health_port(port: Arc<HttpHealthAdapter>) {
    if HEALTH_PORT.set(port).is_err() {
        tracing::error!("HEALTH_PORT global already initialized (called init_health_port twice)");
    }
}

/// Initialize the global space manager (call once at startup)
pub fn init_space_manager(manager: Arc<crate::espacio::SpaceManager>) {
    if SPACE_MANAGER.set(manager).is_err() {
        tracing::error!(
            "SPACE_MANAGER global already initialized (called init_space_manager twice)"
        );
    }
}

/// Get the global space manager, initializing with default path if needed
pub fn get_space_manager() -> Arc<crate::espacio::SpaceManager> {
    SPACE_MANAGER
        .get_or_init(|| Arc::new(crate::espacio::SpaceManager::new("data/spaces")))
        .clone()
}

// ─── Router ─────────────────────────────────────────────────────────────────

/// Create router.
pub fn create_router() -> Router {
    create_router_with_agent_registry(SimpleAgentRegistry::new(None))
}

/// Create router with agent registry.
pub fn create_router_with_agent_registry(agent_registry: Arc<dyn AgentLifecyclePort>) -> Router {
    let router = Router::new()
        .route("/health", get(health_handler))
        .route("/health/history", get(health_history_handler))
        .route("/health/cloud", get(cloud_health_handler))
        .route("/readiness", get(readiness_handler))
        .route("/build", get(build_handler))
        .route(
            "/xavier/agents/{id}/unregister",
            post(unregister_agent_handler),
        )
        .route("/xavier/verify/save", post(verify_save_handler))
        .route("/xavier/time/metric", post(time_metric_handler))
        .route("/xavier/events/session", post(session_event_handler))
        .route("/xavier/sync/check", post(sync_check_handler))
        // ── Memory Sync (peer-to-peer memory synchronisation) ─────────────────
        .route(
            "/api/v1/memory/sync/push",
            post(crate::adapters::inbound::http::handlers::sync::sync_push_handler),
        )
        .route(
            "/api/v1/memory/sync/pull",
            post(crate::adapters::inbound::http::handlers::sync::sync_pull_handler),
        )
        .route(
            "/api/v1/memory/sync/status",
            get(crate::adapters::inbound::http::handlers::sync::sync_status_handler),
        )
        .route(
            "/api/v1/memory/sync/resolve/{conflict_id}",
            post(crate::adapters::inbound::http::handlers::sync::sync_resolve_handler),
        )
        // ── Legacy data-plane compatibility (backward-compatible aliases) ───
        // Old `/v1/memory/*` endpoints kept for backward compatibility with
        // existing PeerMemorySync clients. New code should use `/api/v1/memory/sync/*`.
        .route(
            "/v1/memory/manifest",
            get(crate::adapters::inbound::http::handlers::sync::legacy_manifest_handler),
        )
        .route(
            "/v1/memory/push",
            post(crate::adapters::inbound::http::handlers::sync::legacy_push_handler),
        )
        .route(
            "/v1/memory/pull",
            post(crate::adapters::inbound::http::handlers::sync::legacy_pull_handler),
        )
        .route(
            "/v1/memory/pull-since/{workspace_id}/{since}",
            get(crate::adapters::inbound::http::handlers::sync::legacy_pull_since_handler),
        )
        // ── Founder Node Attestation (SWAL Genesis Status) ─────────────────
        .route(
            "/node/founder/status",
            get(crate::node_identity::founder_status_handler),
        )
        .route(
            "/v1/node/founder/status",
            get(crate::node_identity::founder_status_handler),
        )
        // ── Public Node Directory (SWAL Node Discovery) ──────────────────
        .route(
            "/mesh/public/nodes",
            get(crate::adapters::inbound::http::handlers::nodes::list_public_nodes_handler),
        )
        .route(
            "/v1/mesh/public/nodes",
            get(crate::adapters::inbound::http::handlers::nodes::list_public_nodes_handler),
        )
        // ── Maintenance API ──────────────────────────────────────────────
        .route(
            "/v1/maintenance/reindex-embeddings",
            post(maintenance_reindex_handler)
                .get(maintenance_reindex_status_handler)
                .layer(axum::middleware::from_fn(
                    crate::middleware::require_permission(|r| r.can_edit_config()),
                )),
        )
        // ── Training Datasets API ─────────────────────────────────────────
        .route(
            "/v1/training/datasets",
            get(crate::adapters::inbound::http::handlers::training::list_datasets_handler),
        )
        .route(
            "/v1/training/datasets/{id}",
            get(crate::adapters::inbound::http::handlers::training::get_manifest_handler),
        )
        .route(
            "/v1/training/datasets/{id}/train",
            get(crate::adapters::inbound::http::handlers::training::get_train_split_handler),
        )
        .route(
            "/v1/training/datasets/{id}/eval",
            get(crate::adapters::inbound::http::handlers::training::get_eval_split_handler),
        )
        .route(
            "/v1/training/bundles",
            post(crate::adapters::inbound::http::handlers::training::create_bundle_handler),
        )
        .route("/v1/training/export", post(training_export_handler))
        // ── Content Redaction API ─────────────────────────────────────────
        .route("/v1/memories/redact", post(memories_redact_handler))
        // ── Mini-Experts API ──────────────────────────────────────────────
        .route("/v1/agents/mini-experts", get(mini_experts_list_handler))
        .route(
            "/v1/agents/mini-experts/invoke",
            post(mini_expert_invoke_handler),
        )
        // ── Data Marketplace API ──────────────────────────────────────────
        .route(
            "/v1/marketplace/datasets",
            post(crate::adapters::inbound::http::handlers::list_dataset_handler),
        )
        .route(
            "/v1/marketplace/datasets",
            get(crate::adapters::inbound::http::handlers::list_active_datasets_handler),
        )
        .route(
            "/v1/marketplace/datasets/{id}/query",
            post(crate::adapters::inbound::http::handlers::query_dataset_handler),
        )
        .route(
            "/v1/marketplace/datasets/{id}",
            delete(crate::adapters::inbound::http::handlers::revoke_dataset_handler),
        )
        .route(
            "/v1/marketplace/pricing",
            get(crate::adapters::inbound::http::handlers::get_pricing_preview_handler),
        )
        // ── Identity Verification Network (IVN) API ─────────────────────────
        .route(
            "/v1/identity/request",
            post(crate::adapters::inbound::http::handlers::create_identity_request_handler),
        )
        .route(
            "/v1/identity/request/{id}",
            get(crate::adapters::inbound::http::handlers::get_identity_request_handler),
        )
        .route(
            "/v1/identity/{id}/vote",
            post(crate::adapters::inbound::http::handlers::vote_identity_request_handler),
        )
        .route(
            "/v1/identity/requests",
            get(crate::adapters::inbound::http::handlers::list_identity_requests_handler),
        )
        .route(
            "/v1/identity/verified",
            get(crate::adapters::inbound::http::handlers::list_verified_nodes_handler),
        )
        .route(
            "/v1/ivn/karma/{agent}",
            get(crate::adapters::inbound::http::handlers::ivn::get_ivn_karma_handler),
        )
        // ── Espacio Runtime API ───────────────────────────────────────────
        .nest("/api/v1/espacio", espacio_routes())
        // ── Maloca API ────────────────────────────────────────────────────
        .nest(
            "/api/v1/maloca",
            crate::maloca::nested_router(get_maloca_store()),
        );

    // Add enterprise plugin routes if feature is enabled
    #[cfg(feature = "enterprise")]
    let router = router
        .route("/plugins/health", get(plugins_health_handler))
        .route("/plugins/sync", post(plugins_sync_handler));

    // Global rate limiting middleware (token_bucket per IP: 100 capacity, 60 req/min refill rate).
    let router = router.layer(axum::middleware::from_fn(
        crate::middleware::token_bucket::rate_limit_middleware,
    ));

    router.with_state(agent_registry)
}

#[derive(Debug, Serialize)]
pub struct RouteHealthSystem {
    pub cpu_usage_pct: f64,
    pub memory_used_mb: u64,
    pub memory_total_mb: u64,
    pub disk_usage_pct: f64,
}

#[derive(Debug, Serialize)]
pub struct RouteHealthDatabase {
    pub size_mb: f64,
    pub needs_vacuum: bool,
}

#[derive(Debug, Serialize)]
pub struct RouteHealthCheck {
    pub name: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct RouteHealthResponse {
    pub status: String,
    pub service: &'static str,
    pub version: String,
    pub uptime_secs: u64,
    pub system: RouteHealthSystem,
    pub database: RouteHealthDatabase,
    pub mesh: String,
    pub telegram: crate::health::TelegramHealth,
    pub dependency_graph: crate::health::ComponentDependencyGraph,
    pub checks: Vec<RouteHealthCheck>,
    pub embedding_coverage: crate::health::EmbeddingCoverage,
}

#[derive(Debug, Serialize)]
pub struct RouteReadinessResponse {
    pub status: &'static str,
    pub service: &'static str,
}

#[derive(Debug, Serialize)]
pub struct RouteBuildResponse {
    pub service: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Serialize)]
pub struct SessionEventDetection {
    pub is_injection: bool,
    pub confidence: f32,
    pub attack_type: String,
}

#[derive(Debug, Serialize)]
pub struct SessionEventResponse {
    pub status: &'static str,
    pub session_id: String,
    pub mapped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detection: Option<SessionEventDetection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_sanitized: Option<bool>,
}

async fn cloud_health_handler() -> impl axum::response::IntoResponse {
    let settings = XavierSettings::current();
    let health = crate::health::check_cloud_health(&settings).await;
    Json(health)
}

async fn health_history_handler() -> impl axum::response::IntoResponse {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let history = crate::health::history::fetch_health_history(now_secs);
    Json(history)
}

async fn health_handler() -> impl axum::response::IntoResponse {
    let health = crate::health::collect_health_sync();
    Json(RouteHealthResponse {
        status: health.status,
        service: "xavier",
        version: health.version,
        uptime_secs: health.uptime_secs,
        system: RouteHealthSystem {
            cpu_usage_pct: health.system.cpu_usage_pct,
            memory_used_mb: health.system.memory_used_mb,
            memory_total_mb: health.system.memory_total_mb,
            disk_usage_pct: health.system.disk_usage_pct,
        },
        database: RouteHealthDatabase {
            size_mb: health.database.size_mb,
            needs_vacuum: health.database.needs_vacuum,
        },
        mesh: health.mesh.connectivity,
        telegram: health.telegram,
        dependency_graph: health.dependency_graph,
        checks: health
            .checks
            .iter()
            .map(|c| RouteHealthCheck {
                name: c.name.clone(),
                status: format!("{:?}", c.status),
                detail: c.detail.clone(),
            })
            .collect(),
        embedding_coverage: health.embedding_coverage,
    })
}

async fn readiness_handler() -> impl axum::response::IntoResponse {
    Json(RouteReadinessResponse {
        status: "ok",
        service: "xavier",
    })
}

async fn build_handler() -> impl axum::response::IntoResponse {
    Json(RouteBuildResponse {
        service: "xavier",
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// Session event handler.
pub async fn session_event_handler(
    Json(event): Json<SessionEvent>,
) -> impl axum::response::IntoResponse {
    let Some(entry) = PanelThreadEntry::from_session_event(&event) else {
        return Json(SessionEventResponse {
            status: "ok",
            session_id: event.session_id,
            mapped: false,
            blocked: None,
            reason: None,
            detection: None,
            content_sanitized: None,
        })
        .into_response();
    };

    let security = SecurityService::new();
    let result = security.process_input(&entry.content);

    if !result.allowed {
        return Json(SessionEventResponse {
            status: "blocked",
            session_id: event.session_id,
            mapped: false,
            blocked: Some(true),
            reason: Some("security_policy_violation"),
            detection: Some(SessionEventDetection {
                is_injection: result.detection.is_injection,
                confidence: result.detection.confidence,
                attack_type: format!("{:?}", result.detection.attack_type),
            }),
            content_sanitized: None,
        })
        .into_response();
    }

    Json(SessionEventResponse {
        status: "ok",
        session_id: event.session_id,
        mapped: true,
        blocked: None,
        reason: None,
        detection: None,
        content_sanitized: Some(result.sanitized_input.is_some()),
    })
    .into_response()
}

// ─── Verification Endpoints ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct VerifySaveRequest {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct VerifySaveResponse {
    pub save_ok: bool,
    pub latency_ms: u64,
    pub match_score: f32,
}

/// Verify save handler.
pub async fn verify_save_handler(
    Json(payload): Json<VerifySaveRequest>,
) -> Json<VerifySaveResponse> {
    let start = Instant::now();

    let xavier_url =
        std::env::var("XAVIER_URL").unwrap_or_else(|_| XavierSettings::current().client_base_url());

    // Validate internal URL to prevent SSRF
    if let Err(e) = crate::security::url_validator::validate_internal_url(&xavier_url) {
        tracing::error!("Internal URL validation failed: {}", e);
        return Json(VerifySaveResponse {
            save_ok: false,
            latency_ms: start.elapsed().as_millis() as u64,
            match_score: 0.0,
        });
    }

    let auth_token = match std::env::var("XAVIER_TOKEN") {
        Ok(token) => token,
        Err(_) => {
            tracing::error!("XAVIER_TOKEN is required for verification requests");
            return Json(VerifySaveResponse {
                save_ok: false,
                latency_ms: start.elapsed().as_millis() as u64,
                match_score: 0.0,
            });
        }
    };

    let client = LIB_HTTP_CLIENT.clone();

    let result = AutoVerifier::verify_save(
        &client,
        &xavier_url,
        &auth_token,
        &payload.path,
        &payload.content,
    )
    .await;

    let elapsed = start.elapsed().as_millis() as u64;

    match result {
        Ok(vr) => Json(VerifySaveResponse {
            save_ok: vr.save_ok,
            latency_ms: elapsed,
            match_score: vr.match_score,
        }),
        Err(_) => Json(VerifySaveResponse {
            save_ok: false,
            latency_ms: elapsed,
            match_score: 0.0,
        }),
    }
}

// ─── Time Metrics Endpoint ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct TimeMetricResponse {
    pub status: String,
    pub metric_type: String,
    pub agent_id: String,
}

/// Time metric handler.
pub async fn time_metric_handler(Json(payload): Json<TimeMetricDto>) -> Json<TimeMetricResponse> {
    let workspace_id =
        std::env::var("XAVIER_WORKSPACE_ID").unwrap_or_else(|_| "default".to_string());

    // Try to save via TimeMetricsStore if available
    if let Some(time_store) = TIME_STORE.get() {
        let domain_metric: crate::domain::memory::TimeMetric = payload.clone().into();
        let result = time_store
            .save_time_metric(&domain_metric, &workspace_id)
            .await;
        match result {
            Ok(()) => {
                return Json(TimeMetricResponse {
                    status: "saved".to_string(),
                    metric_type: payload.metric_type,
                    agent_id: payload.agent_id,
                });
            }
            Err(e) => {
                tracing::warn!("TimeMetricsStore save error: {}", e);
            }
        }
    }

    Json(TimeMetricResponse {
        status: "ok".to_string(),
        metric_type: payload.metric_type,
        agent_id: payload.agent_id,
    })
}

// ─── Session Sync Check Endpoint ────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SyncCheckResponse {
    pub status: String,
    pub lag_ms: u64,
    pub save_ok_rate: f64,
    pub match_score: f64,
    pub active_agents: u64,
    pub timestamp_ms: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub alerts: Vec<String>,
}

/// Sync check handler.
pub async fn sync_check_handler() -> Json<SyncCheckResponse> {
    // Return cached sync check results from the SessionSyncTask cron.
    let result = get_last_sync_result();

    Json(SyncCheckResponse {
        status: result.status,
        lag_ms: result.lag_ms,
        save_ok_rate: result.save_ok_rate,
        match_score: result.match_score,
        active_agents: result.active_agents,
        timestamp_ms: result.timestamp_ms,
        alerts: result.alerts,
    })
}

// ─── Enterprise Plugin Endpoints ────────────────────────────────────────────
// These endpoints are only available when the "enterprise" feature is enabled

#[cfg(feature = "enterprise")]
#[derive(Debug, Serialize)]
pub struct PluginsHealthResponse {
    pub status: String,
    pub plugins: Vec<PluginHealthStatus>,
}

#[cfg(feature = "enterprise")]
#[derive(Debug, Serialize)]
pub struct PluginHealthStatus {
    pub name: String,
    pub version: String,
    pub healthy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[cfg(feature = "enterprise")]
#[derive(Debug, Deserialize)]
pub struct PluginSyncRequest {
    pub direction: String, // "push", "pull", or "both"
}

#[cfg(feature = "enterprise")]
#[derive(Debug, Serialize)]
pub struct PluginSyncResponse {
    pub status: String,
    pub results: Vec<PluginSyncResult>,
}

#[cfg(feature = "enterprise")]
#[derive(Debug, Serialize)]
pub struct PluginSyncResult {
    pub plugin_name: String,
    pub success: bool,
    pub items_synced: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Plugin registry singleton (lazy-initialized)
#[cfg(feature = "enterprise")]
static PLUGIN_REGISTRY: std::sync::OnceLock<
    std::sync::Arc<tokio::sync::RwLock<crate::adapters::inbound::http::plugins::PluginRegistry>>,
> = std::sync::OnceLock::new();

/// Initialize the plugin registry (currently no plugins auto-registered)
#[cfg(feature = "enterprise")]
pub fn init_plugin_registry() {
    use crate::adapters::inbound::http::plugins::PluginRegistry;

    let registry = PluginRegistry::new();

    tracing::debug!("Plugin registry initialized (no auto-registered plugins)");

    let registry_arc = std::sync::Arc::new(tokio::sync::RwLock::new(registry));
    if PLUGIN_REGISTRY.set(registry_arc).is_err() {
        tracing::error!("Plugin registry already initialized");
    }
}

// ─── Maintenance API ──────────────────────────────────────────────────────

use crate::codebase::connection_manager::ConnectionManager;
use crate::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ReindexMaintenanceRequest {
    #[serde(default = "default_dry_run")]
    pub dry_run: bool,
    pub limit: Option<usize>,
}

fn default_dry_run() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct ReindexMaintenanceResponse {
    pub status: String,
    pub dry_run: bool,
    pub null_embeddings_count: usize,
    pub processed_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_applied: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct ReindexStatusResponse {
    pub status: String,
    pub is_running: bool,
    pub total_records: usize,
    pub processed_count: usize,
    pub success_count: usize,
    pub failed_count: usize,
}

/// GET /v1/maintenance/reindex-embeddings — query reindexing status and progress.
pub async fn maintenance_reindex_status_handler() -> impl axum::response::IntoResponse {
    use crate::memory::sqlite_vec_store::schema_impl::*;
    use std::sync::atomic::Ordering;

    Json(ReindexStatusResponse {
        status: "ok".to_string(),
        is_running: REINDEX_RUNNING.load(Ordering::SeqCst),
        total_records: REINDEX_TOTAL.load(Ordering::SeqCst),
        processed_count: REINDEX_PROCESSED.load(Ordering::SeqCst),
        success_count: REINDEX_SUCCESS.load(Ordering::SeqCst),
        failed_count: REINDEX_FAILED.load(Ordering::SeqCst),
    })
}

/// POST /v1/maintenance/reindex-embeddings — trigger reindexing of memories lacking embeddings.
pub async fn maintenance_reindex_handler(
    Json(payload): Json<ReindexMaintenanceRequest>,
) -> impl axum::response::IntoResponse {
    use crate::memory::sqlite_vec_store::schema_impl::REINDEX_RUNNING;
    use std::sync::atomic::Ordering;

    let (effective_limit, limit_applied) = match payload.limit {
        Some(lim) => (Some(lim), None),
        None => (Some(500), Some(500)),
    };

    if !payload.dry_run && REINDEX_RUNNING.swap(true, Ordering::SeqCst) {
        return (
            axum::http::StatusCode::CONFLICT,
            Json(serde_json::json!({ "status": "already_running" })),
        )
            .into_response();
    }

    let store_config = VecSqliteStoreConfig::from_env();

    match VecSqliteMemoryStore::new(store_config).await {
        Ok(store) => {
            let project_id_c = store.connection_project_id().to_string();
            let count_res: anyhow::Result<usize> = ConnectionManager::global()
                .with_conn(&project_id_c, move |conn| {
                    let count: usize = conn.query_row(
                        "SELECT COUNT(*) FROM memory_records WHERE embedding IS NULL",
                        [],
                        |row| row.get(0),
                    )?;
                    Ok(count)
                })
                .await;

            let null_count = count_res.unwrap_or(0);

            if payload.dry_run {
                Json(ReindexMaintenanceResponse {
                    status: "ok".to_string(),
                    dry_run: true,
                    null_embeddings_count: null_count,
                    processed_count: 0,
                    limit_applied,
                })
                .into_response()
            } else {
                // Spawn the background reindexing task!
                tokio::spawn(async move {
                    tracing::info!(
                        "Starting triggered background reindexing (limit: {:?})...",
                        effective_limit
                    );
                    match store
                        .reindex_null_embeddings_background_with_limit(effective_limit)
                        .await
                    {
                        Ok(success_count) => {
                            tracing::info!(
                                "Triggered background reindexing completed. Success count: {}",
                                success_count
                            );
                        }
                        Err(e) => {
                            tracing::error!("Triggered background reindexing failed: {}", e);
                        }
                    }
                });

                let processed_expected = effective_limit.unwrap_or(null_count).min(null_count);

                Json(ReindexMaintenanceResponse {
                    status: "reindexing_started".to_string(),
                    dry_run: false,
                    null_embeddings_count: null_count,
                    processed_count: processed_expected,
                    limit_applied,
                })
                .into_response()
            }
        }
        Err(e) => {
            if payload.dry_run {
                Json(ReindexMaintenanceResponse {
                    status: "ok".to_string(),
                    dry_run: true,
                    null_embeddings_count: 0,
                    processed_count: 0,
                    limit_applied,
                })
                .into_response()
            } else {
                REINDEX_RUNNING.store(false, Ordering::SeqCst);
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to initialize memory store for reindexing: {}", e),
                )
                    .into_response()
            }
        }
    }
}

// ─── Training Datasets API ─────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct TrainingExportRequest {
    /// Deterministic seed for reproducible splits.
    #[serde(default)]
    pub seed: u64,
    /// Fraction of records for the eval split (0.0..1.0).
    #[serde(default)]
    pub eval_ratio: f32,
    #[serde(default)]
    pub curated_only: Option<bool>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct TrainingExportResponse {
    pub manifest: serde_json::Value,
    pub train_count: usize,
    pub eval_count: usize,
    pub audit: TrainingAuditDto,
}

#[derive(Debug, Serialize)]
pub struct TrainingAuditDto {
    pub total_records_found: usize,
    pub included_records: usize,
    pub excluded_no_consent: usize,
    pub excluded_revoked: usize,
}

/// POST /v1/training/export — generate a training bundle from telemetry data or curated items.
pub async fn training_export_handler(
    Json(payload): Json<TrainingExportRequest>,
) -> impl axum::response::IntoResponse {
    if payload.curated_only.unwrap_or(false) {
        let queue = crate::curation::CurationQueue::load().unwrap_or_default();
        let mut approved_items = queue.curated_dataset();

        if payload.seed > 0 || payload.eval_ratio > 0.0 {
            use rand::seq::SliceRandom;
            use rand::SeedableRng;
            let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(payload.seed);
            approved_items.shuffle(&mut rng);
        }

        if let Some(limit) = payload.limit {
            approved_items.truncate(limit);
        }

        let redactor = crate::security::redaction::RedactionEngine::default();
        let mut lines = Vec::new();

        for item in approved_items {
            let redacted = redactor.redact(&item.content_ref);
            let obj = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&redacted) {
                parsed
            } else {
                serde_json::json!({
                    "text": redacted,
                    "id": item.id,
                    "clearance": item.proposed_clearance
                })
            };
            if let Ok(line_str) = serde_json::to_string(&obj) {
                lines.push(line_str);
            }
        }

        let body_str = if lines.is_empty() {
            String::new()
        } else {
            lines.join("\n") + "\n"
        };

        return axum::response::Response::builder()
            .header("content-type", "application/x-ndjson")
            .body(axum::body::Body::from(body_str))
            .unwrap_or_else(|e| {
                (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
            });
    }

    let db_path = std::path::PathBuf::from(
        std::env::var("XAVIER_TELEMETRY_DB_PATH")
            .unwrap_or_else(|_| ".xavier/telemetry.db".to_string()),
    );

    if !db_path.exists() {
        return (
            axum::http::StatusCode::NOT_FOUND,
            format!("Telemetry DB not found at {}", db_path.display()),
        )
            .into_response();
    }

    let exporter = crate::data_commons::training::TrainingExporter::new(&db_path);
    let bundle = match exporter.generate_bundle(payload.seed, payload.eval_ratio, None) {
        Ok(b) => b,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };

    let manifest = match serde_json::to_value(&bundle.manifest) {
        Ok(m) => m,
        Err(e) => {
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    };

    Json(TrainingExportResponse {
        manifest,
        train_count: bundle.train_split.len(),
        eval_count: bundle.eval_split.len(),
        audit: TrainingAuditDto {
            total_records_found: bundle.audit_summary.total_records_found,
            included_records: bundle.audit_summary.included_records,
            excluded_no_consent: bundle.audit_summary.excluded_records_no_consent,
            excluded_revoked: bundle.audit_summary.excluded_records_revoked,
        },
    })
    .into_response()
}

// ─── Espacio Runtime Endpoints ──────────────────────────────────────────────

/// Sub-router for Espacio runtime space operations (/api/v1/espacio/*)
pub fn espacio_routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route(
            "/spaces",
            post(espacio_create_handler).get(espacio_list_handler),
        )
        .route(
            "/spaces/{id}",
            get(espacio_get_handler).delete(espacio_delete_handler),
        )
        .route(
            "/spaces/{id}/isolation/{other}",
            get(espacio_isolation_handler),
        )
}

/// POST /api/v1/espacio/spaces - create a new space
pub async fn espacio_create_handler(
    extension: Option<axum::extract::Extension<Arc<crate::espacio::SpaceManager>>>,
    Json(payload): Json<crate::espacio::CreateSpaceRequest>,
) -> impl axum::response::IntoResponse {
    use axum::http::StatusCode;
    let manager = extension.map(|e| e.0).unwrap_or_else(get_space_manager);
    match manager
        .create(
            payload.id,
            payload.name,
            payload.description,
            payload.owner_node,
            payload.is_public,
        )
        .await
    {
        Ok(info) => (
            StatusCode::CREATED,
            Json(serde_json::to_value(info).unwrap_or_default()),
        )
            .into_response(),
        Err(err) => {
            if let Some(space_err) = err.downcast_ref::<crate::espacio::SpaceError>() {
                match space_err {
                    crate::espacio::SpaceError::AlreadyExists(_) => (
                        StatusCode::CONFLICT,
                        crate::adapters::inbound::http::handlers::error_json(space_err),
                    )
                        .into_response(),
                    crate::espacio::SpaceError::InvalidId(_) => (
                        StatusCode::BAD_REQUEST,
                        crate::adapters::inbound::http::handlers::error_json(space_err),
                    )
                        .into_response(),
                    crate::espacio::SpaceError::NotFound(_) => (
                        StatusCode::NOT_FOUND,
                        crate::adapters::inbound::http::handlers::error_json(space_err),
                    )
                        .into_response(),
                    crate::espacio::SpaceError::Storage(_) => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        crate::adapters::inbound::http::handlers::error_json(space_err),
                    )
                        .into_response(),
                }
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    crate::adapters::inbound::http::handlers::error_json(err),
                )
                    .into_response()
            }
        }
    }
}

/// GET /api/v1/espacio/spaces - list all spaces
pub async fn espacio_list_handler(
    extension: Option<axum::extract::Extension<Arc<crate::espacio::SpaceManager>>>,
) -> impl axum::response::IntoResponse {
    let manager = extension.map(|e| e.0).unwrap_or_else(get_space_manager);
    let spaces = manager.list().await;
    Json(spaces)
}

/// GET /api/v1/espacio/spaces/{id} - get space details by ID
pub async fn espacio_get_handler(
    extension: Option<axum::extract::Extension<Arc<crate::espacio::SpaceManager>>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl axum::response::IntoResponse {
    use axum::http::StatusCode;
    let manager = extension.map(|e| e.0).unwrap_or_else(get_space_manager);
    match manager.get(&id).await {
        Ok(info) => Json(serde_json::to_value(info).unwrap_or_default()).into_response(),
        Err(err) => {
            if let Some(space_err) = err.downcast_ref::<crate::espacio::SpaceError>() {
                match space_err {
                    crate::espacio::SpaceError::NotFound(_) => (
                        StatusCode::NOT_FOUND,
                        crate::adapters::inbound::http::handlers::error_json(space_err),
                    )
                        .into_response(),
                    _ => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        crate::adapters::inbound::http::handlers::error_json(space_err),
                    )
                        .into_response(),
                }
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    crate::adapters::inbound::http::handlers::error_json(err),
                )
                    .into_response()
            }
        }
    }
}

/// DELETE /api/v1/espacio/spaces/{id} - delete space by ID
pub async fn espacio_delete_handler(
    extension: Option<axum::extract::Extension<Arc<crate::espacio::SpaceManager>>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl axum::response::IntoResponse {
    use axum::http::StatusCode;
    let manager = extension.map(|e| e.0).unwrap_or_else(get_space_manager);
    match manager.delete(&id).await {
        Ok(()) => Json(serde_json::json!({
            "status": "ok",
            "message": format!("Space {} deleted", id),
            "id": id,
        }))
        .into_response(),
        Err(err) => {
            if let Some(space_err) = err.downcast_ref::<crate::espacio::SpaceError>() {
                match space_err {
                    crate::espacio::SpaceError::NotFound(_) => (
                        StatusCode::NOT_FOUND,
                        crate::adapters::inbound::http::handlers::error_json(space_err),
                    )
                        .into_response(),
                    _ => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        crate::adapters::inbound::http::handlers::error_json(space_err),
                    )
                        .into_response(),
                }
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    crate::adapters::inbound::http::handlers::error_json(err),
                )
                    .into_response()
            }
        }
    }
}

/// GET /api/v1/espacio/spaces/{id}/isolation/{other} - check if two spaces are isolated
pub async fn espacio_isolation_handler(
    extension: Option<axum::extract::Extension<Arc<crate::espacio::SpaceManager>>>,
    axum::extract::Path((id, other)): axum::extract::Path<(String, String)>,
) -> impl axum::response::IntoResponse {
    let manager = extension.map(|e| e.0).unwrap_or_else(get_space_manager);
    let isolated = manager.are_isolated(&id, &other).await;
    Json(serde_json::json!({
        "space_a": id,
        "space_b": other,
        "isolated": isolated,
    }))
}

// ─── Content Redaction API ─────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RedactRequest {
    pub text: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RedactResponse {
    pub redacted_text: String,
}

/// POST /v1/memories/redact - Redact PII / sensitive data from a text payload.
pub async fn memories_redact_handler(
    Json(payload): Json<RedactRequest>,
) -> impl axum::response::IntoResponse {
    let engine = crate::security::redaction::RedactionEngine::default();
    let redacted = engine.redact(&payload.text);
    Json(RedactResponse {
        redacted_text: redacted,
    })
}

/// Get the plugin registry (panics if not initialized)
#[cfg(feature = "enterprise")]
pub fn get_plugin_registry(
) -> std::sync::Arc<tokio::sync::RwLock<crate::adapters::inbound::http::plugins::PluginRegistry>> {
    PLUGIN_REGISTRY
        .get()
        .expect("Plugin registry not initialized. Call init_plugin_registry() at startup.")
        .clone()
}

#[cfg(feature = "enterprise")]
/// Plugins health handler.
pub async fn plugins_health_handler() -> Json<PluginsHealthResponse> {
    #[allow(unused_imports)]
    use crate::adapters::inbound::http::plugins::Plugin;

    let registry = get_plugin_registry();
    let registry = registry.read().await;

    let mut plugins = Vec::new();

    for plugin in registry.plugins() {
        let name = plugin.name().to_string();
        let version = plugin.version().to_string();

        // Run health check
        let health_result = plugin.health_check().await;
        let (healthy, error) = match health_result {
            Ok(()) => (true, None),
            Err(e) => (false, Some(e)),
        };

        plugins.push(PluginHealthStatus {
            name,
            version,
            healthy,
            error,
        });
    }

    let status = if plugins.iter().all(|p| p.healthy) {
        "healthy"
    } else if plugins.iter().any(|p| p.healthy) {
        "degraded"
    } else {
        "unhealthy"
    };

    Json(PluginsHealthResponse {
        status: status.to_string(),
        plugins,
    })
}

#[cfg(feature = "enterprise")]
/// Plugins sync handler.
pub async fn plugins_sync_handler(
    Json(payload): Json<PluginSyncRequest>,
) -> Json<PluginSyncResponse> {
    #[allow(unused_imports)]
    use crate::adapters::inbound::http::plugins::{Plugin, SyncDirection};

    let direction = match payload.direction.to_lowercase().as_str() {
        "push" => SyncDirection::Push,
        "pull" => SyncDirection::Pull,
        "both" => SyncDirection::Both,
        _ => {
            return Json(PluginSyncResponse {
                status: "error".to_string(),
                results: vec![],
            });
        }
    };

    let registry = get_plugin_registry();
    let registry = registry.read().await;

    let mut results = Vec::new();
    let mut any_success = false;
    let mut any_failure = false;

    for plugin in registry.plugins() {
        let plugin_name = plugin.name().to_string();

        match plugin.sync(direction).await {
            Ok(sync_result) => {
                if sync_result.success {
                    any_success = true;
                } else {
                    any_failure = true;
                }

                results.push(PluginSyncResult {
                    plugin_name,
                    success: sync_result.success,
                    items_synced: sync_result.items_synced,
                    error: sync_result.error,
                });
            }
            Err(e) => {
                any_failure = true;
                results.push(PluginSyncResult {
                    plugin_name,
                    success: false,
                    items_synced: 0,
                    error: Some(e),
                });
            }
        }
    }

    let status = if any_failure && !any_success {
        "error"
    } else if any_failure {
        "partial"
    } else {
        "success"
    };

    Json(PluginSyncResponse {
        status: status.to_string(),
        results,
    })
}

// ─── Mini-Experts Endpoints ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct MiniExpertInvokeRequest {
    pub name: String,
    pub prompt: String,
}

#[derive(Debug, Serialize)]
pub struct MiniExpertInvokeResponse {
    pub status: String,
    pub provider: String,
    pub endpoint: String,
    pub response: String,
}

/// GET /v1/agents/mini-experts — list configured mini-experts.
pub async fn mini_experts_list_handler() -> impl axum::response::IntoResponse {
    let registry = crate::agents::mini_experts::MiniExpertRegistry::load_default();
    let reg_entries = registry.list();

    let settings = XavierSettings::current();
    let mut mini_experts = settings.workspace.mini_experts;
    if mini_experts.is_empty() {
        mini_experts = XavierSettings::default().workspace.mini_experts;
    }

    for entry in reg_entries {
        if !mini_experts.iter().any(|e| e.name == entry.name) {
            mini_experts.push(entry.to_config());
        }
    }

    Json(mini_experts)
}

/// POST /v1/agents/mini-experts/invoke — invoke a mini-expert by name.
pub async fn mini_expert_invoke_handler(
    Json(payload): Json<MiniExpertInvokeRequest>,
) -> Result<Json<MiniExpertInvokeResponse>, (axum::http::StatusCode, String)> {
    let registry = crate::agents::mini_experts::MiniExpertRegistry::load_default();
    let settings = XavierSettings::current();
    let mut mini_experts = settings.workspace.mini_experts;
    if mini_experts.is_empty() {
        mini_experts = XavierSettings::default().workspace.mini_experts;
    }

    let router = crate::agents::provider_router::ProviderRouter::from_registry_and_configs(
        &registry,
        mini_experts,
    );

    let expert_config = router.route(&payload.name).ok_or_else(|| {
        (
            axum::http::StatusCode::NOT_FOUND,
            format!("Mini-expert '{}' not found", payload.name),
        )
    })?;

    let provider = expert_config.provider.clone();
    let endpoint = expert_config.endpoint.clone();

    let response = router
        .invoke(&payload.name, &payload.prompt)
        .await
        .map_err(|e| match e {
            crate::agents::provider_router::MiniExpertInvokeError::NotFound(name) => (
                axum::http::StatusCode::NOT_FOUND,
                format!("Mini-expert '{}' not found", name),
            ),
            crate::agents::provider_router::MiniExpertInvokeError::ModelNotInstalled { model } => (
                axum::http::StatusCode::NOT_FOUND,
                format!(
                    "Model '{}' not found in local provider. Please run: ollama pull {}",
                    model, model
                ),
            ),
            crate::agents::provider_router::MiniExpertInvokeError::ProviderError {
                name,
                status,
                details,
            } => (
                axum::http::StatusCode::BAD_GATEWAY,
                format!(
                    "Failed to invoke mini-expert '{}' (HTTP {}): {}",
                    name, status, details
                ),
            ),
            crate::agents::provider_router::MiniExpertInvokeError::NetworkError(err) => (
                axum::http::StatusCode::BAD_GATEWAY,
                format!("Network error invoking mini-expert: {}", err),
            ),
        })?;

    Ok(Json(MiniExpertInvokeResponse {
        status: "success".to_string(),
        provider,
        endpoint,
        response,
    }))
}

#[cfg(test)]
mod route_tests {
    use axum::{
        body::Body,
        http::{Method, Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use tower::util::ServiceExt;

    use super::{create_router, create_router_with_agent_registry};
    use crate::coordination::SimpleAgentRegistry;

    fn post_request(uri: &str) -> Request<Body> {
        Request::builder()
            .uri(uri)
            .method(Method::POST)
            .body(Body::empty())
            .expect("build POST request")
    }

    #[tokio::test]
    async fn test_route_mini_expert_invoke_missing_expert_404() {
        use axum::response::Response;
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let req_body = serde_json::json!({
            "name": "nonexistent-expert",
            "prompt": "hello"
        });
        let response: Response = create_router()
            .oneshot(
                Request::builder()
                    .uri("/v1/agents/mini-experts/invoke")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                    .unwrap(),
            )
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let err_msg = String::from_utf8_lossy(&body);
        assert!(err_msg.contains("not found"));
    }

    #[tokio::test]
    async fn test_route_mini_expert_invoke_missing_model_404() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/v1/chat/completions")
            .with_status(404)
            .with_body(r#"{"error":"model 'qwen3-4b' not found"}"#)
            .create_async()
            .await;

        let temp_dir =
            std::env::temp_dir().join(format!("xavier_test_route_mexp_{}", ulid::Ulid::new()));
        let db_file = temp_dir.join("mini_experts.json");
        let registry = crate::agents::mini_experts::MiniExpertRegistry::new(&db_file);
        registry
            .register(crate::agents::mini_experts::MiniExpertEntry {
                name: "test-qwen3-4b".to_string(),
                segment: "codebase/test".to_string(),
                language: "es".to_string(),
                clearance: 1,
                source_dataset: "ds1".to_string(),
                model_gguf_path: "/models/qwen3.gguf".to_string(),
                provider: "local".to_string(),
                endpoint: format!("{}/v1", server.url()),
                api_key: None,
            })
            .unwrap();

        let router = crate::agents::provider_router::ProviderRouter::from_registry_and_configs(
            &registry,
            vec![],
        );
        let err = router
            .invoke("test-qwen3-4b", "write rust code")
            .await
            .unwrap_err();
        match err {
            crate::agents::provider_router::MiniExpertInvokeError::ModelNotInstalled { model } => {
                assert_eq!(model, "test-qwen3-4b");
            }
            other => panic!("expected ModelNotInstalled, got {:?}", other),
        }

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn unregister_route_removes_existing_agent() {
        let registry = SimpleAgentRegistry::new(None);
        registry
            .register(
                "agent-delete-1".to_string(),
                "session-delete-1".to_string(),
                Default::default(),
            )
            .await;

        let response = create_router_with_agent_registry(registry.clone())
            .oneshot(post_request("/xavier/agents/agent-delete-1/unregister"))
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let parsed: serde_json::Value =
            serde_json::from_slice(&body).expect("parse unregister response");

        assert_eq!(parsed["status"], "ok");
        assert_eq!(parsed["agent_id"], "agent-delete-1");
        assert_eq!(parsed["message"], "Agent unregistered");
        assert!(registry.get("agent-delete-1").await.is_none());
    }

    #[tokio::test]
    async fn unregister_route_returns_error_for_missing_agent() {
        let response = create_router_with_agent_registry(SimpleAgentRegistry::new(None))
            .oneshot(post_request("/xavier/agents/missing-agent/unregister"))
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let parsed: serde_json::Value =
            serde_json::from_slice(&body).expect("parse unregister response");

        assert_eq!(parsed["status"], "error");
        assert_eq!(parsed["agent_id"], "missing-agent");
        assert_eq!(parsed["message"], "Agent not found or already unregistered");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn health_route_returns_json_ok() {
        use axum::response::Response;
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let response: Response = create_router()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let parsed: serde_json::Value =
            serde_json::from_slice(&body).expect("parse health response");

        // Status can be "healthy", "warn", "degraded", or "unhealthy" in test environments
        let status = parsed["status"].as_str().unwrap_or("");
        assert!(
            status == "healthy"
                || status == "warn"
                || status == "degraded"
                || status == "unhealthy",
            "expected status to be one of healthy/warn/degraded/unhealthy, got: {}",
            status
        );
        assert_eq!(parsed["service"], "xavier");
        assert!(parsed.get("version").is_some());
    }

    #[tokio::test]
    async fn readiness_route_returns_json_ok() {
        use axum::response::Response;
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let response: Response = create_router()
            .oneshot(
                Request::builder()
                    .uri("/readiness")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let parsed: serde_json::Value =
            serde_json::from_slice(&body).expect("parse readiness response");

        assert_eq!(parsed["status"], "ok");
        assert_eq!(parsed["service"], "xavier");
    }

    #[tokio::test]
    async fn build_route_returns_json_info() {
        use axum::response::Response;
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let response: Response = create_router()
            .oneshot(
                Request::builder()
                    .uri("/build")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let parsed: serde_json::Value =
            serde_json::from_slice(&body).expect("parse build response");

        assert_eq!(parsed["service"], "xavier");
        assert!(parsed.get("version").is_some());
    }

    #[tokio::test]
    async fn test_route_training_datasets_list() {
        use axum::response::Response;
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let response: Response = create_router()
            .oneshot(
                Request::builder()
                    .uri("/v1/training/datasets")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let parsed: serde_json::Value = serde_json::from_slice(&body).expect("parse list response");
        assert!(parsed.is_array());
    }

    #[tokio::test]
    async fn test_route_training_dataset_manifest_not_found() {
        use axum::response::Response;
        use tower::ServiceExt;
        let response: Response = create_router()
            .oneshot(
                Request::builder()
                    .uri("/v1/training/datasets/nonexistent_dataset_id")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_route_mini_experts_list() {
        use axum::response::Response;
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let response: Response = create_router()
            .oneshot(
                Request::builder()
                    .uri("/v1/agents/mini-experts")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let parsed: serde_json::Value = serde_json::from_slice(&body).expect("parse list response");

        assert!(parsed.is_array());
        let arr = parsed.as_array().unwrap();
        assert!(!arr.is_empty());
        assert_eq!(arr[0]["provider"], "agy");
    }

    #[tokio::test]
    async fn test_route_mini_expert_invoke_mock() {
        use axum::response::Response;
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let req_body = serde_json::json!({
            "name": "agy-expert",
            "prompt": "test prompt"
        });
        let response: Response = create_router()
            .oneshot(
                Request::builder()
                    .uri("/v1/agents/mini-experts/invoke")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                    .unwrap(),
            )
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let parsed: serde_json::Value =
            serde_json::from_slice(&body).expect("parse invoke response");

        assert_eq!(parsed["status"], "success");
        assert_eq!(parsed["provider"], "agy");
        assert!(parsed["response"]
            .as_str()
            .unwrap()
            .contains("Mock response"));
    }

    #[tokio::test]
    async fn test_route_maintenance_reindex_dry_run() {
        use axum::response::Response;
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let req_body = serde_json::json!({
            "dry_run": true,
            "limit": 5
        });
        let mut req = Request::builder()
            .uri("/v1/maintenance/reindex-embeddings")
            .method(Method::POST)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
            .unwrap();
        req.extensions_mut()
            .insert(crate::security::auth::Claims::new(
                "admin".to_string(),
                "admin@example.com".to_string(),
                crate::security::auth::UserRole::Admin,
                chrono::Duration::hours(1),
            ));
        let response: Response = create_router()
            .oneshot(req)
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let parsed: serde_json::Value =
            serde_json::from_slice(&body).expect("parse reindex response");

        assert_eq!(parsed["status"], "ok");
        assert_eq!(parsed["dry_run"], true);
        assert!(parsed.get("null_embeddings_count").is_some());
        assert_eq!(parsed["processed_count"], 0);
    }

    #[tokio::test]
    async fn test_route_maintenance_reindex_null_limit_applies_default_500() {
        use axum::response::Response;
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let req_body = serde_json::json!({
            "dry_run": true,
            "limit": null
        });
        let mut req = Request::builder()
            .uri("/v1/maintenance/reindex-embeddings")
            .method(Method::POST)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
            .unwrap();
        req.extensions_mut()
            .insert(crate::security::auth::Claims::new(
                "admin".to_string(),
                "admin@example.com".to_string(),
                crate::security::auth::UserRole::Admin,
                chrono::Duration::hours(1),
            ));
        let response: Response = create_router()
            .oneshot(req)
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let parsed: serde_json::Value =
            serde_json::from_slice(&body).expect("parse reindex response");

        assert_eq!(parsed["limit_applied"], 500);
    }

    #[tokio::test]
    async fn test_route_maintenance_reindex_already_running_guard() {
        use crate::memory::sqlite_vec_store::schema_impl::REINDEX_RUNNING;
        use axum::response::Response;
        use http_body_util::BodyExt;
        use std::sync::atomic::Ordering;
        use tower::ServiceExt;

        REINDEX_RUNNING.store(true, Ordering::SeqCst);

        let req_body = serde_json::json!({
            "dry_run": false,
            "limit": 10
        });
        let mut req = Request::builder()
            .uri("/v1/maintenance/reindex-embeddings")
            .method(Method::POST)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
            .unwrap();
        req.extensions_mut()
            .insert(crate::security::auth::Claims::new(
                "admin".to_string(),
                "admin@example.com".to_string(),
                crate::security::auth::UserRole::Admin,
                chrono::Duration::hours(1),
            ));
        let response: Response = create_router()
            .oneshot(req)
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let parsed: serde_json::Value =
            serde_json::from_slice(&body).expect("parse already running response");

        assert_eq!(parsed["status"], "already_running");

        REINDEX_RUNNING.store(false, Ordering::SeqCst);
    }

    #[tokio::test]
    async fn test_route_maintenance_reindex_status_get() {
        use axum::response::Response;
        use http_body_util::BodyExt;
        use tower::ServiceExt;

        let mut req = Request::builder()
            .uri("/v1/maintenance/reindex-embeddings")
            .method(Method::GET)
            .body(Body::empty())
            .unwrap();
        req.extensions_mut()
            .insert(crate::security::auth::Claims::new(
                "admin".to_string(),
                "admin@example.com".to_string(),
                crate::security::auth::UserRole::Admin,
                chrono::Duration::hours(1),
            ));
        let response: Response = create_router()
            .oneshot(req)
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let parsed: serde_json::Value =
            serde_json::from_slice(&body).expect("parse status response");

        assert_eq!(parsed["status"], "ok");
        assert!(parsed.get("is_running").is_some());
        assert!(parsed.get("processed_count").is_some());
    }

    #[tokio::test]
    async fn test_route_espacio_create_get_list() {
        use axum::response::Response;
        use axum::Extension;
        use http_body_util::BodyExt;
        use std::sync::Arc;
        use tower::ServiceExt;

        let temp_dir =
            std::env::temp_dir().join(format!("xavier_spaces_test1_{}", ulid::Ulid::new()));
        let mgr = Arc::new(crate::espacio::SpaceManager::new(&temp_dir));

        let req_body = serde_json::json!({
            "id": "esp_rt_1",
            "name": "Runtime Space 1",
            "description": "Test Desc 1",
            "owner_node": "node_1",
            "is_public": true
        });

        let mut req = Request::builder()
            .uri("/api/v1/espacio/spaces")
            .method(Method::POST)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
            .unwrap();
        req.extensions_mut().insert(mgr.clone());

        let response: Response = create_router().oneshot(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(created["id"], "esp_rt_1");
        assert_eq!(created["name"], "Runtime Space 1");

        // GET single space
        let mut req_get = Request::builder()
            .uri("/api/v1/espacio/spaces/esp_rt_1")
            .method(Method::GET)
            .body(Body::empty())
            .unwrap();
        req_get.extensions_mut().insert(mgr.clone());

        let response: Response = create_router().oneshot(req_get).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let fetched: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(fetched["id"], "esp_rt_1");

        // GET list spaces
        let mut req_list = Request::builder()
            .uri("/api/v1/espacio/spaces")
            .method(Method::GET)
            .body(Body::empty())
            .unwrap();
        req_list.extensions_mut().insert(mgr.clone());

        let response: Response = create_router().oneshot(req_list).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let list: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(list.iter().any(|s| s["id"] == "esp_rt_1"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_route_espacio_isolation_delete_not_found() {
        use axum::response::Response;
        use http_body_util::BodyExt;
        use std::sync::Arc;
        use tower::ServiceExt;

        let temp_dir =
            std::env::temp_dir().join(format!("xavier_spaces_test2_{}", ulid::Ulid::new()));
        let mgr = Arc::new(crate::espacio::SpaceManager::new(&temp_dir));

        // Create space A
        let req_a_body = serde_json::json!({
            "id": "esp_rt_2a",
            "name": "Space 2A",
            "description": "Desc 2A",
            "owner_node": "node_1",
            "is_public": false
        });
        let mut req_a = Request::builder()
            .uri("/api/v1/espacio/spaces")
            .method(Method::POST)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&req_a_body).unwrap()))
            .unwrap();
        req_a.extensions_mut().insert(mgr.clone());
        let res_a: Response = create_router().oneshot(req_a).await.unwrap();
        assert_eq!(res_a.status(), StatusCode::CREATED);

        // Create space B
        let req_b_body = serde_json::json!({
            "id": "esp_rt_2b",
            "name": "Space 2B",
            "description": "Desc 2B",
            "owner_node": "node_2",
            "is_public": false
        });
        let mut req_b = Request::builder()
            .uri("/api/v1/espacio/spaces")
            .method(Method::POST)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&req_b_body).unwrap()))
            .unwrap();
        req_b.extensions_mut().insert(mgr.clone());
        let res_b: Response = create_router().oneshot(req_b).await.unwrap();
        assert_eq!(res_b.status(), StatusCode::CREATED);

        // GET isolation check
        let mut req_iso = Request::builder()
            .uri("/api/v1/espacio/spaces/esp_rt_2a/isolation/esp_rt_2b")
            .method(Method::GET)
            .body(Body::empty())
            .unwrap();
        req_iso.extensions_mut().insert(mgr.clone());
        let res_iso: Response = create_router().oneshot(req_iso).await.unwrap();
        assert_eq!(res_iso.status(), StatusCode::OK);
        let body_iso = res_iso.into_body().collect().await.unwrap().to_bytes();
        let parsed_iso: serde_json::Value = serde_json::from_slice(&body_iso).unwrap();
        assert_eq!(parsed_iso["isolated"], true);

        // DELETE space 2a
        let mut req_del = Request::builder()
            .uri("/api/v1/espacio/spaces/esp_rt_2a")
            .method(Method::DELETE)
            .body(Body::empty())
            .unwrap();
        req_del.extensions_mut().insert(mgr.clone());
        let res_del: Response = create_router().oneshot(req_del).await.unwrap();
        assert_eq!(res_del.status(), StatusCode::OK);

        // GET deleted space 2a -> 404
        let mut req_get_del = Request::builder()
            .uri("/api/v1/espacio/spaces/esp_rt_2a")
            .method(Method::GET)
            .body(Body::empty())
            .unwrap();
        req_get_del.extensions_mut().insert(mgr.clone());
        let res_get_del: Response = create_router().oneshot(req_get_del).await.unwrap();
        assert_eq!(res_get_del.status(), StatusCode::NOT_FOUND);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_route_espacio_create_invalid_and_duplicate() {
        use axum::response::Response;
        use std::sync::Arc;
        use tower::ServiceExt;

        let temp_dir =
            std::env::temp_dir().join(format!("xavier_spaces_test3_{}", ulid::Ulid::new()));
        let mgr = Arc::new(crate::espacio::SpaceManager::new(&temp_dir));

        // Create initial space
        let req_valid_body = serde_json::json!({
            "id": "esp_rt_dup",
            "name": "Dup Space",
            "description": "Desc",
            "owner_node": "node_1",
            "is_public": false
        });
        let mut req_valid = Request::builder()
            .uri("/api/v1/espacio/spaces")
            .method(Method::POST)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&req_valid_body).unwrap()))
            .unwrap();
        req_valid.extensions_mut().insert(mgr.clone());
        let res_valid: Response = create_router().oneshot(req_valid).await.unwrap();
        assert_eq!(res_valid.status(), StatusCode::CREATED);

        // Duplicate space -> 409 CONFLICT
        let mut req_dup = Request::builder()
            .uri("/api/v1/espacio/spaces")
            .method(Method::POST)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&req_valid_body).unwrap()))
            .unwrap();
        req_dup.extensions_mut().insert(mgr.clone());
        let res_dup: Response = create_router().oneshot(req_dup).await.unwrap();
        assert_eq!(res_dup.status(), StatusCode::CONFLICT);

        // Invalid space ID -> 400 BAD REQUEST
        let req_invalid_body = serde_json::json!({
            "id": "invalid/space/id",
            "name": "Bad Space",
            "description": "Desc",
            "owner_node": "node_1",
            "is_public": false
        });
        let mut req_invalid = Request::builder()
            .uri("/api/v1/espacio/spaces")
            .method(Method::POST)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&req_invalid_body).unwrap()))
            .unwrap();
        req_invalid.extensions_mut().insert(mgr.clone());
        let res_invalid: Response = create_router().oneshot(req_invalid).await.unwrap();
        assert_eq!(res_invalid.status(), StatusCode::BAD_REQUEST);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_route_founder_status_ok() {
        use axum::response::Response;
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let response: Response = create_router()
            .oneshot(
                Request::builder()
                    .uri("/node/founder/status")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let parsed: serde_json::Value =
            serde_json::from_slice(&body).expect("parse status response");

        assert_eq!(parsed["status"], "active");
        assert_eq!(parsed["is_valid"], true);
        assert!(parsed["attestation"]["signature_hex"].is_string());
        assert_eq!(parsed["attestation"]["version"], 1);
        assert_eq!(parsed["genesis_params"]["network_id"], "swal-mainnet-v1");
    }

    #[tokio::test]
    async fn test_rate_limit_middleware_exceeded_returns_429() {
        use crate::middleware::token_bucket::IpRateLimiter;
        use std::time::Duration;

        let test_limiter = IpRateLimiter::new(2.0, 0.1);
        let test_ip = "192.168.1.100";

        let (allowed1, _) = test_limiter.try_consume(test_ip, 1.0);
        assert!(allowed1);

        let (allowed2, _) = test_limiter.try_consume(test_ip, 1.0);
        assert!(allowed2);

        let (allowed3, retry_after) = test_limiter.try_consume(test_ip, 1.0);
        assert!(!allowed3);
        assert!(retry_after > Duration::ZERO);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::session_sync_task::{SyncCheckResult, LAST_CHECK_RESULT};

    #[tokio::test]
    async fn sync_check_handler_uses_cached_sync_result() {
        let test_result = SyncCheckResult {
            status: "alert".to_string(),
            lag_ms: 42_000,
            save_ok_rate: 0.90,
            match_score: 0.88,
            active_agents: 7,
            timestamp_ms: 1_234_567,
            alerts: vec![
                "Index lag 42000ms exceeds threshold 30000ms".to_string(),
                "Save ok rate 90.0% below threshold 95.0%".to_string(),
            ],
        };
        *LAST_CHECK_RESULT.write().expect("test assertion") = test_result;

        let Json(response) = sync_check_handler().await;

        assert_eq!(response.status, "alert");
        assert_eq!(response.lag_ms, 42_000);
        assert_eq!(response.save_ok_rate, 0.90);
        assert_eq!(response.match_score, 0.88);
        assert_eq!(response.active_agents, 7);
        assert_eq!(response.timestamp_ms, 1_234_567);
        assert_eq!(response.alerts.len(), 2);
    }
}
