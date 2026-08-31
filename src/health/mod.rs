//! Runtime health monitoring for Xavier
//!
//! Provides a native health loop inside the Xavier binary for:
//! - System health (CPU, RAM, disk, uptime)
//! - Database integrity (SQLite VACUUM, page count, WAL size)
//! - Embedding health (provider ping, latency, error rate)
//! - Mesh peer health (connectivity, sync lag)
//!
//! Exposes `GET /health` endpoint and auto-repair actions.

use crate::settings::XavierSettings;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;

use mesh_telemetry::MeshTelemetryCollector;

/// Singleton health state
static HEALTH_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Global mesh telemetry singleton (optional)
static MESH_TELEMETRY: std::sync::OnceLock<Arc<MeshTelemetryCollector>> =
    std::sync::OnceLock::new();

/// Global health registry
static HEALTH_REGISTRY: std::sync::OnceLock<Arc<RwLock<HealthState>>> = std::sync::OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmbeddingCoverage {
    pub indexed: u64,
    pub total: u64,
    pub percent: f64,
    pub status: String,
}

impl Default for EmbeddingCoverage {
    fn default() -> Self {
        Self {
            indexed: 0,
            total: 0,
            percent: 100.0,
            status: "healthy".to_string(),
        }
    }
}

/// Unified health response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub uptime_secs: u64,
    pub system: SystemHealth,
    pub database: DatabaseHealth,
    pub embedding: EmbeddingHealth,
    pub mesh: MeshHealth,
    #[serde(default)]
    pub telegram: TelegramHealth,
    #[serde(default)]
    pub auth: crate::security::auth::AuthHealth,
    #[serde(default)]
    pub dependency_graph: ComponentDependencyGraph,
    pub checks: Vec<HealthCheck>,
    pub embedding_coverage: EmbeddingCoverage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemHealth {
    pub cpu_usage_pct: f64,
    pub memory_used_mb: u64,
    pub memory_total_mb: u64,
    pub disk_used_gb: f64,
    pub disk_total_gb: f64,
    pub disk_usage_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseHealth {
    pub path: String,
    pub size_mb: f64,
    pub wal_size_mb: f64,
    pub page_count: u64,
    pub fragmentation_pct: f64,
    pub needs_vacuum: bool,
    pub last_vacuum: Option<u64>,
    #[serde(default)]
    pub latency_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingHealth {
    pub provider: String,
    pub connected: bool,
    pub latency_ms: f64,
    pub error_rate_pct: f64,
    pub last_success: Option<u64>,
    #[serde(default)]
    pub fallback_success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshHealth {
    pub peers_count: u32,
    pub connected_peers: u32,
    pub sync_lag_ms: f64,
    #[serde(default)]
    pub latency_ms: f64,
    pub connectivity: String,
    #[serde(default)]
    pub maturity: crate::mesh::MeshMaturityReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramHealth {
    pub enabled: bool,
    pub latency_ms: f64,
    pub status: String,
}

impl Default for TelegramHealth {
    fn default() -> Self {
        Self {
            enabled: false,
            latency_ms: 0.0,
            status: "disabled".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DependencyNode {
    pub component: String,
    pub status: String,
    pub latency_ms: f64,
    pub upstream_deps: Vec<String>,
    pub propagated_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ComponentDependencyGraph {
    pub nodes: Vec<DependencyNode>,
}

impl ComponentDependencyGraph {
    pub fn build(raw_nodes: Vec<(&str, &str, f64, Vec<&str>)>) -> Self {
        let mut raw_map = std::collections::HashMap::new();
        for (name, status, _latency_ms, _upstream) in &raw_nodes {
            raw_map.insert((*name).to_string(), (*status).to_string());
        }

        let mut nodes = Vec::new();
        for (name, status, latency_ms, upstream) in raw_nodes {
            let upstream_strs: Vec<String> = upstream.iter().map(|s| s.to_string()).collect();
            let mut propagated = status.to_string();

            if status == "healthy" {
                let mut upstream_degraded = false;
                for up in &upstream_strs {
                    if let Some(up_status) = raw_map.get(up) {
                        if up_status == "degraded"
                            || up_status == "unhealthy"
                            || up_status == "fail"
                            || up_status == "warn"
                        {
                            upstream_degraded = true;
                            break;
                        }
                    }
                }
                if upstream_degraded {
                    propagated = "degraded".to_string();
                }
            }

            nodes.push(DependencyNode {
                component: name.to_string(),
                status: status.to_string(),
                latency_ms,
                upstream_deps: upstream_strs,
                propagated_status: propagated,
            });
        }

        ComponentDependencyGraph { nodes }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    pub name: String,
    pub status: CheckStatus,
    pub detail: String,
    pub timestamp_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudHealthResponse {
    pub supabase: BackendStatus,
    pub postgres: BackendStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendStatus {
    pub status: String,
    pub detail: String,
}

/// Internal mutable health state
#[derive(Debug)]
pub struct HealthState {
    pub started_at: SystemTime,
    pub system: SystemHealth,
    pub database: DatabaseHealth,
    pub embedding: EmbeddingHealth,
    pub mesh: MeshHealth,
    pub telegram: TelegramHealth,
    pub dependency_graph: ComponentDependencyGraph,
    pub checks: Vec<HealthCheck>,
    pub embedding_coverage: EmbeddingCoverage,
}

impl Default for HealthState {
    fn default() -> Self {
        Self {
            started_at: SystemTime::now(),
            system: SystemHealth {
                cpu_usage_pct: 0.0,
                memory_used_mb: 0,
                memory_total_mb: 0,
                disk_used_gb: 0.0,
                disk_total_gb: 0.0,
                disk_usage_pct: 0.0,
            },
            database: DatabaseHealth {
                path: String::new(),
                size_mb: 0.0,
                wal_size_mb: 0.0,
                page_count: 0,
                fragmentation_pct: 0.0,
                needs_vacuum: false,
                last_vacuum: None,
                latency_ms: 0.0,
            },
            embedding: EmbeddingHealth {
                provider: String::new(),
                connected: false,
                latency_ms: 0.0,
                error_rate_pct: 0.0,
                last_success: None,
                fallback_success: false,
            },
            mesh: MeshHealth {
                peers_count: 0,
                connected_peers: 0,
                sync_lag_ms: 0.0,
                latency_ms: 0.0,
                connectivity: "unknown".to_string(),
                maturity: crate::mesh::MeshMaturityReport::default(),
            },
            telegram: TelegramHealth::default(),
            dependency_graph: ComponentDependencyGraph::default(),
            checks: Vec::new(),
            embedding_coverage: EmbeddingCoverage::default(),
        }
    }
}

/// Initialize the health registry
pub fn init_health() -> Arc<RwLock<HealthState>> {
    if HEALTH_INITIALIZED.load(Ordering::Acquire) {
        if let Some(reg) = HEALTH_REGISTRY.get() {
            return reg.clone();
        }
    }

    let state = Arc::new(RwLock::new(HealthState::default()));
    if HEALTH_REGISTRY.set(state.clone()).is_err() {
        // Already set by another thread – safe to return existing
        return HEALTH_REGISTRY.get().expect("registry just set").clone();
    }
    HEALTH_INITIALIZED.store(true, Ordering::Release);
    state
}

/// Get a reference to the health registry
pub fn health_registry() -> Option<Arc<RwLock<HealthState>>> {
    HEALTH_REGISTRY.get().cloned()
}

/// Set the mesh telemetry singleton. Returns the stored instance.
pub fn set_mesh_telemetry(collector: Arc<MeshTelemetryCollector>) -> Arc<MeshTelemetryCollector> {
    MESH_TELEMETRY.get_or_init(|| collector).clone()
}

/// Get a reference to the mesh telemetry collector, if initialized.
pub fn mesh_telemetry() -> Option<Arc<MeshTelemetryCollector>> {
    MESH_TELEMETRY.get().cloned()
}

/// Synchronous version — called from axum handlers.
///
/// Spawns a dedicated OS thread with its own multi-threaded tokio runtime
/// so that sysinfo calls and async health gathering never collide with
/// any existing tokio context (e.g. `#[tokio::test]`).
pub fn collect_health_sync() -> HealthResponse {
    let settings = XavierSettings::current();

    // Spawning a new OS thread ensures we can always block without interfering
    // with the current runtime, and avoids "cannot start a runtime from within a runtime".
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create health check runtime");

        rt.block_on(async {
            let (cpu, mem_used, mem_total, disk_used, disk_total) = gather_system_metrics();
            collect_health_impl(
                &settings, None, cpu, mem_used, mem_total, disk_used, disk_total,
            )
            .await
        })
    })
    .join()
    .expect("health thread panicked")
}

/// Async version — called from async contexts like `collect_health_sync` internals.
pub async fn collect_health(
    settings: &XavierSettings,
    db: Option<&rusqlite::Connection>,
) -> HealthResponse {
    let (cpu, mem_used, mem_total, disk_used, disk_total) =
        tokio::task::spawn_blocking(gather_system_metrics)
            .await
            .unwrap_or((0.0, 0, 0, 0.0, 0.0));
    collect_health_impl(
        settings, db, cpu, mem_used, mem_total, disk_used, disk_total,
    )
    .await
}

/// Check cloud health.
pub async fn check_cloud_health(settings: &XavierSettings) -> CloudHealthResponse {
    let mut supabase_status = BackendStatus {
        status: "not configured".to_string(),
        detail: "Supabase URL or Key not set".to_string(),
    };

    let has_supabase = (std::env::var("XAVIER_SUPABASE_URL").is_ok()
        || settings.memory.supabase_url.is_some())
        && (std::env::var("XAVIER_SUPABASE_KEY").is_ok() || settings.memory.supabase_key.is_some());

    if has_supabase {
        match crate::memory::supabase_store::SupabaseMemoryStore::from_env().await {
            Ok(store) => match store.health_check().await {
                Ok(_) => {
                    supabase_status.status = "healthy".to_string();
                    supabase_status.detail = "Connected to Supabase".to_string();
                }
                Err(e) => {
                    supabase_status.status = "unhealthy".to_string();
                    supabase_status.detail = e.to_string();
                }
            },
            Err(e) => {
                supabase_status.status = "unhealthy".to_string();
                supabase_status.detail = format!("Failed to initialize Supabase store: {}", e);
            }
        }
    }

    let mut postgres_status = BackendStatus {
        status: "not configured".to_string(),
        detail: "Postgres URL not set".to_string(),
    };

    let has_postgres =
        std::env::var("XAVIER_POSTGRES_URL").is_ok() || settings.memory.postgres_url.is_some();

    if has_postgres {
        match crate::memory::postgres_store::PostgresMemoryStore::from_env().await {
            Ok(store) => match store.health_check().await {
                Ok(_) => {
                    postgres_status.status = "healthy".to_string();
                    postgres_status.detail = "Connected to Postgres".to_string();
                }
                Err(e) => {
                    postgres_status.status = "unhealthy".to_string();
                    postgres_status.detail = e.to_string();
                }
            },
            Err(e) => {
                postgres_status.status = "unhealthy".to_string();
                postgres_status.detail = format!("Failed to initialize Postgres store: {}", e);
            }
        }
    }

    CloudHealthResponse {
        supabase: supabase_status,
        postgres: postgres_status,
    }
}

pub fn gather_embedding_coverage(settings: &XavierSettings) -> EmbeddingCoverage {
    let mut paths = Vec::new();
    if let Ok(p) = std::env::var("XAVIER_MEMORY_VEC_PATH") {
        paths.push(std::path::PathBuf::from(p));
    }
    if !settings.memory.vec_path.trim().is_empty() {
        paths.push(std::path::PathBuf::from(&settings.memory.vec_path));
    }
    paths.push(std::path::PathBuf::from(&settings.memory.data_dir).join("vec-store.sqlite3"));
    paths.push(std::path::PathBuf::from(&settings.memory.data_dir).join("xavier_memory_vec.db"));
    paths.push(std::path::PathBuf::from(&settings.memory.data_dir).join("xavier_memory.db"));
    paths.push(std::path::PathBuf::from(&settings.memory.data_dir).join("memory.db"));

    if let Ok(p) = std::env::var("XAVIER_MEMORY_SQLITE_PATH") {
        paths.push(std::path::PathBuf::from(p));
    }
    if !settings.memory.sqlite_path.trim().is_empty() {
        paths.push(std::path::PathBuf::from(&settings.memory.sqlite_path));
    }

    let mut total = 0;
    let mut indexed = 0;
    let mut found = false;

    for path in paths {
        if path.exists() {
            if let Ok(conn) = rusqlite::Connection::open_with_flags(
                &path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
            ) {
                let table_exists: rusqlite::Result<i32> = conn.query_row(
                    "SELECT 1 FROM sqlite_master WHERE type='table' AND name='memory_records'",
                    [],
                    |row| row.get(0),
                );
                if table_exists.is_ok() {
                    let total_res: rusqlite::Result<u64> =
                        conn.query_row("SELECT COUNT(*) FROM memory_records", [], |row| row.get(0));
                    let indexed_res: rusqlite::Result<u64> = conn.query_row(
                        "SELECT COUNT(*) FROM memory_records WHERE length(embedding) > 10",
                        [],
                        |row| row.get(0),
                    );
                    if let (Ok(t), Ok(ind)) = (total_res, indexed_res) {
                        total = t;
                        indexed = ind;
                        found = true;
                        break;
                    }
                }
            }
        }
    }

    let percent = if found && total > 0 {
        (indexed as f64 / total as f64) * 100.0
    } else {
        100.0
    };

    let status = if percent < 50.0 {
        "unhealthy"
    } else if percent < 80.0 {
        "degraded"
    } else {
        "healthy"
    };

    EmbeddingCoverage {
        indexed,
        total,
        percent,
        status: status.to_string(),
    }
}

/// Run a health check and return a structured response
/// Internal impl shared by both `collect_health` and `collect_health_sync`.
/// Metrics are passed in because `gather_system_metrics` contains
/// blocking sysinfo calls and must be collected on a blocking thread.
async fn collect_health_impl(
    settings: &XavierSettings,
    db: Option<&rusqlite::Connection>,
    cpu: f64,
    mem_used: u64,
    mem_total: u64,
    disk_used: f64,
    disk_total: f64,
) -> HealthResponse {
    let registry = init_health();
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let disk_pct = if disk_total > 0.0 {
        (disk_used / disk_total) * 100.0
    } else {
        0.0
    };

    let system = SystemHealth {
        cpu_usage_pct: cpu,
        memory_used_mb: mem_used,
        memory_total_mb: mem_total,
        disk_used_gb: disk_used,
        disk_total_gb: disk_total,
        disk_usage_pct: disk_pct,
    };

    // --- Embedding coverage ---
    let embedding_coverage = gather_embedding_coverage(settings);

    // --- Database health ---
    let db_start = std::time::Instant::now();
    let mut db_health = gather_db_health(settings);
    db_health.latency_ms = db_start.elapsed().as_secs_f64() * 1000.0;

    // --- Embedding health ---
    let probe_start = std::time::Instant::now();
    let (connected, latency_ms, error_rate_pct, last_success, fallback_success) =
        match crate::embedding::build_embedder_from_env().await {
            Ok(embedder) => {
                if embedder.dimension() == 0 {
                    (false, 0.0, 0.0, None, false)
                } else {
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        embedder.probe_health(),
                    )
                    .await
                    {
                        Ok(Ok(lat)) => {
                            let total_errors = crate::embedding::get_embedding_error_count();
                            let fallback_succ = total_errors > 0 || settings.embedding.embedder.contains("fallback");
                            (true, lat, 0.0, Some(now_secs), fallback_succ)
                        }
                        Ok(Err(_err)) => {
                            let total_errors = crate::embedding::get_embedding_error_count().max(1);
                            (
                                false,
                                probe_start.elapsed().as_secs_f64() * 1000.0,
                                (total_errors as f64).min(100.0),
                                None,
                                false,
                            )
                        }
                        Err(_timeout) => {
                            let total_errors = crate::embedding::get_embedding_error_count().max(1);
                            (false, 2000.0, (total_errors as f64).min(100.0), None, false)
                        }
                    }
                }
            }
            Err(_) => {
                let total_errors = crate::embedding::get_embedding_error_count().max(1);
                (false, 0.0, (total_errors as f64).min(100.0), None, false)
            }
        };

    let embedding = EmbeddingHealth {
        provider: settings.embedding.embedder.clone(),
        connected,
        latency_ms,
        error_rate_pct,
        last_success,
        fallback_success,
    };

    // Fan an unhealthy embedding out to the SYSTEM_ALERTS channel so operators
    // (and the Panel UI via /alerts) see it immediately.
    push_embedding_alert_if_unhealthy(&embedding);

    // --- Mesh health ---
    let mesh_start = std::time::Instant::now();
    let mut mesh = if let Some(telemetry) = mesh_telemetry() {
        let peers_count = telemetry.peer_count();
        let connected_peers = telemetry.connected_peer_count();
        let sync_lag_ms = telemetry.average_latency_ms();
        MeshHealth {
            peers_count,
            connected_peers,
            sync_lag_ms,
            latency_ms: 0.0,
            connectivity: if peers_count > 0 {
                "online".to_string()
            } else {
                "no peers".to_string()
            },
            maturity: crate::mesh::MeshMaturityReport::default(),
        }
    } else {
        MeshHealth {
            peers_count: 0,
            connected_peers: 0,
            sync_lag_ms: 0.0,
            latency_ms: 0.0,
            connectivity: if settings.license.mesh_accepted {
                if cfg!(feature = "mesh") {
                    "online"
                } else {
                    "disabled (mesh feature not compiled)"
                }
            } else {
                "disabled (mesh license not accepted)"
            }
            .to_string(),
            maturity: crate::mesh::MeshMaturityReport::default(),
        }
    };
    mesh.latency_ms = mesh_start.elapsed().as_secs_f64() * 1000.0;

    // --- Telegram health ---
    let telegram_start = std::time::Instant::now();
    let telegram_enabled = std::env::var("XAVIER_TELEGRAM_BOT_TOKEN").is_ok()
        || settings
            .telegram
            .bot_token
            .as_ref()
            .map(|s| !s.is_empty())
            .unwrap_or(false);
    let telegram_latency_ms = telegram_start.elapsed().as_secs_f64() * 1000.0;
    let telegram = TelegramHealth {
        enabled: telegram_enabled,
        latency_ms: telegram_latency_ms,
        status: if telegram_enabled {
            "healthy".to_string()
        } else {
            "disabled".to_string()
        },
    };

    // --- Checks ---
    let mut checks = Vec::new();

    // 1. Disk space check
    if disk_pct > 90.0 {
        checks.push(HealthCheck {
            name: "disk_space".into(),
            status: CheckStatus::Fail,
            detail: format!("Disk usage at {:.1}% — above 90% threshold", disk_pct),
            timestamp_secs: now_secs,
        });
    } else if disk_pct > 75.0 {
        checks.push(HealthCheck {
            name: "disk_space".into(),
            status: CheckStatus::Warn,
            detail: format!(
                "Disk usage at {:.1}% — above 75% warning threshold",
                disk_pct
            ),
            timestamp_secs: now_secs,
        });
    } else {
        checks.push(HealthCheck {
            name: "disk_space".into(),
            status: CheckStatus::Pass,
            detail: format!("Disk usage at {:.1}%", disk_pct),
            timestamp_secs: now_secs,
        });
    }

    // 2. Database health check
    if db_health.needs_vacuum {
        checks.push(HealthCheck {
            name: "database_integrity".into(),
            status: CheckStatus::Warn,
            detail: format!(
                "Database fragmentation at {:.1}% — VACUUM recommended",
                db_health.fragmentation_pct
            ),
            timestamp_secs: now_secs,
        });
    } else {
        checks.push(HealthCheck {
            name: "database_integrity".into(),
            status: CheckStatus::Pass,
            detail: "Database healthy".into(),
            timestamp_secs: now_secs,
        });
    }

    // 2b. SQLite integrity check (when connection is available)
    let mut sqlite_integrity_failed = false;
    if let Some(conn) = db {
        match run_integrity_check(conn) {
            Ok(ref msg) if msg == "ok" => {
                checks.push(HealthCheck {
                    name: "sqlite_integrity".into(),
                    status: CheckStatus::Pass,
                    detail: "PRAGMA integrity_check: ok".into(),
                    timestamp_secs: now_secs,
                });
            }
            Ok(msg) => {
                sqlite_integrity_failed = true;
                checks.push(HealthCheck {
                    name: "sqlite_integrity".into(),
                    status: CheckStatus::Fail,
                    detail: format!("PRAGMA integrity_check: {}", msg),
                    timestamp_secs: now_secs,
                });
            }
            Err(e) => {
                sqlite_integrity_failed = true;
                checks.push(HealthCheck {
                    name: "sqlite_integrity".into(),
                    status: CheckStatus::Fail,
                    detail: format!("integrity_check error: {}", e),
                    timestamp_secs: now_secs,
                });
            }
        }
    }

    // --- Dependency Graph & Status Propagation ---
    let db_status_str = if sqlite_integrity_failed {
        "unhealthy"
    } else if db_health.needs_vacuum {
        "degraded"
    } else {
        "healthy"
    };
    let emb_status_str =
        if settings.embedding.embedder == "disabled" || settings.embedding.embedder == "noop" {
            "disabled"
        } else if !embedding.connected || embedding.error_rate_pct > 0.0 || embedding.fallback_success {
            "degraded"
        } else {
            "healthy"
        };
    let mesh_status_str = if mesh.connectivity == "online" || mesh.connectivity == "no peers" {
        "healthy"
    } else {
        "degraded"
    };
    let tg_status_str = if telegram.enabled {
        "healthy"
    } else {
        "disabled"
    };

    let dependency_graph = ComponentDependencyGraph::build(vec![
        ("database", db_status_str, db_health.latency_ms, vec![]),
        (
            "embedding",
            emb_status_str,
            embedding.latency_ms,
            vec!["database"],
        ),
        ("mesh", mesh_status_str, mesh.latency_ms, vec!["database"]),
        (
            "telegram",
            tg_status_str,
            telegram.latency_ms,
            vec!["embedding", "database"],
        ),
    ]);

    // 3. Memory check
    if mem_total > 0 {
        let mem_pct = (mem_used as f64 / mem_total as f64) * 100.0;
        if mem_pct > 85.0 {
            checks.push(HealthCheck {
                name: "memory".into(),
                status: CheckStatus::Warn,
                detail: format!("Memory at {:.1}%", mem_pct),
                timestamp_secs: now_secs,
            });
        } else {
            checks.push(HealthCheck {
                name: "memory".into(),
                status: CheckStatus::Pass,
                detail: format!("Memory at {:.1}%", mem_pct),
                timestamp_secs: now_secs,
            });
        }
    }

    // 4. Embedding check
    if (!embedding.connected && !embedding.fallback_success) || embedding.error_rate_pct > 10.0 {
        checks.push(HealthCheck {
            name: "embedding".into(),
            status: CheckStatus::Fail,
            detail: format!(
                "Embedding provider '{}' unhealthy (connected={}, error_rate={:.1}%)",
                embedding.provider, embedding.connected, embedding.error_rate_pct
            ),
            timestamp_secs: now_secs,
        });
    } else if embedding.fallback_success {
        checks.push(HealthCheck {
            name: "embedding".into(),
            status: CheckStatus::Warn,
            detail: format!(
                "Embedding provider '{}' degraded (fallback succeeded via secondary backend)",
                embedding.provider
            ),
            timestamp_secs: now_secs,
        });
    } else {
        checks.push(HealthCheck {
            name: "embedding".into(),
            status: CheckStatus::Pass,
            detail: format!("Embedding provider '{}' healthy", embedding.provider),
            timestamp_secs: now_secs,
        });
    }

    // 5. Embedding coverage check
    let coverage_check_status = if embedding_coverage.percent < 50.0 {
        CheckStatus::Fail
    } else if embedding_coverage.percent < 80.0 {
        CheckStatus::Warn
    } else {
        CheckStatus::Pass
    };

    checks.push(HealthCheck {
        name: "embedding_coverage".into(),
        status: coverage_check_status,
        detail: format!(
            "Embedding coverage at {:.1}% ({}/{} records indexed)",
            embedding_coverage.percent, embedding_coverage.indexed, embedding_coverage.total
        ),
        timestamp_secs: now_secs,
    });

    let uptime = registry
        .read()
        .await
        .started_at
        .elapsed()
        .unwrap_or_default()
        .as_secs();

    let critical_failure = checks.iter().any(|c| {
        matches!(c.status, CheckStatus::Fail)
            && (c.name == "disk_space"
                || c.name == "memory"
                || c.name == "database_integrity"
                || c.name == "sqlite_integrity")
    }) || embedding_coverage.status == "unhealthy";

    let overall_status = if critical_failure {
        "unhealthy"
    } else if checks.iter().any(|c| matches!(c.status, CheckStatus::Fail))
        || embedding_coverage.status == "degraded"
    {
        "degraded"
    } else if checks.iter().any(|c| matches!(c.status, CheckStatus::Warn)) {
        "warn"
    } else {
        "healthy"
    };

    let response = HealthResponse {
        status: overall_status.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_secs: uptime,
        system,
        database: db_health,
        embedding,
        mesh,
        telegram,
        auth: crate::security::auth::AuthHealth::default(),
        dependency_graph: dependency_graph.clone(),
        checks,
        embedding_coverage,
    };

    // Record snapshot to 24h history ring buffer
    let mut comp_statuses = std::collections::BTreeMap::new();
    let mut comp_latencies = std::collections::BTreeMap::new();
    for node in &dependency_graph.nodes {
        comp_statuses.insert(node.component.clone(), node.propagated_status.clone());
        comp_latencies.insert(node.component.clone(), node.latency_ms);
    }

    history::record_health_history(history::HealthHistoryEntry {
        timestamp_secs: now_secs,
        status: response.status.clone(),
        component_statuses: comp_statuses,
        component_latencies_ms: comp_latencies,
        transition_reason: if response.status != "healthy" {
            Some(format!("System status: {}", response.status))
        } else {
            None
        },
    });

    // Update registry
    {
        let mut reg = registry.write().await;
        reg.system = response.system.clone();
        reg.database = response.database.clone();
        reg.embedding = response.embedding.clone();
        reg.mesh = response.mesh.clone();
        reg.telegram = response.telegram.clone();
        reg.dependency_graph = response.dependency_graph.clone();
        reg.checks = response.checks.clone();
        reg.embedding_coverage = response.embedding_coverage.clone();
    }

    response
}

/// Run SQLite PRAGMA integrity_check on the database connection
pub fn run_integrity_check(conn: &rusqlite::Connection) -> Result<String, String> {
    let mut stmt = conn
        .prepare("PRAGMA integrity_check")
        .map_err(|e| format!("integrity_check prepare: {}", e))?;
    let result: String = stmt
        .query_row([], |row| row.get(0))
        .map_err(|e| format!("integrity_check failed: {}", e))?;
    Ok(result)
}

/// Get page count and page size from a connection
pub fn get_db_page_stats(conn: &rusqlite::Connection) -> (u64, u64) {
    let page_count: u64 = conn
        .pragma_query_value(None, "page_count", |r| r.get(0))
        .unwrap_or(0);
    let page_size: u64 = conn
        .pragma_query_value(None, "page_size", |r| r.get(0))
        .unwrap_or(4096);
    (page_count, page_size)
}

/// Compute the live fragmentation of a SQLite connection from its freelist.
///
/// Returns the percentage of database pages currently on the freelist
/// (`freelist_count / page_count * 100`). This is the authoritative
/// fragmentation signal — a high ratio means many pages are allocated but
/// unused, and a `VACUUM` will compact them. Returns `0.0` for an empty DB.
pub fn conn_fragmentation_pct(conn: &rusqlite::Connection) -> f64 {
    let freelist: u64 = conn
        .pragma_query_value(None, "freelist_count", |r| r.get(0))
        .unwrap_or(0);
    let page_count: u64 = conn
        .pragma_query_value(None, "page_count", |r| r.get(0))
        .unwrap_or(0);
    if page_count == 0 {
        0.0
    } else {
        (freelist as f64 / page_count as f64) * 100.0
    }
}

/// Auto-repair: run VACUUM if fragmentation > 30%.
///
/// Fragmentation is taken as the worse of two signals: the file-level WAL-ratio
/// heuristic from [`gather_db_health`] (good for the on-disk production DB) and
/// the connection's live freelist ratio from [`conn_fragmentation_pct`] (good
/// for in-memory and pooled connections). This lets the threshold fire on real
/// free-page bloat even when the WAL heuristic is quiet.
pub async fn auto_vacuum_if_needed(
    conn: &rusqlite::Connection,
    settings: &XavierSettings,
) -> Result<(), String> {
    let db = gather_db_health(settings);
    let conn_frag = conn_fragmentation_pct(conn);
    let fragmentation = db.fragmentation_pct.max(conn_frag);
    // Trigger if either signal crosses the threshold or the file heuristic
    // already flagged it (e.g. huge page count).
    let needs_vacuum = db.needs_vacuum || conn_frag > 30.0 || fragmentation > 30.0;
    if needs_vacuum {
        tracing::info!(
            file_fragmentation = %db.fragmentation_pct,
            conn_fragmentation = %conn_frag,
            "Running auto-VACUUM on database"
        );
        conn.execute_batch("VACUUM;")
            .map_err(|e| format!("VACUUM failed: {}", e))?;
        tracing::info!("Auto-VACUUM completed successfully");

        // Run integrity check after VACUUM
        match run_integrity_check(conn) {
            Ok(msg) if msg == "ok" => {
                tracing::info!("Database integrity check passed post-VACUUM");
            }
            Ok(msg) => {
                tracing::warn!("Database integrity check post-VACUUM: {}", msg);
            }
            Err(e) => {
                tracing::error!("Database integrity check failed post-VACUUM: {}", e);
            }
        }
    }
    Ok(())
}

/// Push a SYSTEM_ALERT when the embedding provider is unhealthy.
///
/// "Unhealthy" means: not connected, an elevated error rate (>10%), or a very
/// high latency (>5s) when a provider is configured. Returns `true` when an
/// alert was pushed. This is wired into [`collect_health_impl`] so a degraded
/// embedding backend surfaces in the `/alerts` channel and the Panel UI.
pub fn push_embedding_alert_if_unhealthy(embedding: &EmbeddingHealth) -> bool {
    let unhealthy = !embedding.connected
        || embedding.error_rate_pct > 10.0
        || (!embedding.provider.is_empty() && embedding.latency_ms > 5000.0);
    if unhealthy {
        crate::server::alerts::SYSTEM_ALERTS.push_alert(
            "WARN",
            &format!(
                "Embedding provider '{}' is unhealthy (connected={}, error_rate={:.1}%, latency={:.0}ms)",
                embedding.provider, embedding.connected, embedding.error_rate_pct, embedding.latency_ms
            ),
            "embedding",
        );
    }
    unhealthy
}

// ═══════════════════════════════════════════════
// Metrics gathering (platform-independent)
// ═══════════════════════════════════════════════

fn gather_system_metrics() -> (f64, u64, u64, f64, f64) {
    let mut sys = sysinfo::System::new();
    sys.refresh_cpu_usage(); // Must refresh first; first call returns 0.0
    sys.refresh_memory();
    let mem_used = sys.used_memory() / (1024 * 1024); // MB
    let mem_total = sys.total_memory() / (1024 * 1024);

    // CPU: average across all cores, clamped to [0, 100]
    let cpus = sys.cpus();
    let cpu_usage = if !cpus.is_empty() {
        let sum: f32 = cpus.iter().map(|c| c.cpu_usage()).sum();
        (sum / cpus.len() as f32) as f64
    } else {
        sys.global_cpu_usage() as f64
    };
    let cpu_usage_pct = cpu_usage.clamp(0.0, 100.0);

    // Disk
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let total_bytes: u64 = disks.iter().map(|d| d.total_space()).sum();
    let available_bytes: u64 = disks.iter().map(|d| d.available_space()).sum();
    let disk_used = if total_bytes > 0 {
        (total_bytes - available_bytes) as f64 / (1024.0 * 1024.0 * 1024.0)
    } else {
        0.0
    };
    let disk_total = total_bytes as f64 / (1024.0 * 1024.0 * 1024.0);

    (cpu_usage_pct, mem_used, mem_total, disk_used, disk_total)
}

fn gather_db_health(settings: &XavierSettings) -> DatabaseHealth {
    // Use configured path or fall back to default
    let db_path_str = if !settings.memory.sqlite_path.is_empty() {
        settings.memory.sqlite_path.clone()
    } else if !settings.memory.file_path.is_empty() {
        settings.memory.file_path.clone()
    } else {
        format!("{}/memory.db", settings.memory.data_dir)
    };
    let db_path = std::path::Path::new(&db_path_str);
    let (size_mb, wal_size_mb, page_count, fragmentation) = if db_path.exists() {
        let size = db_path
            .metadata()
            .map(|m| m.len() as f64 / (1024.0 * 1024.0))
            .unwrap_or(0.0);
        // SQLite WAL file: <name>-wal and <name>-shm
        let wal_path = {
            let mut w = db_path.to_path_buf();
            let name = db_path
                .file_name()
                .unwrap_or_default()
                .to_str()
                .unwrap_or("memory.db");
            w.set_file_name(format!("{}-wal", name));
            if !w.exists() {
                // Try with original extension: memory.db-wal
                let mut w2 = db_path.to_path_buf();
                w2.set_file_name(format!(
                    "{}.db-wal",
                    db_path
                        .file_stem()
                        .unwrap_or_default()
                        .to_str()
                        .unwrap_or("memory")
                ));
                w2
            } else {
                w
            }
        };
        let wal = if wal_path.exists() {
            wal_path
                .metadata()
                .map(|m| m.len() as f64 / (1024.0 * 1024.0))
                .unwrap_or(0.0)
        } else {
            0.0
        };
        // Estimate fragmentation from WAL ratio
        let frag = if size > 0.0 {
            (wal / size) * 100.0
        } else {
            0.0
        };
        (size, wal, 0, frag)
    } else {
        (0.0, 0.0, 0, 0.0)
    };

    DatabaseHealth {
        path: db_path_str.clone(),
        size_mb,
        wal_size_mb,
        page_count,
        fragmentation_pct: fragmentation,
        needs_vacuum: fragmentation > 30.0 || page_count > 100000,
        last_vacuum: None,
        latency_ms: 0.0,
    }
}

pub mod fallback;
pub mod history;
pub mod mesh_telemetry;
pub mod repair;

// ═══════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::XavierSettings;

    #[test]
    fn test_health_registry_init() {
        // If called after other tests already initialized the singleton,
        // init_health() just returns the existing registry — that's fine.
        let reg = init_health();
        // Registry should always be readable
        let read = reg.try_read();
        assert!(read.is_ok(), "registry should always be readable");
    }

    #[test]
    fn test_health_registry_singleton() {
        let r1 = init_health();
        let r2 = init_health();
        assert!(Arc::ptr_eq(&r1, &r2));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_collect_health_returns_valid_structure() {
        let settings = XavierSettings::default();
        let health = collect_health(&settings, None).await;
        assert!(!health.version.is_empty());
        assert!(
            health.status == "healthy"
                || health.status == "warn"
                || health.status == "degraded"
                || health.status == "unhealthy"
        );
        // uptime can be 0 in test environments where no real clock has elapsed
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_health_check_disk_pass() {
        let settings = XavierSettings::default();
        let health = collect_health(&settings, None).await;
        let disk_check = health.checks.iter().find(|c| c.name == "disk_space");
        assert!(disk_check.is_some());
    }

    #[test]
    fn test_integrity_check_on_dummy_db() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t (x); INSERT INTO t VALUES (1);")
            .unwrap();
        let result = run_integrity_check(&conn);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "ok");
    }

    #[test]
    fn test_db_page_stats() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t (x); INSERT INTO t VALUES (1);")
            .unwrap();
        let (count, size) = get_db_page_stats(&conn);
        assert!(count > 0);
        assert!(size == 4096 || size > 0);
    }

    #[test]
    fn test_auto_vacuum_no_op_on_healthy_db() {
        let settings = XavierSettings::default();
        // A fresh in-memory DB won't need vacuum
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let conn = rusqlite::Connection::open_in_memory().unwrap();
            conn.execute_batch("CREATE TABLE t (x); INSERT INTO t VALUES (1);")
                .unwrap();
            let result = auto_vacuum_if_needed(&conn, &settings).await;
            assert!(result.is_ok());
        });
    }

    #[test]
    fn test_conn_fragmentation_zero_on_fresh_db() {
        // A freshly created in-memory DB has no free pages, so fragmentation is 0.
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t (x INTEGER PRIMARY KEY); INSERT INTO t VALUES (1);")
            .unwrap();
        assert!(
            conn_fragmentation_pct(&conn) < 1.0,
            "fresh DB should have ~0% fragmentation"
        );
    }

    #[test]
    fn test_conn_fragmentation_after_deletes() {
        // Create bloat: insert many rows, then delete most of them. SQLite
        // moves the freed pages to its freelist, raising the fragmentation ratio.
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE t (id INTEGER PRIMARY KEY, payload TEXT);
             BEGIN;
             INSERT INTO t (payload) VALUES (randomblob(4096));
             INSERT INTO t (payload) VALUES (randomblob(4096));
             INSERT INTO t (payload) VALUES (randomblob(4096));
             INSERT INTO t (payload) VALUES (randomblob(4096));
             INSERT INTO t (payload) VALUES (randomblob(4096));
             COMMIT;",
        )
        .unwrap();
        // Force pages onto the freelist.
        conn.execute("DELETE FROM t WHERE id > 1;", []).unwrap();
        // Fragmentation may or may not be high depending on auto-vacuum state,
        // but the function must not panic and must return a sane value.
        let frag = conn_fragmentation_pct(&conn);
        assert!(
            (0.0..=100.0).contains(&frag),
            "fragmentation out of range: {frag}"
        );
    }

    #[test]
    fn test_auto_vacuum_runs_on_fragmented_connection() {
        // Build a connection whose freelist ratio exceeds the 30% threshold by
        // inserting many ~page-sized blobs then deleting most of them. The
        // auto-vacuum should fire and the resulting freelist should shrink.
        let settings = XavierSettings::default();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let conn = rusqlite::Connection::open_in_memory().unwrap();
            conn.execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, payload BLOB);
                 BEGIN;
                 INSERT INTO t (payload) VALUES (randomblob(8192));
                 INSERT INTO t (payload) SELECT randomblob(8192) FROM t;
                 INSERT INTO t (payload) SELECT randomblob(8192) FROM t;
                 INSERT INTO t (payload) SELECT randomblob(8192) FROM t;
                 INSERT INTO t (payload) SELECT randomblob(8192) FROM t;
                 COMMIT;",
            )
            .unwrap();
            // Delete all but one row to push pages onto the freelist.
            conn.execute("DELETE FROM t WHERE id > 1;", []).unwrap();

            let result = auto_vacuum_if_needed(&conn, &settings).await;
            assert!(
                result.is_ok(),
                "auto_vacuum should succeed on a fragmented db"
            );
            // After VACUUM the freelist should be empty (0% fragmentation).
            assert_eq!(
                conn_fragmentation_pct(&conn),
                0.0,
                "freelist should be fully reclaimed after VACUUM"
            );
        });
    }

    #[test]
    fn test_auto_vacuum_skips_when_below_threshold() {
        // A connection with only live data and no freelist must not trigger a
        // VACUUM (and the call must still succeed).
        let settings = XavierSettings::default();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let conn = rusqlite::Connection::open_in_memory().unwrap();
            conn.execute_batch("CREATE TABLE t (x); INSERT INTO t VALUES (1),(2),(3);")
                .unwrap();
            let frag_before = conn_fragmentation_pct(&conn);
            let result = auto_vacuum_if_needed(&conn, &settings).await;
            assert!(result.is_ok());
            // No bloat to reclaim → fragmentation unchanged at 0%.
            assert_eq!(conn_fragmentation_pct(&conn), frag_before);
            assert_eq!(conn_fragmentation_pct(&conn), 0.0);
        });
    }

    // Serializes tests that mutate the shared SYSTEM_ALERTS global (parallel
    // test threads race on clear()/get_alerts() otherwise — flaky).
    static ALERTS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_embedding_alert_on_unhealthy_provider() {
        let _guard = ALERTS_LOCK.lock().unwrap();
        // An unhealthy embedding (disconnected) should push a WARN alert.
        crate::server::alerts::SYSTEM_ALERTS.clear();
        let embedding = EmbeddingHealth {
            provider: "openai".to_string(),
            connected: false,
            latency_ms: 0.0,
            error_rate_pct: 0.0,
            last_success: None,
            fallback_success: false,
        };
        let pushed = push_embedding_alert_if_unhealthy(&embedding);
        assert!(pushed, "disconnected embedding should trigger an alert");
        let alerts = crate::server::alerts::SYSTEM_ALERTS.get_alerts();
        let ours = alerts
            .iter()
            .find(|a| a.component == "embedding")
            .expect("alert should be registered under 'embedding' component");
        assert_eq!(ours.level, "WARN");
        assert!(ours.message.contains("openai"));
    }

    #[test]
    fn test_embedding_alert_on_high_error_rate() {
        let _guard = ALERTS_LOCK.lock().unwrap();
        // A connected but very flaky provider (>10% errors) should also alert.
        crate::server::alerts::SYSTEM_ALERTS.clear();
        let embedding = EmbeddingHealth {
            provider: "ollama".to_string(),
            connected: true,
            latency_ms: 200.0,
            error_rate_pct: 42.0,
            last_success: None,
            fallback_success: false,
        };
        assert!(push_embedding_alert_if_unhealthy(&embedding));
        assert!(
            crate::server::alerts::SYSTEM_ALERTS
                .get_alerts()
                .iter()
                .any(|a| a.message.contains("42")),
            "alert should mention the error rate"
        );
    }

    #[test]
    fn test_no_embedding_alert_when_healthy() {
        let _guard = ALERTS_LOCK.lock().unwrap();
        // A healthy embedding must NOT push an alert.
        crate::server::alerts::SYSTEM_ALERTS.clear();
        let embedding = EmbeddingHealth {
            provider: "openai".to_string(),
            connected: true,
            latency_ms: 120.0,
            error_rate_pct: 0.0,
            last_success: Some(0),
            fallback_success: false,
        };
        let pushed = push_embedding_alert_if_unhealthy(&embedding);
        assert!(!pushed, "healthy embedding should not trigger an alert");
        assert!(
            crate::server::alerts::SYSTEM_ALERTS
                .get_alerts()
                .iter()
                .all(|a| a.component != "embedding"),
            "no embedding alert should exist"
        );
    }

    #[test]
    fn test_embedding_fallback_success_surfaces_as_degraded() {
        let embedding = EmbeddingHealth {
            provider: "nomic".to_string(),
            connected: true,
            latency_ms: 150.0,
            error_rate_pct: 0.0,
            last_success: Some(100),
            fallback_success: true,
        };
        assert!(!push_embedding_alert_if_unhealthy(&embedding));
        assert_eq!(
            fallback::eval_fallback_status(false, embedding.fallback_success),
            "degraded"
        );
    }

    fn prioritize_status(checks: &[HealthCheck]) -> &'static str {
        let critical_failure = checks.iter().any(|c| {
            matches!(c.status, CheckStatus::Fail)
                && (c.name == "disk_space"
                    || c.name == "memory"
                    || c.name == "database_integrity"
                    || c.name == "sqlite_integrity")
        });
        if critical_failure {
            "unhealthy"
        } else if checks.iter().any(|c| matches!(c.status, CheckStatus::Fail)) {
            "degraded"
        } else if checks.iter().any(|c| matches!(c.status, CheckStatus::Warn)) {
            "warn"
        } else {
            "healthy"
        }
    }

    #[tokio::test]
    async fn test_overall_status_prioritization() {
        // Hermetic: do not require the host collect_health() baseline to be perfectly healthy.
        let mut checks = vec![HealthCheck {
            name: "disk_space".into(),
            status: CheckStatus::Pass,
            detail: "ok".into(),
            timestamp_secs: 0,
        }];
        assert_eq!(prioritize_status(&checks), "healthy");

        checks.push(HealthCheck {
            name: "embedding".into(),
            status: CheckStatus::Fail,
            detail: "forced failure".into(),
            timestamp_secs: 0,
        });
        assert_eq!(prioritize_status(&checks), "degraded");

        checks.push(HealthCheck {
            name: "database_integrity".into(),
            status: CheckStatus::Fail,
            detail: "forced failure".into(),
            timestamp_secs: 0,
        });
        assert_eq!(prioritize_status(&checks), "unhealthy");

        // Warn without Fail → warn
        let warn_only = vec![HealthCheck {
            name: "embedding".into(),
            status: CheckStatus::Warn,
            detail: "slow".into(),
            timestamp_secs: 0,
        }];
        assert_eq!(prioritize_status(&warn_only), "warn");
    }

    #[test]
    fn test_component_dependency_graph_status_propagation() {
        let graph = ComponentDependencyGraph::build(vec![
            ("database", "degraded", 1.5, vec![]),
            ("embedding", "healthy", 2.0, vec!["database"]),
            ("telegram", "healthy", 0.5, vec!["embedding", "database"]),
        ]);

        let db_node = graph
            .nodes
            .iter()
            .find(|n| n.component == "database")
            .unwrap();
        assert_eq!(db_node.status, "degraded");
        assert_eq!(db_node.propagated_status, "degraded");

        let emb_node = graph
            .nodes
            .iter()
            .find(|n| n.component == "embedding")
            .unwrap();
        assert_eq!(emb_node.status, "healthy");
        assert_eq!(emb_node.propagated_status, "degraded");

        let tg_node = graph
            .nodes
            .iter()
            .find(|n| n.component == "telegram")
            .unwrap();
        assert_eq!(tg_node.status, "healthy");
        assert_eq!(tg_node.propagated_status, "degraded");
    }

    #[tokio::test]
    async fn test_integrity_check_failure_surfaces_as_unhealthy() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();

        let check_res = run_integrity_check(&conn).unwrap();
        assert_eq!(check_res, "ok");

        // Verify prioritized status returns unhealthy on sqlite_integrity Fail
        let failed_checks = vec![HealthCheck {
            name: "sqlite_integrity".into(),
            status: CheckStatus::Fail,
            detail: "PRAGMA integrity_check: file is corrupted".into(),
            timestamp_secs: 0,
        }];
        assert_eq!(prioritize_status(&failed_checks), "unhealthy");

        let warn_checks = vec![HealthCheck {
            name: "database_integrity".into(),
            status: CheckStatus::Warn,
            detail: "Database fragmentation at 40% — VACUUM recommended".into(),
            timestamp_secs: 0,
        }];
        assert_eq!(prioritize_status(&warn_checks), "warn");
    }
}
