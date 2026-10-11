//! HTTP server and WebSocket handlers

use anyhow::{anyhow, Result};
use axum::{
    extract::DefaultBodyLimit,
    handler::Handler,
    middleware::{self},
    routing::{delete, get, post, put},
    Router,
};
use axum_server::tls_rustls::RustlsConfig;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
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
use xavier::middleware::require_permission;
use xavier::security::auth::{Permission, UserRole};
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
    get_graph, list_bookmarks, list_widgets, save_bookmark, save_graph, save_widget,
};
#[cfg(feature = "panel-ui")]
use xavier::server::panel::{panel_asset, panel_index};
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
    autometrics::prometheus_exporter::encode_http_response().into_response()
}

/// Handler to list lab segments with clearance levels (WAVE-22.02)
pub async fn list_segments_handler() -> axum::response::Response {
    use axum::response::IntoResponse;
    let segments: Vec<serde_json::Value> = xavier::security::groups::LAB_SEGMENTS
        .iter()
        .map(|(id, name, level)| {
            serde_json::json!({
                "id": id,
                "name": name,
                "clearance": level.as_str(),
            })
        })
        .collect();
    (
        axum::http::StatusCode::OK,
        axum::Json(serde_json::json!({ "segments": segments })),
    )
        .into_response()
}

/// Handler to list documents under a segment space (WAVE-22.02)
pub async fn get_segment_documents_handler(
    axum::extract::State(state): axum::extract::State<CliState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let space = xavier::memory::segments::SegmentSpace::new(state.memory.as_ref());
    match space.list(&id, 100).await {
        Ok(docs) => (
            axum::http::StatusCode::OK,
            axum::Json(serde_json::json!({
                "segment_id": id,
                "documents": docs,
            })),
        )
            .into_response(),
        Err(_) => (
            axum::http::StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({
                "error": format!("Segment '{}' not found", id)
            })),
        )
            .into_response(),
    }
}

/// Memory and MCP routes of the node, shared with the isolation tests so the
/// test composition cannot drift from production. Layers (auth, clearance,
/// rate limit, body limit) are applied by the caller.
pub fn memory_routes() -> Router<CliState> {
    Router::new()
        .route("/memory/search", post(search_handler))
        .route(
            "/memory/get",
            get(crate::cli::handlers::memory::get_handler),
        )
        .route(
            "/memory/update",
            post(update_handler).layer(middleware::from_fn(require_permission(|r| {
                r.can_add_memory()
            }))),
        )
        .route(
            "/memory/delete",
            post(delete_handler).layer(middleware::from_fn(require_permission(|r| {
                r.can_delete_memory()
            }))),
        )
        .route(
            "/memory/reindex",
            post(reindex_handler).layer(middleware::from_fn(require_permission(|r| {
                r.can_add_memory()
            }))),
        )
        .route(
            "/v1/maintenance/reindex-embeddings",
            post(xavier::adapters::inbound::http::routes::maintenance_reindex_handler).layer(
                middleware::from_fn(xavier::middleware::require_permission(|r| {
                    r.can_edit_config()
                })),
            ),
        )
        .route("/memory/stats", get(stats_handler))
        .route("/v1/stats", get(stats_handler))
        .route("/memory/export", get(export_handler))
        .route("/memory/export-markdown", get(export_markdown_handler))
        .route("/v1/memory/export-markdown", get(export_markdown_handler))
        .route(
            "/memory/decay",
            post(decay_handler).layer(middleware::from_fn(require_permission(|r| {
                r.can_delete_memory()
            }))),
        )
        .route(
            "/memory/consolidate",
            post(consolidate_handler).layer(middleware::from_fn(require_permission(|r| {
                r.can_delete_memory()
            }))),
        )
        .route(
            "/memory/prune",
            post(memory_prune_handler).layer(middleware::from_fn(require_permission(|r| {
                r.can_delete_memory()
            }))),
        )
        .route("/memory/index-self", post(memory_index_self_handler))
        .route(
            "/memory/evict",
            axum::routing::delete(evict_handler).layer(middleware::from_fn(require_permission(
                |r| r.can_delete_memory(),
            ))),
        )
        .route("/memory/manage", post(manage_handler))
        .route("/memory/timeline/query", post(timeline_query_handler))
        .route("/v1/segments", get(list_segments_handler))
        .route(
            "/v1/segments/{id}/documents",
            get(get_segment_documents_handler),
        )
        .route(
            "/v1/memories",
            post(
                add_handler.layer(middleware::from_fn(require_permission(|r| {
                    r.can_add_memory()
                }))),
            )
            .get(stats_handler),
        )
        // Sync write path: it persists client-supplied records, so it carries
        // the same write permission gate as the single-record add route.
        .route(
            "/v1/memory/push",
            post(crate::cli::handlers::memory::memory_push_handler).layer(middleware::from_fn(
                require_permission(|r| r.can_add_memory()),
            )),
        )
        .route(
            "/v1/memories/search",
            post(xavier::server::v1_api::v1_memories_search),
        )
        .route(
            "/v1/memories/prune",
            post(xavier::server::v1_api::v1_memories_prune).layer(middleware::from_fn(
                require_permission(|r| r.can_delete_memory()),
            )),
        )
        .route(
            "/v1/memories/consolidate",
            post(xavier::server::v1_api::v1_memories_consolidate),
        )
        .route(
            "/v1/events",
            post(xavier::server::v1_api::v1_events_add).get(xavier::server::v1_api::v1_events_list),
        )
        .route(
            "/v1/context/assemble",
            post(xavier::server::v1_api::v1_context_assemble),
        )
        .route(
            "/v1/context/package",
            post(xavier::server::v1_api::v1_context_package),
        )
        .route(
            "/v1/memory/recall-eval",
            post(xavier::server::v1_api::v1_memory_recall_eval),
        )
        .route(
            "/v1/memory/recall/stats",
            get(xavier::server::v1_api::v1_memory_recall_stats),
        )
        .route(
            "/v1/memories/{id}",
            get(xavier::server::v1_api::v1_memories_get),
        )
        .route(
            "/v1/memories/{id}/outline",
            get(xavier::server::v1_api::v1_memories_outline),
        )
        .route(
            "/v1/memories/graph",
            get(xavier::server::v1_api::v1_memories_graph),
        )
        .route(
            "/v1/graph/export",
            get(xavier::server::v1_api::v1_graph_export),
        )
        .route("/mcp/tools", get(mcp_tools_handler))
        .route("/mcp/tools/call", post(mcp_tools_call_handler))
}

/// Memory routes that accept large bodies (the caller adds the body limit).
pub fn memory_large_body_routes() -> Router<CliState> {
    Router::new()
        .route(
            "/memory/add",
            post(add_handler).layer(middleware::from_fn(require_permission(|r| {
                r.can_add_memory()
            }))),
        )
        .route("/memory/export-pack", post(export_pack_handler))
}

/// Refuse to serve HTTP without a configured token.
fn ensure_http_token_configured(token: &str) -> Result<()> {
    if token.trim().is_empty() {
        return Err(anyhow!(
            "XAVIER_TOKEN is not configured; refusing to start the HTTP server without a token"
        ));
    }
    Ok(())
}

/// Importers owned by one process of the periodic ingestion loop.
///
/// Built once, outside the cycle loop. Each cursor file is
/// `{data_dir}/ingest-cursors/<source>.json`, so a later process skips sources
/// that have not changed. Without those files a new process full-scans.
struct SessionImporters {
    antigravity: xavier::memory::antigravity_importer::AntigravityImporter,
    opencode: xavier::memory::opencode_importer::OpenCodeImporter,
    hermes: xavier::memory::hermes_importer::HermesImporter,
    codex: xavier::memory::codex_importer::CodexImporter,
}

fn build_session_importers(
    data_dir: &std::path::Path,
    embedder: Arc<dyn xavier::embedding::Embedder>,
) -> SessionImporters {
    let cursors = data_dir.join("ingest-cursors");
    SessionImporters {
        antigravity: xavier::memory::antigravity_importer::AntigravityImporter::new()
            .with_embedder(embedder.clone())
            .with_cursor_path(cursors.join("antigravity.json")),
        opencode: xavier::memory::opencode_importer::OpenCodeImporter::new()
            .with_embedder(embedder.clone())
            .with_cursor_path(cursors.join("opencode.json")),
        hermes: xavier::memory::hermes_importer::HermesImporter::new()
            .with_embedder(embedder.clone())
            .with_cursor_path(cursors.join("hermes.json")),
        codex: xavier::memory::codex_importer::CodexImporter::with_embedder(embedder)
            .with_cursor_path(cursors.join("codex.json")),
    }
}

/// Start http server.
pub async fn start_http_server(
    port: u16,
    mcp_port: Option<u16>,
    #[cfg_attr(not(feature = "panel-ui"), allow(unused_variables))] no_ui: bool,
) -> Result<()> {
    // Initialize Prometheus exporter
    let _ = autometrics::prometheus_exporter::try_init();

    // Initial health check run to populate the static HEALTH instance
    tokio::spawn(async {
        let _ = xavier::observability::health::HEALTH.run_checks().await;
    });
    Arc::clone(&*xavier::observability::health::HEALTH).spawn();

    let settings = XavierSettings::current();
    settings.apply_to_env();

    // Validate opencode CLI if active
    if settings
        .models
        .provider
        .trim()
        .eq_ignore_ascii_case("opencode")
    {
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
    ensure_http_token_configured(&token)?;
    std::env::set_var("XAVIER_TOKEN", &token);

    let cm = ConnectionManager::global();

    // Register the sqlite-vec extension (vec_f32, vec_distance_cosine, ...) via
    // `sqlite3_auto_extension` *before* any pool below opens its first
    // connection. `sqlite3_auto_extension` only affects connections opened
    // *after* this call — it does not retroactively patch a connection that
    // was already established. `cm.connect("memory"/"metrics"/"security", ...)`
    // just below used to run first, so those pools' connections could
    // permanently lack `vec_f32` for the lifetime of the process (surfaced as
    // `no such function: vec_f32` on memory writes through those pools; see
    // issue #2544). Registration is idempotent, so calling it again later
    // (e.g. inside `VecSqliteMemoryStore::new`) is harmless.
    xavier::memory::sqlite_vec_store::VecSqliteMemoryStore::register_sqlite_vec_extension()?;

    let config = VecSqliteStoreConfig::from_env();
    // ── Startup guard: detect store fragmentation ────────────────────────────
    // Warn if multiple vec-store*.sqlite3 files exist outside the canonical data/
    // directory. This prevents silent data split across locations.
    {
        let mut fragment_count: u32 = 0;
        let mut fragment_paths: Vec<String> = Vec::new();
        // Scan workspace root for vec-store*.sqlite3 outside data/
        if let Ok(entries) = std::fs::read_dir(".") {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name.starts_with("vec-store") && name.ends_with(".sqlite3") {
                        // Skip the canonical store
                        if let Ok(canonical) = std::fs::canonicalize(&p)
                            .and_then(|c| std::fs::canonicalize(&config.path).map(|k| (c, k)))
                        {
                            if canonical.0 == canonical.1 {
                                continue;
                            }
                        }
                        fragment_count += 1;
                        fragment_paths.push(p.display().to_string());
                    }
                }
            }
        }
        if fragment_count > 0 {
            tracing::warn!(
                fragment_count,
                canonical = %config.path.display(),
                ?fragment_paths,
                "Store fragmentation detected — multiple vec-store files found outside canonical data/ dir. \
                 Run scripts/consolidate-stores.py to merge."
            );
            xavier::server::alerts::SYSTEM_ALERTS.push_alert(
                "WARN",
                &format!(
                    "Store fragmentation: {} legacy vec-store files detected outside data/. \
                     Run scripts/consolidate-stores.py to merge.",
                    fragment_count
                ),
                "storage",
            );
        }
    }

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

    // Encrypted Xavier Cloud backup: runs only with PGHEART_URL/PGHEART_TOKEN AND a
    // backup passphrase; XAVIER_CLOUD_BACKUP_INTERVAL_MINS=0 disables it.
    if let Some(cloud_cfg) = xavier::sync::xavier_cloud::CloudBackupConfig::from_env() {
        match (
            xavier::sync::xavier_cloud::passphrase_from_env(),
            xavier::sync::xavier_cloud::interval_from_env(),
        ) {
            (Ok(Some(pass)), Some(interval)) => {
                tracing::info!(
                    instance = %cloud_cfg.instance_id,
                    interval_mins = interval.as_secs() / 60,
                    "cloud_backup: periodic encrypted backup enabled"
                );
                let backup_store: Arc<dyn xavier::memory::store::MemoryStore> = store.clone();
                tokio::spawn(xavier::sync::xavier_cloud::periodic_backup_loop(
                    backup_store,
                    cloud_cfg,
                    pass,
                    interval,
                ));
            }
            (Ok(None), _) => tracing::warn!(
                "cloud_backup: PGHEART_* is set but {} is not: periodic cloud backup disabled, nothing is uploaded",
                xavier::sync::xavier_cloud::PASSPHRASE_ENV
            ),
            (Err(e), _) => tracing::warn!("cloud_backup: disabled: {e:#}"),
            (Ok(Some(_)), None) => {
                tracing::info!("cloud_backup: periodic backup disabled by interval 0")
            }
        }
    }

    let time_store = Arc::new(TimeMetricsStore::new());
    let audit_logger = Arc::new(xavier::secrets::audit::QmdAuditLogger::new());
    let rate_manager = Arc::new(RateLimitManager::new());
    let threat_store = Arc::new(SecurityThreatStore::new());

    let state_dir_str = std::env::var("XAVIER_STATE_DIR")
        .or_else(|_| std::env::var("HOME"))
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    let state_dir = PathBuf::from(&state_dir_str);
    let xavier_dir = state_dir.join(".xavier");
    std::fs::create_dir_all(&xavier_dir)?;

    let auth_store_file_path = xavier_dir.join("auth_store.db");
    let auth_db_path = auth_store_file_path.to_string_lossy().to_string();
    let auth_store = Arc::new(AuthStore::open(&auth_db_path)?);

    let auth_db_file_path = xavier_dir.join("auth.db");

    let auth2_db = Arc::new(parking_lot::Mutex::new(xavier::auth2::db::AuthDb::new(
        &auth_db_file_path,
    )?));

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

    // LAZY LOADING: Skip loading all 30K+ documents into RAM at startup.
    // The pool is loaded on first access (search, get, ls, etc.) instead.
    // Env var XAVIER_MEMORY_EAGER_LOAD=1 restores the old behavior.
    let eager_load = std::env::var("XAVIER_MEMORY_EAGER_LOAD")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);

    let memory = if eager_load {
        tracing::info!("Memory pool: EAGER mode (XAVIER_MEMORY_EAGER_LOAD=1)");
        let durable_state = store.load_workspace_state(&workspace_id).await?;
        let docs = Arc::new(RwLock::new(
            durable_state
                .memories
                .iter()
                .map(MemoryRecord::to_document)
                .collect::<Vec<MemoryDocument>>(),
        ));
        Arc::new(QmdMemory::new_with_workspace(docs, workspace_id.clone()))
    } else {
        tracing::info!("Memory pool: LAZY mode (docs load on first access)");
        Arc::new(QmdMemory::new_lazy(workspace_id.clone()))
    };

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
    use xavier::domain::cycle_breaks::w30_02::TimeMetricSink;
    let health_adapter = Arc::new(HttpHealthAdapter::new(
        resolve_base_url_for_port(port),
        http_client.clone(),
    ));
    let time_adapter = Arc::new(TimeMetricsAdapter::new(
        Arc::clone(&time_store) as Arc<dyn TimeMetricSink>
    )) as Arc<dyn TimeMetricsPort>;
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

    // ── Keepalive del mesh ────────────────────────────────────────────────────
    // El health marca un peer como "degraded" si lleva mas de 60 s sin contacto
    // (observability/health.rs). Sin este latido, un emparejamiento recien verificado
    // pasaba a "degraded" al minuto aunque el peer estuviera vivo y accesible, que es
    // justo lo que no puede pasar en una demo. Se refresca last_seen_at de los peers
    // con endpoint conocido haciendo el handshake firmado normal.
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;

            let Ok(identity) = xavier::mesh::NodeIdentity::load_or_create() else {
                continue;
            };
            let Ok(mut registry) = xavier::mesh::PeerRegistry::load() else {
                continue;
            };
            let peers: Vec<xavier::mesh::PeerInfo> =
                registry.list_peers().into_iter().cloned().collect();
            if peers.is_empty() {
                continue;
            }

            let transport = xavier::mesh::MeshTransport::new(Arc::new(identity));
            let token = xavier::security::auth::resolve_xavier_token();
            let mut refreshed = 0usize;

            for peer in peers {
                // Sin endpoint no hay a quien saludar (los peers emparejados por handshake
                // entrante quedan asi hasta que se les registra el endpoint).
                if peer.endpoint_url.trim().is_empty() {
                    continue;
                }
                if transport
                    .handshake_with_secret(&peer.endpoint_url, &token, None)
                    .await
                    .is_ok()
                {
                    if let Some(entry) = registry.get_peer_mut(&peer.node_id) {
                        entry.last_seen_at = Some(chrono::Utc::now().timestamp());
                    }
                    refreshed += 1;
                }
            }

            if refreshed > 0 {
                // Persistir para que /health (que lee el disco) vea el latido.
                let _ = registry.save();
                tracing::debug!(refreshed, "mesh keepalive: last_seen_at actualizado");
            }
        }
    });

    let code_db_path = code_graph_db_path();
    if let Some(parent) = code_db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let code_db = Arc::new(::code_graph::db::CodeGraphDB::new(&code_db_path)?);
    // Skip symbol embeddings for bulk indexing when XAVIER_CODE_GRAPH_SKIP_EMBED=1
    // (embeddings per symbol at ~1.5/s CPU make bulk index of 1k+ files take hours;
    //  populate later via maintenance reindex or GPU-enabled embedding)
    let skip_symbol_embed = std::env::var("XAVIER_CODE_GRAPH_SKIP_EMBED")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);
    let mut code_indexer_obj = ::code_graph::indexer::Indexer::new(Arc::clone(&code_db));
    if !skip_symbol_embed {
        let symbol_embedder = Arc::new(crate::cli::handlers::code::XavierSymbolEmbedder::new(
            embedder.clone(),
        ));
        code_indexer_obj.set_embedder(symbol_embedder);
    } else {
        tracing::info!("code-graph: symbol embeddings SKIPPED (XAVIER_CODE_GRAPH_SKIP_EMBED=1) — bulk index mode");
    }
    let code_indexer = Arc::new(code_indexer_obj);
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

    // Consent-first Colby sidecar (usually non-TTY on boot → skip/honour env). Soft-fail.
    let sidecar =
        xavier::codebase::codegraph_sidecar::ensure_codegraph_sidecar_soft(&workspace_dir);
    info!("codegraph sidecar: {}", sidecar.message);

    let panel_root = state_panel_root(&workspace_dir, &workspace_id);
    let panel_store = Arc::new(ConversationsDb::open("default").await?);
    panel_store.create_schema().await?;
    panel_store.migrate_legacy_sessions(&panel_root).await?;

    let prompt_cache = Arc::new(parking_lot::Mutex::new(HashMap::new()));
    let security_service = Arc::new(AppSecurityService::new());
    let event_bus = XavierEventBus::new(100);
    // Durable mirror of the in-memory bus; a failure here must not stop boot.
    match xavier::coordination::event_log::EventLog::open(
        &xavier::maloca::MalocaStore::resolve_state_dir(),
        xavier::coordination::event_log::RetentionConfig::from_env(),
    ) {
        Ok(log) => {
            xavier::coordination::event_log::spawn_subscriber(&event_bus, Arc::clone(&log));
            xavier::coordination::event_log::install_global(log);
        }
        Err(e) => tracing::error!(error = %e, "event log unavailable; bus events are RAM-only"),
    }
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
            .filter_map(|c| {
                xavier::agents::provider::router::ProviderKind::from_str(&c.provider_label)
            })
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

    let multi_db = xavier::storage::multi_db::MultiDbManager::open_default().unwrap_or_else(|e| {
        tracing::warn!("multi_db registry unavailable ({e}); using RAM-only registry");
        xavier::storage::multi_db::MultiDbManager::new()
    });

    // Clone the bus for the WebSocket layer before it moves into CliState.
    let event_bus_for_ws = event_bus.clone();

    let state = CliState {
        memory: memory_port,
        qmd_memory: Arc::clone(&memory),
        session_manager: Arc::new(SessionManager::new(60)),
        store,
        workspace_id,
        workspace_dir,
        state_dir: state_dir.clone(),
        auth_db: Some(auth2_db),
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
        multi_db,
        maloca: xavier::maloca::MalocaStore::open_default(),
    };

    info!(
        "Memory store initialized for workspace: {}",
        state.workspace_id
    );

    // Espacio: one long-lived SpaceManager under the daemon data dir. Key
    // init failure or XAVIER_SPACES=off leaves espacio disabled, never a crash.
    let space_manager =
        crate::cli::state::open_space_manager(&xavier::maloca::MalocaStore::resolve_state_dir());
    if let Some(m) = &space_manager {
        xavier::adapters::inbound::http::routes::init_space_manager(m.clone());
    }

    // Initialize and wire up the Memory Sync singleton
    let node_id = if let Ok(identity) = xavier::mesh::NodeIdentity::load_or_create() {
        identity.node_id.0
    } else {
        "local".to_string()
    };
    // Shared mesh token: peers in the same SWAL mesh authenticate with the
    // same XAVIER_TOKEN (fallback: XAVIER_MESH_TOKEN, then none).
    let mesh_token = std::env::var("XAVIER_TOKEN")
        .ok()
        .or_else(|| std::env::var("XAVIER_MESH_TOKEN").ok());
    let sync_service = Arc::new(xavier::memory::sync::PeerMemorySync::with_peer_token(
        state.store.clone(),
        node_id,
        mesh_token,
    ));
    xavier::adapters::inbound::http::handlers::sync::init_memory_sync(Arc::clone(&sync_service));

    // ── Background sync loop ──────────────────────────────────────────────
    // Spawns a periodic background task that syncs memory with all configured
    // peers. Reads peer URLs from the XAVIER_PEERS env var (comma-separated)
    // or from an explicit list. If neither is configured, the loop idles.
    let (_sync_handle, _sync_stop) = sync_service.spawn_background_sync(vec![]);
    tracing::info!("Background memory sync loop spawned");

    // ── Universal Agent Session Ingestion Background Loop ────────────────
    // Periodically ingests transcripts from Antigravity, OpenCode, Hermes, and Codex.
    // Interval via XAVIER_INGESTION_INTERVAL_SECS (default 600); 0 disables
    // the loop (emergency brake for embedding storms).
    let ingestion_interval_secs = ingestion_interval_secs();
    let ingestion_store = state.store.clone();
    let ingestion_embedder = state.embedder.clone();
    let ingestion_in_flight = Arc::new(AtomicBool::new(false));
    if ingestion_interval_secs == 0 {
        tracing::warn!(
            "Universal agent session ingestion DISABLED (XAVIER_INGESTION_INTERVAL_SECS=0)"
        );
    } else {
        tracing::info!(
            interval_secs = ingestion_interval_secs,
            "Universal agent session ingestion loop spawned"
        );
        let in_flight = Arc::clone(&ingestion_in_flight);
        // The importers are built ONCE, here, and not inside the cycle loop.
        // Each one carries a cursor (fingerprint / watermark of what it already
        // ingested), loaded from and saved to `ingest-cursors/` under the daemon
        // data dir, and `sync` consults it to skip work. A constructor inside
        // the loop discards that state every cycle, which silently degrades
        // every `sync` back into a full scan — the exact regression this wiring
        // fixes. The structural tests in `ingestion_wiring_tests` guard the
        // position of these constructors. This stays inside the enabled branch:
        // `XAVIER_INGESTION_INTERVAL_SECS=0` does not build importers or cursor
        // files.
        let ingestion_data_dir = xavier::maloca::MalocaStore::resolve_state_dir();
        let SessionImporters {
            antigravity: ag_importer,
            opencode: oc_importer,
            hermes: hermes_importer,
            codex: codex_importer,
        } = build_session_importers(&ingestion_data_dir, ingestion_embedder);
        tokio::spawn(async move {
            // Initial grace period to allow server start
            tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;
            loop {
                if !try_begin_cycle(&in_flight) {
                    tracing::warn!("skipping ingestion cycle: previous still running");
                    tokio::time::sleep(tokio::time::Duration::from_secs(ingestion_interval_secs))
                        .await;
                    continue;
                }

                tracing::info!("🔄 Running universal agent session ingestion cycle...");

                // All three importers use `sync`, not `import_all`: the periodic
                // pass skips sources whose fingerprint is unchanged.
                // `import_all` is the explicit re-index path used by the /index
                // handlers, which report a count and so must read everything.
                //
                // Stats go to `info`, not `debug`: an idle node must be able to
                // show `read=0 skipped=N` in the journal or /health to prove the
                // cursor is working. At debug level the fix is unobservable in
                // production, which is how the previous full-scan regression ran
                // for days unnoticed.
                match ag_importer.sync(ingestion_store.as_ref()).await {
                    Ok(stats) => tracing::info!(
                        source = "antigravity",
                        candidates = stats.candidates,
                        read = stats.read,
                        skipped = stats.skipped,
                        records = stats.records,
                        store_reads = stats.store_reads,
                        "session ingestion cursor pass"
                    ),
                    Err(e) => {
                        tracing::warn!(source = "antigravity", error = %e, "ingestion pass failed")
                    }
                }

                match oc_importer.sync(ingestion_store.as_ref()).await {
                    Ok(stats) => tracing::info!(
                        source = "opencode",
                        candidates = stats.candidates,
                        read = stats.read,
                        skipped = stats.skipped,
                        hot_deferred = stats.hot_deferred,
                        message_rows = stats.message_rows,
                        records = stats.records,
                        store_reads = stats.store_reads,
                        "session ingestion cursor pass"
                    ),
                    Err(e) => {
                        tracing::warn!(source = "opencode", error = %e, "ingestion pass failed")
                    }
                }

                match hermes_importer.sync(ingestion_store.as_ref()).await {
                    Ok(stats) => tracing::info!(
                        source = "hermes",
                        read = stats.read,
                        skipped = stats.skipped,
                        records = stats.records,
                        store_reads = stats.store_reads,
                        "session ingestion cursor pass"
                    ),
                    Err(e) => {
                        tracing::warn!(source = "hermes", error = %e, "ingestion pass failed")
                    }
                }

                match codex_importer.sync(ingestion_store.as_ref()).await {
                    Ok(stats) => tracing::info!(
                        source = "codex",
                        candidates = stats.candidates,
                        read = stats.read,
                        skipped = stats.skipped,
                        errors = stats.errors,
                        records = stats.records,
                        "session ingestion cursor pass"
                    ),
                    Err(e) => {
                        tracing::warn!(source = "codex", error = %e, "ingestion pass failed")
                    }
                }

                end_cycle(&in_flight);

                // Ingestion cadence is operator-tunable without a rebuild.
                tokio::time::sleep(tokio::time::Duration::from_secs(ingestion_interval_secs)).await;
            }
        });
    }

    let protected_routes = Router::new()
        .merge(
            xavier::server::training_routes::router(
                xavier::server::training_routes::TrainingState {
                    db_path: state.workspace_dir.clone().join("data/vec-store.sqlite3"),
                    data_dir: state.workspace_dir.clone().join("data/datasets"),
                },
            )
            .with_state(()),
        )
        .merge({
            let data_dir = state.workspace_dir.clone().join("data");
            let cfg = xavier::training::routes::TrainingJobsConfig::from_env(
                data_dir.join("training"),
                data_dir.join("datasets"),
            );
            xavier::training::routes::router(cfg)
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "training jobs API disabled");
                    Router::new()
                })
                .with_state(())
        })
        .nest(
            "/auth/google",
            xavier::server::auth_routes::router(
                xavier::server::auth_routes::GoogleOAuthState::default(),
            )
            .with_state(()),
        )
        .merge(
            xavier::server::f12_routes::router(xavier::server::f12_routes::F12State::new(
                state.workspace_dir.clone().join("data"),
            ))
            .with_state(()),
        )
        .merge(
            xavier::server::mesh_governance_routes::router(
                xavier::server::mesh_governance_routes::MeshGovernanceState::new(),
            )
            .with_state(()),
        )
        // ── DocBot & Document Management API ────────────────────────────────
        .nest(
            "/api/docbot/v1",
            {
                let docbot_db_path = state
                    .workspace_dir
                    .clone()
                    .join("data/docbot_collections.db");
                if let Some(parent) = docbot_db_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let docbot_store = Arc::new(
                    xavier::collections::store::CollectionStore::open(&docbot_db_path)
                        .unwrap_or_else(|_| {
                            let mem_path =
                                std::env::temp_dir().join("docbot_collections_fallback.db");
                            xavier::collections::store::CollectionStore::open(&mem_path)
                                .expect("fallback docbot store")
                        }),
                );
                let docbot_pipeline = Arc::new(xavier::rag::pipeline::DocBotPipeline::new(
                    docbot_store.clone(),
                    xavier::rag::llm_adapter::LlmConfig {
                        backend: xavier::rag::llm_adapter::LlmBackend::RetrievalOnly,
                        ..Default::default()
                    },
                    xavier::rag::pipeline::PipelineConfig::default(),
                ));
                let docbot_state = xavier::server::docbot_routes::DocBotState {
                    store: docbot_store,
                    pipeline: docbot_pipeline,
                    chunk_config: xavier::collections::indexer::ChunkConfig::default(),
                };
                xavier::server::docbot_routes::docbot_router(docbot_state)
            }
            .with_state(()),
        )
        // ── PageIndex tree retrieval API (same auth as the rest) ────────────
        .merge({
            #[cfg(feature = "pageindex")]
            let pageindex = Router::new().nest(
                "/v1/pageindex",
                xavier::server::pageindex_routes::router(
                    xavier::pageindex_glue::state::shared_state(),
                ),
            );
            #[cfg(not(feature = "pageindex"))]
            let pageindex = Router::new();
            pageindex.with_state(())
        })
        // ── Memory Sync endpoints ──────────────────────────────────────────
        .route(
            "/v1/memory/manifest",
            get(crate::cli::handlers::memory::memory_manifest_handler),
        )
        .route(
            "/v1/memory/pull",
            post(crate::cli::handlers::memory::memory_pull_handler),
        )
        .route(
            "/v1/memory/pull-since/{workspace_id}/{since}",
            get(crate::cli::handlers::memory::memory_pull_since_handler),
        )
        .route(
            "/api/v1/memory/sync/push",
            post(xavier::adapters::inbound::http::handlers::sync::sync_push_handler),
        )
        .route(
            "/api/v1/memory/sync/pull",
            post(xavier::adapters::inbound::http::handlers::sync::sync_pull_handler),
        )
        .route(
            "/api/v1/memory/sync/status",
            get(xavier::adapters::inbound::http::handlers::sync::sync_status_handler),
        )
        .route(
            "/api/v1/memory/sync/resolve/{conflict_id}",
            post(xavier::adapters::inbound::http::handlers::sync::sync_resolve_handler),
        )
        .merge(memory_routes())
        .route("/agents", get(agent_list_handler))
        .route("/workspace/default", get(workspace_info_handler))
        // ── Clavis Key Vault API ─────────────────────────────────────────
        // Mounted here as well as in `routes::create_router()`: the live
        // `xavier http` server builds its router in this module, so routing
        // the endpoints only in `routes.rs` left them 404 in production while
        // the handler tests (which call `create_router()`) stayed green.
        .route(
            "/v1/clavis/keys/{key_name}",
            get(xavier::adapters::inbound::http::handlers::clavis::get_clavis_key_handler)
                .put(xavier::adapters::inbound::http::handlers::clavis::put_clavis_key_handler)
                .layer(middleware::from_fn(require_permission(|r| {
                    r.can_manage_secrets()
                }))),
        )
        .route(
            "/v1/clavis/proxy",
            post(xavier::adapters::inbound::http::handlers::clavis::clavis_proxy_handler).layer(
                middleware::from_fn(require_permission(|r| r.can_manage_secrets())),
            ),
        )
        .route(
            "/v1/workspaces/db",
            post(create_workspace_db_handler).get(list_workspace_dbs_handler),
        )
        .route(
            "/v1/workspaces/db/{id}",
            delete(delete_workspace_db_handler).layer(middleware::from_fn(require_permission(
                |r| r.can_manage_users(),
            ))),
        )
        .route(
            "/v1/onboarding/suggestions",
            get(onboarding_suggestions_handler),
        )
        .route("/v1/auth/sessions", get(list_sessions_handler))
        .route(
            "/v1/auth/sessions/{id}",
            delete(revoke_session_handler).layer(middleware::from_fn(require_permission(|r| {
                r.can_manage_users()
            }))),
        )
        // Memory Knowledge Graph (EntityGraph)
        .route("/memory/graph/entities", get(memory_graph_list_entities))
        .route(
            "/memory/graph/entities/{entity_id}",
            get(memory_graph_entity),
        )
        .route("/memory/graph/relations", get(memory_graph_relations))
        .route("/memory/graph/view", get(memory_graph_view))
        .route("/code/find", post(code_find_handler))
        // #2580: index, dump and load read and write the filesystem, so each
        // sits behind the Admin role gate. The routes and their gate live in
        // `code_fs_routes` so the router tests cover this very wiring (#2793).
        .merge(crate::cli::handlers::code::code_fs_routes())
        .route("/code/search", post(code_search_handler))
        .route("/code/memories", post(code_memories_handler))
        .route("/code/context", post(code_context_handler))
        .route("/code/stats", get(code_stats_handler))
        .route("/code/sync", post(code_sync_handler))
        .route("/code/dependencies", post(code_dependencies_handler))
        .route(
            "/code/reverse-dependencies",
            post(code_reverse_dependencies_handler),
        )
        .route("/code/call-chain", post(code_call_chain_handler))
        .route("/code/blast-radius", post(code_blast_radius_handler))
        // Code graph canvas projection
        .route("/code/graph/view", get(code_graph_view_handler))
        .route("/code/hubs", get(code_hubs_handler))
        .route("/code/hotspots", get(code_hotspots_handler))
        .route("/v1/account/usage", get(account_usage_handler))
        .route("/v1/embeddings", post(embed_handler))
        .route("/v1/embeddings/stats", get(embedding_stats_handler))
        .route("/v1/auth/session", post(session_create_handler))
        // Deprecated legacy user-auth API (GH #2545): these paths used to be documented as
        // canonical but were backed by a user store nothing ever populated. They now answer
        // 308 (equivalent contract at /auth/*) or 410 (no direct successor) instead of a
        // silent 401/404 — see `src/cli/handlers/auth.rs` module docs for the full rationale.
        // `/v1/auth/sessions*` (root-token sessions, routed above) is unrelated and unaffected.
        .nest(
            "/v1/auth",
            Router::new()
                .route("/login", post(deprecated_v1_login_handler))
                .route("/register", post(deprecated_v1_register_handler))
                .route("/refresh", post(deprecated_v1_refresh_handler))
                .route("/logout", post(deprecated_v1_logout_handler))
                .route("/recover", post(deprecated_v1_recover_handler))
                .route("/totp/verify", post(deprecated_v1_totp_verify_handler))
                .route("/totp/setup", post(deprecated_v1_totp_setup_handler)),
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
        // ── Maloca Timeline Export API (MS-002) ──────────────────────────
        .route(
            "/maloca/timeline",
            get(xavier::maloca::timeline::timeline_export),
        )
        .route(
            "/maloca/timeline/sessions",
            get(xavier::maloca::timeline::timeline_sessions),
        )
        .route(
            "/maloca/timeline/{id}/context",
            get(xavier::maloca::timeline::timeline_event_context),
        )
        // ── Maloca Commit Chronicle API (MS-003) ─────────────────────────
        .route(
            "/maloca/commits/graph",
            get(xavier::maloca::commits::commits_graph),
        )
        // ── Maloca WebSocket Live Feed (MS-004) ──────────────────────────
        .route("/maloca/ws/feed", get(xavier::maloca::ws::ws_live_feed))
        // NOTE: GET /maloca/feed/status se registra en maloca::nested_router (src/maloca/mod.rs:53)
        // y se mergea abajo — NO duplicar aquí (panic axum "Overlapping method route").
        // ── Maloca Belief Graph Confidence (MS-005) ──────────────────────
        .route(
            "/maloca/beliefs",
            get(xavier::maloca::beliefs::beliefs_snapshot),
        )
        .route(
            "/maloca/beliefs/path",
            get(xavier::maloca::beliefs::belief_path),
        )
        .route(
            "/api/settings/cloud-node",
            get(xavier::api::settings::get_cloud_node).post(
                xavier::api::settings::update_cloud_node.layer(middleware::from_fn(
                    require_permission(|r| r.can_edit_config()),
                )),
            ),
        )
        .route(
            "/api/settings/discord",
            get(xavier::api::settings::get_discord_settings).post(
                xavier::api::settings::update_discord_settings.layer(middleware::from_fn(
                    require_permission(|r| r.can_edit_config()),
                )),
            ),
        )
        .route(
            "/api/settings/discord/test",
            post(
                xavier::api::settings::test_discord_connection.layer(middleware::from_fn(
                    require_permission(|r| r.can_edit_config()),
                )),
            ),
        )
        .route(
            "/api/settings/telegram",
            get(xavier::api::settings::get_telegram_settings).post(
                xavier::api::settings::update_telegram_settings.layer(middleware::from_fn(
                    require_permission(|r| r.can_edit_config()),
                )),
            ),
        )
        .route(
            "/api/settings/telegram/test",
            post(
                xavier::api::settings::test_telegram_connection.layer(middleware::from_fn(
                    require_permission(|r| r.can_edit_config()),
                )),
            ),
        )
        .route("/xavier/events/session", post(session_event_handler))
        .route(
            "/xavier/events",
            get(xavier::adapters::inbound::http::routes::events_replay_handler),
        )
        .route("/xavier/time/metric", post(time_metric_handler))
        .route("/xavier/agents/register", post(agent_register_handler))
        .route("/xavier/agents/active", get(agent_active_handler))
        .route("/xavier/agents/scan", get(agent_scan_handler))
        .route("/xavier/agents/index", post(agent_index_handler))
        .route("/xavier/openclaw/scan", get(openclaw_scan_handler))
        .route("/xavier/openclaw/index", post(openclaw_index_handler))
        .route(
            "/xavier/codex/index",
            post(crate::cli::handlers::agent::codex_index_handler),
        )
        .route(
            "/xavier/jules/index",
            post(crate::cli::handlers::agent::jules_index_handler),
        )
        .route(
            "/xavier/antigravity/index",
            post(crate::cli::handlers::agent::antigravity_index_handler),
        )
        .route(
            "/xavier/opencode/index",
            post(crate::cli::handlers::agent::opencode_index_handler),
        )
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
        .merge(crate::cli::handlers::secrets::secrets_routes())
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
            get(list_tokens_handler).post(create_token_handler.layer(middleware::from_fn(
                require_permission(|r| r.can_manage_users()),
            ))),
        )
        .route(
            "/security/tokens/{id}",
            delete(revoke_token_handler).layer(middleware::from_fn(require_permission(|r| {
                r.can_manage_users()
            }))),
        )
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
        // ── Offline Models API ──────────────────────────────────────────
        .route(
            "/v1/offline/config",
            get(crate::cli::handlers::offline_models::get_offline_config_handler).post(
                crate::cli::handlers::offline_models::update_offline_config_handler.layer(
                    middleware::from_fn(require_permission(|r| r.can_edit_config())),
                ),
            ),
        )
        .route(
            "/v1/offline/models",
            get(crate::cli::handlers::offline_models::list_offline_models_handler),
        )
        .route(
            "/v1/offline/status",
            get(crate::cli::handlers::offline_models::get_offline_status_handler),
        )
        .route(
            "/v1/offline/download",
            post(crate::cli::handlers::offline_models::download_offline_model_handler),
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
            "/v1/agents/mini-experts",
            get(crate::cli::handlers::mini_experts::list_experts_handler),
        )
        .route(
            "/v1/agents/mini-experts/invoke",
            post(crate::cli::handlers::mini_experts::invoke_expert_handler).layer(
                middleware::from_fn(require_permission(|r| r.can_add_memory())),
            ),
        )
        .route(
            "/v1/agents/mini-experts/{name}",
            get(crate::cli::handlers::mini_experts::get_expert_handler),
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
            "/v1/mesh/health",
            get(xavier::server::v1_api::v1_mesh_health),
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
        .route(
            "/v1/mesh/chat/send",
            post(xavier::server::mesh_chat_routes::v1_mesh_chat_send),
        )
        .route(
            "/v1/mesh/chat/history/{room_or_peer}",
            get(xavier::server::mesh_chat_routes::v1_mesh_chat_history),
        )
        .route(
            "/v1/mesh/chat/signal",
            post(xavier::server::mesh_chat_routes::v1_mesh_chat_signal),
        )
        .route(
            "/mesh/public/nodes",
            get(crate::cli::handlers::nodes::list_public_nodes_handler),
        )
        .route(
            "/v1/mesh/public/nodes",
            get(crate::cli::handlers::nodes::list_public_nodes_handler),
        )
        .route(
            "/v1/mesh/peers",
            get(list_peers_handler).post(crate::cli::handlers::mesh::add_peer_handler),
        )
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
        .route(
            "/v1/mesh/bridges",
            post(crate::cli::handlers::mesh::create_bridge_handler)
                .get(crate::cli::handlers::mesh::list_bridges_handler),
        )
        .route(
            "/v1/mesh/bridges/{id}",
            delete(crate::cli::handlers::mesh::delete_bridge_handler),
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
            "/notifications/stream",
            get(crate::cli::handlers::notifications::stream_notifications_handler),
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
        .route(
            "/notifications/subscriptions",
            get(crate::cli::handlers::notifications::list_subscriptions_handler)
                .post(crate::cli::handlers::notifications::create_subscription_handler),
        )
        .route(
            "/notifications/subscriptions/{id}",
            axum::routing::delete(crate::cli::handlers::notifications::delete_subscription_handler),
        )
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit_middleware,
        ))
        // Clearance derivado de la identidad autenticada (corre DESPUÉS de auth).
        .layer(middleware::from_fn(
            xavier::adapters::inbound::http::middleware::clearance::clearance_session_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let large_body_routes = Router::new()
        .merge(memory_large_body_routes())
        .route("/panel/api/chat", post(panel_process_chat))
        .route("/code/scan", post(code_scan_handler))
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit_middleware,
        ))
        // Clearance derivado de la identidad autenticada (corre DESPUÉS de auth).
        .layer(middleware::from_fn(
            xavier::adapters::inbound::http::middleware::clearance::clearance_session_middleware,
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
        workspace: workspace_state.clone(),
    };
    let working_snapshot_path =
        xavier::memory::working::snapshot_path(&xavier::maloca::MalocaStore::resolve_state_dir());
    xavier::memory::working::spawn_persistence(
        Arc::clone(&workspace_state.working_memory),
        working_snapshot_path.clone(),
        xavier::memory::working::snapshot_interval_from_env(),
    );
    let working_memory_for_shutdown = Arc::clone(&workspace_state.working_memory);

    // Maloca ops API — public local dogfood (matches @swal/maloca-client; no token).
    let maloca_store = state.maloca.clone();

    let app = Router::new()
        .nest(
            "/auth",
            // Limite de tasa tambien aqui: /auth/register, /auth/login y el arranque de OAuth son
            // publicos y sin esto se pueden martillear sin freno (medido: 60 intentos de login
            // seguidos, todos 401, en 0.1 s => ~600 req/s de fuerza bruta). El resto del router ya
            // lo tenia; este nest se habia quedado fuera.
            // Middleware DEDICADO y sin estado: dentro del nest el path ya viene sin el prefijo
            // (`/login`, no `/auth/login`), asi que este no puede decidir por la ruta.
            auth_routes::<CliState>(&state.state_dir.to_string_lossy())
                .layer(middleware::from_fn(auth_rate_limit_middleware)),
        )
        .route("/health", get(health_handler))
        .route(
            "/node/founder/status",
            get(xavier::node_identity::founder_status_handler),
        )
        .route(
            "/v1/node/founder/status",
            get(xavier::node_identity::founder_status_handler),
        )
        .route(
            "/healthz",
            get(crate::cli::handlers::system::healthz_handler),
        )
        .route("/health/history", get(health_history_handler))
        .route("/metrics", get(metrics_handler))
        .route("/health/cloud", get(cloud_health_handler))
        .route(
            "/system/alerts",
            get(crate::cli::handlers::system::system_alerts_handler),
        )
        .route(
            "/v1/messaging/status",
            get(crate::cli::handlers::config::get_messaging_status_handler),
        )
        .route("/v1/version", get(version_handler))
        .route("/build", get(build_handler))
        .route("/ready", get(readiness_handler))
        .route("/readiness", get(readiness_handler))
        .route("/v1/health/ready", get(readiness_handler));

    #[cfg(feature = "panel-ui")]
    let app = if !no_ui {
        // Serve index + assets at both `/` and `/panel` so portable installs and
        // bookmarked `/panel` URLs both work.
        app.route("/", get(panel_index))
            .route("/panel", get(panel_index))
            .route("/assets/{*path}", get(panel_asset))
            .route("/panel/assets/{*path}", get(panel_asset))
    } else {
        app
    };

    let app = app
        .merge(protected_routes)
        .merge(crate::cli::state::guarded_espacio_router(&state))
        .merge(large_body_routes)
        .layer(Extension(workspace_ctx.clone()))
        .layer(Extension(event_bus_for_ws));

    // Outside every auth layer (see `install_space_manager`).
    let app = crate::cli::state::install_space_manager(app, space_manager.clone());

    let app = app.with_state(state.clone());

    // Maloca ops API — public local dogfood for reads (matches @swal/maloca-client;
    // no token needed on GET/HEAD), but every mutating verb (POST/PUT/PATCH/DELETE)
    // requires the same token as the rest of the API via
    // `maloca_mutation_auth_middleware`. This router is merged in BEFORE the
    // `CorsLayer`/timeout layer below so those layers wrap Maloca too — previously
    // they were applied first and the Maloca merge happened after, leaving `/maloca/*`
    // and `/v1/maloca/*` with no CORS headers and no auth enforcement on writes.
    // HumanChallenge + introspection persist in `{data dir}/humanchallenge.db` (same data
    // dir resolution as the Maloca store). On failure fall back to in-memory (None) so the
    // daemon still boots, but say so loudly.
    let hc_store = {
        let hc_dir = xavier::maloca::MalocaStore::resolve_state_dir();
        let _ = std::fs::create_dir_all(&hc_dir);
        let hc_path = hc_dir.join("humanchallenge.db");
        match xavier::humanchallenge::HumanChallengeStore::new(&hc_path) {
            Ok(s) => {
                xavier::humanchallenge::store::set_store_backing(
                    xavier::humanchallenge::store::StoreBacking::File,
                );
                Some(Arc::new(s))
            }
            Err(e) => {
                tracing::error!(path = %hc_path.display(), error = %e,
                    "HumanChallenge store unavailable; challenges/introspection will NOT persist");
                xavier::humanchallenge::store::set_store_backing(
                    xavier::humanchallenge::store::StoreBacking::Memory,
                );
                None
            }
        }
    };
    // Opt-in harvester: only runs when XAVIER_HC_SESSIONS_DIR points at a sessions directory
    // (no default scan of an arbitrary path on the production node).
    let hc_sessions_dir = match std::env::var("XAVIER_HC_SESSIONS_DIR") {
        Ok(dir) if !dir.trim().is_empty() => Some(dir),
        Ok(_) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            tracing::warn!("XAVIER_HC_SESSIONS_DIR is not valid UTF-8; harvester disabled");
            None
        }
        Err(std::env::VarError::NotPresent) => None,
    };
    let hc_harvester = match (hc_store.clone(), hc_sessions_dir) {
        (Some(store), Some(dir)) => {
            let config = xavier::server::maloca::HcCronBridgeConfig {
                sessions_dir: PathBuf::from(dir),
                ..Default::default()
            };
            Some(
                Arc::new(xavier::server::maloca::HcCronBridge::with_shared_store(
                    config, store,
                ))
                .start_background_worker(),
            )
        }
        _ => None,
    };
    let maloca_router = xavier::maloca::nested_router::<()>(maloca_store.clone())
        .merge(xavier::server::maloca::v1_maloca_router_with_maloca_store(
            hc_store,
            Some(state.workspace_dir.clone()),
            Some(maloca_store),
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            maloca_mutation_auth_middleware,
        ));

    let agent_indexer_cron = state.agent_indexer.clone();
    let memory_port_cron = state.memory.clone();
    let app = app
        .merge(maloca_router)
        .layer(CorsLayer::permissive())
        .layer(middleware::from_fn(
            xavier::adapters::inbound::http::middleware::timeout::timeout_middleware,
        ));

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
    let expected_token = xavier::security::auth::resolve_xavier_token();
    if expected_token.is_empty() {
        tracing::warn!(
            "MCP HTTP+SSE server startup skipped: resolve_xavier_token() returned empty"
        );
    } else if mcp_port > 0 {
        let workspace_registry = Arc::new(xavier::workspace::WorkspaceRegistry::new());
        let _ = workspace_registry.insert_arc(workspace_state.clone()).await;

        let mcp_app_state = xavier::AppState {
            workspace_registry,
            code_indexer: Arc::clone(&code_indexer),
            code_query: Arc::clone(&code_query),
            code_db: Arc::clone(&code_db),
            indexer: xavier::memory::file_indexer::FileIndexer::new(
                xavier::memory::file_indexer::FileIndexerConfig::default(),
                Some(code_indexer.clone()),
            ),
            agent_indexer: xavier::memory::agent_indexer::AgentIndexer::new(
                xavier::memory::file_indexer::FileIndexer::new(
                    xavier::memory::file_indexer::FileIndexerConfig::default(),
                    Some(code_indexer.clone()),
                ),
            ),
            security_service: Arc::clone(&security_service),
            code_graph_dump_path: Some(xavier::codebase::codegraph_paths::codegraph_dump_path_for(
                &state.workspace_dir,
            )),
        };

        let bind_host = resolve_http_bind_host();
        let mcp_bind_addr = format!("{}:{}", bind_host, mcp_port);
        info!("Starting MCP HTTP+SSE server on {}", mcp_bind_addr);
        let mcp_workspace_ctx = workspace_ctx.clone();
        tokio::spawn(async move {
            if let Err(error) = xavier::server::mcp::transport::start_mcp_http_server(
                mcp_app_state,
                mcp_workspace_ctx,
                mcp_bind_addr.clone(),
            )
            .await
            {
                tracing::error!("MCP HTTP+SSE server on {} failed: {}", mcp_bind_addr, error);
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

                // El default NO se hard-codea a un modelo que puede no existir en el
                // nodo (pasó con qwen3-coder y luego con qwen2.5-coder:7b: 122 avisos/
                // día en cada nodo aunque el usuario tuviera otro modelo). Se lee el
                // modelo realmente configurado, con el mismo orden de resolución que
                // ollama_models.rs: env XAVIER_LOCAL_LLM_MODEL -> settings.local_llm_model
                // -> DEFAULT_LOCAL_MODEL del crate.
                let default_model = std::env::var("XAVIER_LOCAL_LLM_MODEL")
                    .ok()
                    .filter(|m| !m.trim().is_empty())
                    .unwrap_or_else(|| {
                        let settings = crate::settings::XavierSettings::current();
                        if settings.models.local_llm_model.trim().is_empty() {
                            xavier::agents::provider::local::DEFAULT_LOCAL_MODEL.to_string()
                        } else {
                            settings.models.local_llm_model.clone()
                        }
                    });
                if !result
                    .ollama
                    .models
                    .iter()
                    .any(|m| m.to_lowercase().contains(&default_model.to_lowercase()))
                {
                    tracing::warn!(
                        "⚠️ Configured local LLM '{}' not found in Ollama. Run: ollama pull {}",
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
        if settings.telegram.enabled {
            let memory_bot = state.memory.clone();
            let agents_bot = state.agent_registry.clone();
            let security_bot = state.security_scan.clone();
            tokio::spawn(async move {
                xavier::telegram::run_bot(memory_bot, agents_bot, security_bot).await;
            });
        } else {
            tracing::info!("Telegram bot is disabled in settings; skipping initialization to preserve idle memory");
        }
    }

    tokio::spawn(async move {
        // Env-gated agentic scanner (2026-08-24): XAVIER_AGENT_SCANNER_INTERVAL
        // en segundos; 0 deshabilita el scanner por completo.
        let scan_interval = std::env::var("XAVIER_AGENT_SCANNER_INTERVAL")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(6 * 3600);
        if scan_interval == 0 {
            info!("Agentic Scanner DISABLED (XAVIER_AGENT_SCANNER_INTERVAL=0)");
            return;
        }
        let mut interval = tokio::time::interval(Duration::from_secs(scan_interval));
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
                        deleted_at: None,
                        parent_id: None,
                        cluster_id: None,
                        level: Default::default(),
                        relation: None,
                        clearance: Default::default(),
                        revisions: vec![],
                        encrypted_dek: None,
                        content_iv: None,
                        metadata_iv: None,
                        ..Default::default()
                    };
                    let _ = memory_port_cron.add(record).await;
                }
            }
        }
    });

    let tls_config = if let (Ok(cert), Ok(key)) = (
        std::env::var("XAVIER_TLS_CERT"),
        std::env::var("XAVIER_TLS_KEY"),
    ) {
        info!("TLS 1.3 encryption enabled");
        Some(RustlsConfig::from_pem_file(cert, key).await?)
    } else {
        None
    };
    let drain = Duration::from_secs(
        std::env::var("XAVIER_SHUTDOWN_GRACE_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(10),
    );
    let tls_handle = tls_config
        .as_ref()
        .map(|_| axum_server::Handle::<std::net::SocketAddr>::new());
    let shutdown_handle = tls_handle.clone();
    let (stop_server, server_shutdown) = tokio::sync::oneshot::channel();
    let (server_done, mut server_finished) = tokio::sync::oneshot::channel();
    // Register eagerly, before the task is scheduled, so SIGTERM is never missed.
    let signal = shutdown_signal();
    let shutdown_task = tokio::spawn(async move {
        tokio::select! {
            _ = signal => {},
            _ = &mut server_finished => return,
        }
        let deadline = tokio::time::Instant::now() + drain;
        if let Some(handle) = shutdown_handle {
            handle.graceful_shutdown(Some(drain));
        }
        let _ = stop_server.send(());
        xavier::memory::working::save_if_dirty(
            &working_memory_for_shutdown,
            &working_snapshot_path,
        )
        .await;
        if let Some(harvester) = hc_harvester {
            harvester.abort();
        }
        if let Some(shutdown) = sync_shutdown {
            shutdown.shutdown();
            shutdown.wait_for_shutdown(Duration::from_secs(5)).await;
        }
        // Cleanup must finish before the deadline can force an exit.
        tokio::select! {
            biased;
            _ = &mut server_finished => {},
            _ = tokio::time::sleep_until(deadline) => {
                tracing::warn!("HTTP shutdown grace period expired after cleanup; forcing exit");
                std::process::exit(0);
            }
        }
    });

    use std::net::SocketAddr;
    let server_result = if let Some(rustls_config) = tls_config {
        let addr = listener.local_addr()?;
        drop(listener);
        axum_server::bind_rustls(addr, rustls_config)
            .handle(tls_handle.expect("TLS shutdown handle"))
            .serve(app.into_make_service_with_connect_info::<SocketAddr>())
            .await
    } else {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = server_shutdown.await;
        })
        .await
    };
    let _ = server_done.send(());
    shutdown_task.await?;
    server_result?;

    Ok(())
}

/// Registers shutdown listeners eagerly and waits for either termination signal.
pub(crate) fn shutdown_signal() -> impl std::future::Future<Output = &'static str> + Send {
    #[cfg(unix)]
    let (mut terminate, mut interrupt) = {
        use tokio::signal::unix::{signal, SignalKind};
        (
            signal(SignalKind::terminate()).expect("install SIGTERM listener"),
            signal(SignalKind::interrupt()).expect("install SIGINT listener"),
        )
    };

    async move {
        #[cfg(unix)]
        let signal = tokio::select! {
            _ = terminate.recv() => "SIGTERM",
            _ = interrupt.recv() => "SIGINT",
        };
        #[cfg(not(unix))]
        let signal = {
            tokio::signal::ctrl_c()
                .await
                .expect("install Ctrl+C listener");
            "SIGINT"
        };
        info!(signal, "HTTP server shutdown requested");
        signal
    }
}

#[cfg(all(test, unix))]
mod shutdown_tests {
    #[tokio::test]
    #[serial_test::serial]
    async fn shutdown_signal_receives_sigterm() {
        let signal = super::shutdown_signal();
        // The eager listener above replaces SIGTERM's default disposition.
        // `kill(1)` keeps the crate free of `unsafe` (lib.rs: deny(unsafe_code)).
        let status = std::process::Command::new("kill")
            .args(["-TERM", &std::process::id().to_string()])
            .status()
            .expect("run kill -TERM");
        assert!(status.success(), "kill -TERM failed: {status}");
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(5), signal)
                .await
                .expect("SIGTERM should resolve shutdown within five seconds"),
            "SIGTERM"
        );
    }
}

/// Cadence of the universal agent session ingestion loop, in seconds.
///
/// Reads `XAVIER_INGESTION_INTERVAL_SECS` so operators can tune it without a
/// rebuild (default 600). `0` disables the loop (emergency brake for
/// embedding storms); surrounding whitespace and a trailing `s` are tolerated; unparsable values
/// fall back to the default.
pub fn ingestion_interval_secs() -> u64 {
    std::env::var("XAVIER_INGESTION_INTERVAL_SECS")
        .ok()
        .and_then(|v| parse_interval_secs(&v))
        .unwrap_or(600)
}

/// Parses an interval in seconds, tolerating surrounding whitespace and a
/// trailing `s` unit (`"0 "`, `"0s"`, `" 300s "`). A near-miss of `0` must
/// still read as `0`: falling back to the default would silently re-enable the
/// loop an operator just tried to brake.
fn parse_interval_secs(raw: &str) -> Option<u64> {
    let t = raw.trim();
    let t = t
        .strip_suffix('s')
        .or_else(|| t.strip_suffix('S'))
        .unwrap_or(t)
        .trim();
    t.parse::<u64>().ok()
}

/// Attempts to mark an ingestion cycle as in-flight.
/// Returns `true` if the cycle was successfully started (flag changed false -> true),
/// or `false` if another cycle is already running.
pub fn try_begin_cycle(flag: &AtomicBool) -> bool {
    flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

/// Marks the current ingestion cycle as complete, resetting the in-flight flag.
pub fn end_cycle(flag: &AtomicBool) {
    flag.store(false, Ordering::Release);
}

#[cfg(test)]
mod ingestion_interval_tests {
    use super::ingestion_interval_secs;

    #[test]
    fn defaults_to_600_without_env() {
        std::env::remove_var("XAVIER_INGESTION_INTERVAL_SECS");
        assert_eq!(ingestion_interval_secs(), 600);
    }

    #[test]
    fn honors_env_and_zero_disables() {
        std::env::set_var("XAVIER_INGESTION_INTERVAL_SECS", "1800");
        assert_eq!(ingestion_interval_secs(), 1800);
        std::env::set_var("XAVIER_INGESTION_INTERVAL_SECS", "0");
        assert_eq!(ingestion_interval_secs(), 0);
        std::env::remove_var("XAVIER_INGESTION_INTERVAL_SECS");
    }

    #[test]
    fn zero_with_whitespace_or_unit_still_disables() {
        for raw in ["0 ", " 0", "0s", "0 s", "0S", "\t0s\n"] {
            std::env::set_var("XAVIER_INGESTION_INTERVAL_SECS", raw);
            assert_eq!(
                ingestion_interval_secs(),
                0,
                "{raw:?} must disable the loop"
            );
        }
        std::env::set_var("XAVIER_INGESTION_INTERVAL_SECS", " 300s ");
        assert_eq!(ingestion_interval_secs(), 300);
        std::env::remove_var("XAVIER_INGESTION_INTERVAL_SECS");
    }

    #[test]
    fn unparsable_falls_back_to_default() {
        std::env::set_var("XAVIER_INGESTION_INTERVAL_SECS", "not-a-number");
        assert_eq!(ingestion_interval_secs(), 600);
        std::env::remove_var("XAVIER_INGESTION_INTERVAL_SECS");
    }
}

#[cfg(test)]
mod ingestion_guard_tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn cycle_guard_serializes() {
        let flag = AtomicBool::new(false);

        // First attempt succeeds
        assert!(try_begin_cycle(&flag));

        // Second overlapping attempt fails
        assert!(!try_begin_cycle(&flag));

        // End the first cycle
        end_cycle(&flag);

        // Subsequent attempt succeeds again
        assert!(try_begin_cycle(&flag));
    }

    #[test]
    fn end_cycle_resets() {
        let flag = AtomicBool::new(true);

        // Double-ending is harmless
        end_cycle(&flag);
        end_cycle(&flag);

        // Flag is false, so try_begin_cycle succeeds
        assert!(try_begin_cycle(&flag));
    }
}

#[cfg(test)]
mod ingestion_wiring_tests {
    //! Guards the production wiring of the periodic ingestion loop.
    //!
    //! The importers hold an in-memory cursor. The loop must therefore build
    //! them ONCE, outside `loop {`. These are structural tests on this file's
    //! own source because the wiring itself is a runtime shape inside a spawned
    //! task that no unit test can observe; the cursor behaviour itself is
    //! covered in the importer modules (`cursor_survives_across_cycles`).

    fn server_source() -> &'static str {
        include_str!("server.rs")
    }

    /// Byte offset of the ingestion cycle `loop {`.
    fn cycle_loop_offset(src: &str) -> usize {
        src.find("loop {\n                if !try_begin_cycle(&in_flight)")
            .expect("ingestion cycle loop not found in server.rs")
    }

    #[test]
    fn importers_are_built_before_the_cycle_loop() {
        let src = server_source();
        let loop_at = cycle_loop_offset(src);

        for (name, ctor) in [
            ("Antigravity", "AntigravityImporter::new()"),
            ("OpenCode", "OpenCodeImporter::new()"),
            ("Hermes", "HermesImporter::new()"),
            ("Codex", "CodexImporter::with_embedder("),
        ] {
            let at = src
                .find(ctor)
                .unwrap_or_else(|| panic!("{} constructor not found in server.rs", name));
            assert!(
                at < loop_at,
                "{} is constructed INSIDE the ingestion cycle loop (offset {} >= loop {}). \
                 Its cursor is discarded every cycle and sync degrades to a full scan.",
                name,
                at,
                loop_at
            );
        }
    }

    /// No importer constructor may appear inside the cycle body (D8: the Codex
    /// importer used to be rebuilt every cycle; `find` above only sees the
    /// first match, so this checks the loop body itself).
    #[test]
    fn no_importer_is_constructed_inside_the_cycle_body() {
        let src = server_source();
        let loop_at = cycle_loop_offset(src);
        let body_end = loop_at
            + src[loop_at..]
                .find("end_cycle(&in_flight);")
                .expect("cycle body end not found");
        let body = &src[loop_at..body_end];
        for needle in ["Importer::new(", "Importer::with_"] {
            assert!(
                !body.contains(needle),
                "an importer is constructed inside the cycle loop ({needle}): its cursor is lost every cycle"
            );
        }
    }

    /// The periodic pass must use `sync` for every importer. `import_all` in
    /// the cycle loop is a full scan: on the live node that was 1723 OpenCode
    /// sessions and 40 593 messages re-read every 600 s at 78-94 % CPU.
    #[test]
    fn cycle_loop_uses_sync_and_never_import_all() {
        let src = server_source();
        let loop_at = cycle_loop_offset(src);
        let spawn_end = loop_at
            + src[loop_at..]
                .find("end_cycle(&in_flight);")
                .expect("cycle body end not found");

        let body = &src[loop_at..spawn_end];

        for importer in [
            "ag_importer",
            "oc_importer",
            "hermes_importer",
            "codex_importer",
        ] {
            assert!(
                body.contains(&format!("{}.sync(", importer)),
                "{} is not driven by sync() in the cycle loop",
                importer
            );
            assert!(
                !body.contains(&format!("{}.import_all(", importer)),
                "{} still calls import_all() in the cycle loop: that is a full scan",
                importer
            );
        }
    }

    /// Cursor stats must reach `info`, not `debug`: at debug level an idle node
    /// cannot demonstrate `read=0 skipped=N` in the journal or /health, so the
    /// fix would be unobservable in production.
    #[test]
    fn cursor_stats_are_logged_at_info() {
        let src = server_source();
        let loop_at = cycle_loop_offset(src);
        let body_end = loop_at
            + src[loop_at..]
                .find("end_cycle(&in_flight);")
                .expect("cycle body end not found");
        let body = &src[loop_at..body_end];

        let info_logs = body.matches("tracing::info!").count();
        assert!(
            info_logs >= 3,
            "expected one info! cursor log per source (antigravity, opencode, hermes), found {}",
            info_logs
        );
        assert!(
            !body.contains("tracing::debug!(\n                        read = stats.read"),
            "cursor stats are still logged at debug level, where production cannot observe them"
        );
        for field in ["read = stats.read", "skipped = stats.skipped"] {
            assert!(
                body.contains(field),
                "cursor log does not expose `{}`",
                field
            );
        }
    }

    /// The in-flight guard must survive the wiring change: overlapping cycles
    /// would fold two snapshots of the cursor into one lost update.
    #[test]
    fn in_flight_guard_still_wraps_the_cycle() {
        let src = server_source();
        let loop_at = cycle_loop_offset(src);
        let body = &src[loop_at..];
        let guard = body
            .find("if !try_begin_cycle(&in_flight)")
            .expect("guard missing");
        let first_sync = body.find("ag_importer.sync(").expect("first sync missing");
        let release = body
            .find("end_cycle(&in_flight);")
            .expect("release missing");

        assert!(
            guard < first_sync && first_sync < release,
            "cycle guard must bracket the sync passes: guard={} sync={} release={}",
            guard,
            first_sync,
            release
        );
    }
}

#[cfg(test)]
mod ensure_http_token_tests {
    use super::ensure_http_token_configured;

    #[test]
    fn ensure_http_token_rejects_empty_and_whitespace() {
        xavier::isolate_test_process!();
        assert!(ensure_http_token_configured("").is_err());
        assert!(ensure_http_token_configured("  \t\n").is_err());
    }

    #[test]
    fn ensure_http_token_accepts_present() {
        xavier::isolate_test_process!();
        assert!(ensure_http_token_configured("a-token").is_ok());
    }
}

#[cfg(test)]
mod ingestion_persisted_cursor_tests {
    use super::build_session_importers;
    use std::path::{Path, PathBuf};
    use xavier::embedding::NoopEmbedder;
    use xavier::memory::store::{InMemoryMemoryStore, MemoryStore};

    /// Puts env vars back when the test ends, including on assertion failure.
    struct RestoreEnv {
        saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl RestoreEnv {
        fn set(pairs: &[(&'static str, &Path)]) -> Self {
            let saved = pairs
                .iter()
                .map(|(key, value)| {
                    let prev = std::env::var_os(key);
                    std::env::set_var(key, value);
                    (*key, prev)
                })
                .collect();
            Self { saved }
        }
    }

    impl Drop for RestoreEnv {
        fn drop(&mut self) {
            for (key, prev) in self.saved.drain(..) {
                match prev {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    fn write_opencode_db(path: &Path) -> std::io::Result<()> {
        let conn =
            rusqlite::Connection::open(path).map_err(|e| std::io::Error::other(e.to_string()))?;
        conn.execute_batch(
            "CREATE TABLE session (
                id TEXT PRIMARY KEY,
                title TEXT,
                agent TEXT,
                model TEXT,
                time_created INTEGER,
                time_updated INTEGER
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT,
                time_created INTEGER,
                data TEXT
            );
            CREATE TABLE part (
                id TEXT PRIMARY KEY,
                message_id TEXT,
                session_id TEXT,
                time_created INTEGER,
                data TEXT
            );
            INSERT INTO session VALUES (
                'ses_a', 'title', 'coder', 'model', 1600000000000, 1600000000000
            );
            INSERT INTO message VALUES (
                'm1', 'ses_a', 1600000000001, '{\"role\":\"user\"}'
            );
            INSERT INTO part VALUES (
                'p1', 'm1', 'ses_a', 1600000000001,
                '{\"type\":\"text\",\"text\":\"hello opencode\"}'
            );",
        )
        .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(())
    }

    fn write_sources(root: &Path) -> std::io::Result<(PathBuf, PathBuf, PathBuf, PathBuf)> {
        let ag = root.join("ag");
        let logs = ag.join("sess_a").join(".system_generated").join("logs");
        std::fs::create_dir_all(&logs)?;
        std::fs::write(
            logs.join("transcript.jsonl"),
            "{\"step_index\":0,\"type\":\"USER_INPUT\",\"content\":\"hello antigravity\"}\n",
        )?;

        let oc = root.join("opencode.db");
        write_opencode_db(&oc)?;

        let hermes = root.join("hermes");
        std::fs::create_dir_all(&hermes)?;
        std::fs::write(
            hermes.join("s1.json"),
            r#"{"session_id":"s1","request":{"body":{"model":"m","messages":[{"role":"user","content":"hello hermes"},{"role":"assistant","content":"ack"}]}}}"#,
        )?;

        let codex = root.join("codex");
        std::fs::create_dir_all(&codex)?;
        std::fs::write(
            codex.join("a.json"),
            r#"{"session_id":"a","messages":[{"role":"user","content":"hello codex"}]}"#,
        )?;
        Ok((ag, oc, hermes, codex))
    }

    async fn stored(store: &InMemoryMemoryStore) -> [usize; 4] {
        [
            store.list("agent:antigravity").await.expect("ag").len(),
            store.list("agent:opencode").await.expect("oc").len(),
            store.list("agent:hermes").await.expect("hermes").len(),
            store.list("agent:codex").await.expect("codex").len(),
        ]
    }

    /// A second process over unchanged sources reads and imports nothing.
    /// The first set is dropped: an in-memory cursor would skip on its own,
    /// and only the files under `ingest-cursors/` make the new set a no-op.
    #[tokio::test]
    async fn second_ingestion_pass_over_unchanged_sources_reads_nothing_new() {
        let src = include_str!("server.rs");
        let after_disabled = src
            .split_once("Universal agent session ingestion DISABLED")
            .expect("disabled branch")
            .1;
        let call_at = after_disabled
            .find("build_session_importers(")
            .expect("the enabled loop must build importers through build_session_importers");
        assert!(
            after_disabled[..call_at].contains("} else {"),
            "persisted cursors must be built only when the ingestion loop is enabled"
        );
        let dir_needle = format!("{}{}", "data_dir.join(\"", "ingest-cursors\")");
        assert!(
            src.contains(&dir_needle),
            "cursor files must live in an ingest-cursors/ directory"
        );
        for name in [
            "antigravity.json",
            "opencode.json",
            "hermes.json",
            "codex.json",
        ] {
            let needle = format!("cursors.join(\"{name}\")");
            assert!(
                src.contains(&needle),
                "cursor file {name} is not under ingest-cursors/"
            );
        }

        let root = tempfile::tempdir().expect("tempdir");
        let data_dir = root.path().join("daemon-data");
        let (ag, oc, hermes, codex) = write_sources(root.path()).expect("fixtures");
        let _env = RestoreEnv::set(&[
            ("ANTIGRAVITY_BRAIN_DIR", ag.as_path()),
            ("OPENCODE_DB_PATH", oc.as_path()),
            ("HERMES_SESSIONS_DIR", hermes.as_path()),
            ("CODEX_SESSIONS_DIR", codex.as_path()),
        ]);

        let embedder = std::sync::Arc::new(NoopEmbedder);
        let store = InMemoryMemoryStore::new();
        let first = build_session_importers(&data_dir, embedder.clone());
        let ag1 = first.antigravity.sync(&store).await.expect("ag pass 1");
        let oc1 = first.opencode.sync(&store).await.expect("oc pass 1");
        let he1 = first.hermes.sync(&store).await.expect("hermes pass 1");
        let cx1 = first.codex.sync(&store).await.expect("codex pass 1");
        assert!(ag1.read >= 1 && oc1.read >= 1 && he1.read >= 1 && cx1.read >= 1);
        assert!(
            oc1.message_rows >= 1,
            "the first pass must read message parts"
        );
        let cursor_dir = data_dir.join("ingest-cursors");
        for name in [
            "antigravity.json",
            "opencode.json",
            "hermes.json",
            "codex.json",
        ] {
            let path = cursor_dir.join(name);
            assert!(
                path.is_file() && path.starts_with(root.path()),
                "cursor {} must be a file inside the tempdir",
                path.display()
            );
        }
        let before = stored(&store).await;
        assert!(
            before.iter().all(|n| *n > 0),
            "first pass imported nothing: {before:?}"
        );
        drop(first);

        let second = build_session_importers(&data_dir, embedder);
        let ag2 = second.antigravity.sync(&store).await.expect("ag pass 2");
        let oc2 = second.opencode.sync(&store).await.expect("oc pass 2");
        let he2 = second.hermes.sync(&store).await.expect("hermes pass 2");
        let cx2 = second.codex.sync(&store).await.expect("codex pass 2");
        assert_eq!(
            (
                ag2.read,
                ag2.records,
                oc2.read,
                oc2.records,
                oc2.message_rows
            ),
            (0, 0, 0, 0, 0),
            "antigravity/opencode second pass read new data"
        );
        assert_eq!(
            (he2.read, he2.records, cx2.read, cx2.records),
            (0, 0, 0, 0),
            "hermes/codex second pass read new data"
        );
        assert!(ag2.skipped >= 1 && oc2.skipped >= 1 && he2.skipped >= 1 && cx2.skipped >= 1);
        assert_eq!(
            stored(&store).await,
            before,
            "second pass imported new rows"
        );
    }
}
