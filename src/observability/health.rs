//! # Health Monitoring
//!
//! Native runtime health loop that monitors system resources, database integrity,
//! embedding providers, and mesh peers.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::codebase::connection_manager::ConnectionManager;
use crate::embedding::Embedder;
use crate::health::repair::{should_retry_peer, PeerRetryDecision};
use crate::mesh::PeerRegistry;
use crate::notifications::{IslandId, NOTIFICATIONS};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum HealthLevel {
    Healthy,
    Degraded,
    Unhealthy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmHealth {
    pub provider: String,
    pub model: String,
    pub endpoint: String,
    pub reachable: bool,
    pub status: HealthLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorDbHealth {
    pub backend: String,
    pub path: String,
    pub status: HealthLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemHealth {
    pub cpu_usage: f32,
    pub ram_usage_percent: f32,
    pub disk_usage_percent: f32,
    pub uptime_secs: u64,
    pub status: HealthLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbHealth {
    pub integrity_ok: bool,
    /// `false` = no se pudo EJECUTAR la comprobacion (pool ocupado, timeout...). Distinto de
    /// "corrupta": antes cualquier error se reportaba como `integrity_ok: false` y el nodo entero
    /// pasaba a `unhealthy` sin que la base tuviera nada.
    #[serde(default)]
    pub integrity_verified: bool,
    pub fragmentation_percent: f32,
    pub wal_size_bytes: u64,
    pub page_count: u32,
    pub status: HealthLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingHealth {
    pub provider: String,
    pub model: String,
    pub latency_ms: u64,
    pub error_rate: f32,
    pub status: HealthLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<crate::embedding::cache::EmbeddingCacheMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerHealth {
    pub node_id: String,
    pub connectivity_ok: bool,
    pub sync_lag_secs: u64,
    pub trust_score: f32,
    #[serde(default)]
    pub is_stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshHealth {
    pub active_peers: usize,
    pub peers: Vec<PeerHealth>,
    pub status: HealthLevel,
    #[serde(default)]
    pub maturity: crate::mesh::MeshMaturityReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub timestamp: DateTime<Utc>,
    pub status: HealthLevel,
    pub mode: crate::server::alerts::OperationalMode,
    pub system: SystemHealth,
    pub database: DbHealth,
    pub embedding: EmbeddingHealth,
    pub llm: LlmHealth,
    pub vector_db: VectorDbHealth,
    pub mesh: MeshHealth,
    pub tgd_consolidation: Option<crate::tgd::consolidation::ProgressReport>,
}

impl Default for HealthStatus {
    fn default() -> Self {
        Self {
            timestamp: Utc::now(),
            status: HealthLevel::Healthy,
            mode: crate::server::alerts::OperationalMode::LocalHealthy,
            system: SystemHealth {
                cpu_usage: 0.0,
                ram_usage_percent: 0.0,
                disk_usage_percent: 0.0,
                uptime_secs: 0,
                status: HealthLevel::Healthy,
            },
            database: DbHealth {
                integrity_ok: true,
                integrity_verified: false,
                fragmentation_percent: 0.0,
                wal_size_bytes: 0,
                page_count: 0,
                status: HealthLevel::Healthy,
            },
            embedding: EmbeddingHealth {
                provider: "unknown".into(),
                model: "unknown".into(),
                latency_ms: 0,
                error_rate: 0.0,
                status: HealthLevel::Healthy,
                cache: None,
            },
            llm: LlmHealth {
                provider: "unknown".into(),
                model: "unknown".into(),
                endpoint: "unknown".into(),
                reachable: false,
                status: HealthLevel::Healthy,
            },
            vector_db: VectorDbHealth {
                backend: "unknown".into(),
                path: "unknown".into(),
                status: HealthLevel::Healthy,
            },
            mesh: MeshHealth {
                active_peers: 0,
                peers: vec![],
                status: HealthLevel::Healthy,
                maturity: crate::mesh::MeshMaturityReport::default(),
            },
            tgd_consolidation: None,
        }
    }
}

pub struct HealthMonitor {
    current_status: Arc<RwLock<HealthStatus>>,
    cm: &'static ConnectionManager,
    peer_registry: Arc<RwLock<Option<Arc<PeerRegistry>>>>,
    embedder: Arc<RwLock<Option<Arc<dyn Embedder>>>>,
    tgd_progress: Arc<RwLock<Option<Arc<RwLock<crate::tgd::consolidation::ProgressReport>>>>>,
    llm_failure_count: Arc<RwLock<u32>>,
    peer_attempts: Arc<RwLock<HashMap<String, u64>>>,
    http_client: reqwest::Client,
}

/// Decide el estado de la base a partir de lo observado.
///
/// Regla que importa: **no poder verificar no es estar corrupta**. Un `integrity_check` que no se
/// puede ejecutar (pool ocupado, timeout) antes ponia el nodo entero en `Unhealthy` y ademas
/// escondia la corrupcion real. Ahora:
///   * verificada y ok                        -> Healthy (o Degraded por fragmentacion/WAL)
///   * verificada y NO ok (corrupcion real)   -> Unhealthy
///   * no verificada                          -> Degraded, nunca Unhealthy
fn db_status_from(
    verified: bool,
    integrity_ok: bool,
    fragmentation_percent: f32,
    wal_size_bytes: u64,
) -> HealthLevel {
    let mut status = if !verified {
        HealthLevel::Degraded
    } else if !integrity_ok {
        HealthLevel::Unhealthy
    } else {
        HealthLevel::Healthy
    };

    if fragmentation_percent > 60.0 || wal_size_bytes > 1024 * 1024 * 1024 {
        status = HealthLevel::Unhealthy;
    } else if status == HealthLevel::Healthy
        && (fragmentation_percent > 30.0 || wal_size_bytes > 256 * 1024 * 1024)
    {
        status = HealthLevel::Degraded;
    }
    status
}

impl HealthMonitor {
    /// New.
    pub fn new(cm: &'static ConnectionManager) -> Self {
        Self {
            current_status: Arc::new(RwLock::new(HealthStatus::default())),
            cm,
            peer_registry: Arc::new(RwLock::new(None)),
            embedder: Arc::new(RwLock::new(None)),
            tgd_progress: Arc::new(RwLock::new(None)),
            llm_failure_count: Arc::new(RwLock::new(0)),
            peer_attempts: Arc::new(RwLock::new(HashMap::new())),
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Set tgd progress.
    pub async fn set_tgd_progress(
        &self,
        progress: Arc<RwLock<crate::tgd::consolidation::ProgressReport>>,
    ) {
        let mut prg = self.tgd_progress.write().await;
        *prg = Some(progress);
    }

    /// Set peer registry.
    pub async fn set_peer_registry(&self, peer_registry: Arc<PeerRegistry>) {
        let mut reg = self.peer_registry.write().await;
        *reg = Some(peer_registry);
    }

    /// Set embedder.
    pub async fn set_embedder(&self, embedder: Arc<dyn Embedder>) {
        let mut emb = self.embedder.write().await;
        *emb = Some(embedder);
    }

    /// Get status.
    pub async fn get_status(&self) -> HealthStatus {
        self.current_status.read().await.clone()
    }

    /// Run checks.
    pub async fn run_checks(&self) -> HealthStatus {
        let mut sys_info = sysinfo::System::new_all();

        let system = self.check_system(&mut sys_info).await;
        let database = self.check_database().await;
        let embedding = self.check_embedding().await;
        let llm = self.check_llm().await;
        let vector_db = self.check_vector_db().await;
        let mesh = self.check_mesh().await;
        let tgd_consolidation = self.check_tgd_progress().await;

        let mut status = HealthLevel::Healthy;
        // Critical failures: system or database. Embedding/LLM failures only degrade the system.
        if system.status == HealthLevel::Unhealthy || database.status == HealthLevel::Unhealthy {
            status = HealthLevel::Unhealthy;
        } else if system.status == HealthLevel::Degraded
            || database.status == HealthLevel::Degraded
            || embedding.status == HealthLevel::Unhealthy
            || embedding.status == HealthLevel::Degraded
            || llm.status == HealthLevel::Unhealthy
            || llm.status == HealthLevel::Degraded
            || mesh.status == HealthLevel::Degraded
        {
            status = HealthLevel::Degraded;
        }

        let mode = crate::server::alerts::SYSTEM_ALERTS.get_mode();

        let new_status = HealthStatus {
            timestamp: Utc::now(),
            status,
            mode,
            system,
            database,
            embedding,
            llm,
            vector_db,
            mesh,
            tgd_consolidation,
        };

        // Notify if status changed
        let previous_level = {
            let current = self.current_status.read().await;
            current.status.clone()
        };

        if new_status.status != previous_level {
            let (title, severity) = match new_status.status {
                HealthLevel::Healthy => ("System Health Restored", "success"),
                HealthLevel::Degraded => ("System Health Degraded", "warning"),
                HealthLevel::Unhealthy => ("System Health Critical", "error"),
            };
            let _ = NOTIFICATIONS
                .notify(
                    IslandId::System,
                    title,
                    &format!("System status is now {:?}", new_status.status),
                    severity,
                )
                .await;
        }

        // Auto-repair actions
        if new_status.database.fragmentation_percent > 30.0 {
            tracing::info!(
                "Auto-repair: High fragmentation ({:.1}%), running VACUUM...",
                new_status.database.fragmentation_percent
            );
            let config = crate::memory::sqlite_vec_store::VecSqliteStoreConfig::from_env();
            let project_id = crate::memory::sqlite_vec_store::project_id_for_path(&config.path);
            let _ = self
                .cm
                .with_conn(&project_id, |conn| {
                    conn.execute("VACUUM", [])?;
                    Ok(())
                })
                .await;
        }

        let auto_repair_enabled = std::env::var("XAVIER_MESH_AUTO_REPAIR")
            .map(|v| v != "0" && v.to_lowercase() != "false")
            .unwrap_or(true);

        if auto_repair_enabled {
            let mut attempts = self.peer_attempts.write().await;
            for peer in &new_status.mesh.peers {
                let attempt = attempts.get(&peer.node_id).copied().unwrap_or(0);
                let decision = should_retry_peer(peer.sync_lag_secs, attempt);

                match decision {
                    PeerRetryDecision::Healthy => {
                        attempts.remove(&peer.node_id);
                    }
                    PeerRetryDecision::RetryImmediately => {
                        attempts.insert(peer.node_id.clone(), attempt + 1);
                        tracing::info!(
                            "Auto-repair: High lag for peer {} ({}s), attempting reconnection hint...",
                            peer.node_id,
                            peer.sync_lag_secs
                        );
                    }
                    PeerRetryDecision::RetryWithBackoff { should_log } => {
                        attempts.insert(peer.node_id.clone(), attempt + 1);
                        if should_log {
                            tracing::info!(
                                "Auto-repair: High lag for peer {} ({}s), attempting reconnection hint...",
                                peer.node_id,
                                peer.sync_lag_secs
                            );
                        }
                    }
                    PeerRetryDecision::Stale => {
                        attempts.insert(peer.node_id.clone(), attempt + 1);
                        tracing::debug!(
                            "Auto-repair: Peer {} is stale (lag {}s > 7 days), skipping reconnection hint",
                            peer.node_id,
                            peer.sync_lag_secs
                        );
                    }
                }
            }
        }

        let mut current = self.current_status.write().await;
        *current = new_status.clone();
        new_status
    }

    async fn check_system(&self, sys: &mut sysinfo::System) -> SystemHealth {
        sys.refresh_cpu_usage();
        sys.refresh_memory();

        let cpu_usage = sys.global_cpu_usage();
        let total_memory = sys.total_memory();
        let used_memory = sys.used_memory();
        let ram_usage_percent = if total_memory > 0 {
            (used_memory as f32 / total_memory as f32) * 100.0
        } else {
            0.0
        };

        let mut disk_usage_percent = 0.0;
        let disks = sysinfo::Disks::new_with_refreshed_list();
        if let Some(main_disk) = disks.iter().next() {
            let total = main_disk.total_space();
            let available = main_disk.available_space();
            if total > 0 {
                disk_usage_percent = ((total - available) as f32 / total as f32) * 100.0;
            }
        }

        let uptime_secs = sysinfo::System::uptime();

        let mut status = HealthLevel::Healthy;
        if cpu_usage > 95.0 || ram_usage_percent > 95.0 || disk_usage_percent > 95.0 {
            status = HealthLevel::Unhealthy;
        } else if cpu_usage > 80.0 || ram_usage_percent > 85.0 || disk_usage_percent > 85.0 {
            status = HealthLevel::Degraded;
        }

        SystemHealth {
            cpu_usage,
            ram_usage_percent,
            disk_usage_percent,
            uptime_secs,
            status,
        }
    }

    async fn check_database(&self) -> DbHealth {
        // El monitor corre cada 60 s: `PRAGMA integrity_check` COMPLETO sobre esta base (991 MB,
        // 253k paginas) cuesta una barbaridad y se ejecutaba en cada vuelta. Se usa `quick_check`,
        // que detecta la corrupcion real a una fraccion del coste.
        let mut integrity_ok = false;
        let mut integrity_verified = false;
        let mut fragmentation_percent = 0.0;
        let mut page_count = 0;

        let config = crate::memory::sqlite_vec_store::VecSqliteStoreConfig::from_env();
        let project_id = crate::memory::sqlite_vec_store::project_id_for_path(&config.path);

        let res = self
            .cm
            .with_conn(&project_id, |conn| {
                let integrity: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
                let pc: u32 = conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
                let fc: u32 = conn.query_row("PRAGMA freelist_count", [], |r| r.get(0))?;

                let frag = if pc > 0 {
                    (fc as f32 / pc as f32) * 100.0
                } else {
                    0.0
                };

                Ok((integrity, frag, pc))
            })
            .await;

        match res {
            Ok((integrity, frag, pc)) => {
                integrity_verified = true;
                integrity_ok = integrity == "ok";
                if !integrity_ok {
                    tracing::error!(detalle = %integrity, "integridad de la base comprometida");
                }
                fragmentation_percent = frag;
                page_count = pc;
            }
            Err(e) => {
                // NO se pudo verificar. Se registra y se marca degradado, no corrupto.
                tracing::warn!(error = %e, "no se pudo verificar la integridad de la base");
            }
        }

        let mut wal_size_bytes = 0;
        let wal_path = config.path.clone();
        let mut os_str = wal_path.into_os_string();
        os_str.push("-wal");
        let wal_path = std::path::PathBuf::from(os_str);
        if let Ok(metadata) = std::fs::metadata(wal_path) {
            wal_size_bytes = metadata.len();
        }

        let status = db_status_from(
            integrity_verified,
            integrity_ok,
            fragmentation_percent,
            wal_size_bytes,
        );

        DbHealth {
            integrity_ok,
            integrity_verified,
            fragmentation_percent,
            wal_size_bytes,
            page_count,
            status,
        }
    }

    async fn check_embedding(&self) -> EmbeddingHealth {
        let provider = std::env::var("XAVIER_EMBEDDER").unwrap_or_else(|_| "openai".into());
        let model = std::env::var("XAVIER_EMBEDDING_MODEL").unwrap_or_else(|_| "unknown".into());
        let mut latency_ms = 0;
        let mut status = HealthLevel::Healthy;

        let embedder_opt = self.embedder.read().await;
        if let Some(ref embedder) = *embedder_opt {
            let start = std::time::Instant::now();
            match embedder.encode("health check ping").await {
                Ok(_) => {
                    latency_ms = start.elapsed().as_millis() as u64;
                    if latency_ms > 3000 {
                        status = HealthLevel::Degraded;
                    }
                }
                Err(_) => {
                    status = HealthLevel::Unhealthy;
                }
            }
        }

        let cache = embedder_opt.as_ref().and_then(|e| e.cache_metrics());

        EmbeddingHealth {
            provider,
            model,
            latency_ms,
            error_rate: 0.0,
            status,
            cache,
        }
    }

    async fn check_llm(&self) -> LlmHealth {
        let config = crate::agents::provider::ModelProviderConfig::from_env();
        let provider = config.provider_label.clone();
        let model = config.model.clone();
        let endpoint = config
            .base_url
            .clone()
            .unwrap_or_else(|| "none".to_string());
        let mut reachable = false;
        let mut status = HealthLevel::Healthy;

        if config.provider_mode == crate::agents::provider::types::ProviderMode::Local
            || config.provider_mode == crate::agents::provider::types::ProviderMode::ManagedLocal
        {
            if let Some(url) = &config.get_resolved_base_url() {
                // No adivinar por el puerto: un Ollama puede vivir en cualquier host:puerto (p.ej.
                // un segundo servidor con sus propios modelos). Antes solo se probaba
                // `/api/version` si la URL contenia "11434"; para cualquier otro puerto se hacia
                // GET a `/v1`, que devuelve 404, y el proveedor local se marcaba como NO
                // alcanzable aunque estuviera perfectamente vivo (el nodo entero pasaba a
                // "unhealthy"). Se prueban los endpoints habituales y basta con que uno responda.
                let trimmed = url.trim_end_matches('/');
                let base = trimmed.trim_end_matches("/v1");
                let candidates = [
                    format!("{}/api/version", base), // Ollama nativo
                    format!("{}/models", trimmed),   // OpenAI-compatible
                ];

                let mut probe_ok = false;
                for check_url in &candidates {
                    if let Ok(resp) = self.http_client.get(check_url).send().await {
                        if resp.status().is_success() {
                            probe_ok = true;
                            break;
                        }
                    }
                }

                if probe_ok {
                    reachable = true;
                    let mut fail_count = self.llm_failure_count.write().await;
                    *fail_count = 0;
                } else {
                    reachable = false;
                    let mut fail_count = self.llm_failure_count.write().await;
                    *fail_count += 1;
                    if *fail_count >= 3 {
                        crate::server::alerts::SYSTEM_ALERTS.push_alert(
                            "ERROR",
                            "Ollama local no responde — modo degradado",
                            "llm",
                        );
                        status = HealthLevel::Unhealthy;
                    } else {
                        status = HealthLevel::Degraded;
                    }
                }
            }
        } else if config.provider_mode == crate::agents::provider::types::ProviderMode::Cloud {
            reachable = true; // Assume cloud is reachable if configured for now
        }

        LlmHealth {
            provider,
            model,
            endpoint,
            reachable,
            status,
        }
    }

    async fn check_vector_db(&self) -> VectorDbHealth {
        let settings = crate::settings::XavierSettings::current();
        let backend = settings.memory.backend.clone();
        let config = crate::memory::sqlite_vec_store::VecSqliteStoreConfig::from_env();
        let path = config.path.to_string_lossy().to_string();

        VectorDbHealth {
            backend,
            path,
            status: HealthLevel::Healthy,
        }
    }

    async fn check_tgd_progress(&self) -> Option<crate::tgd::consolidation::ProgressReport> {
        let prg_opt = self.tgd_progress.read().await;
        if let Some(ref prg) = *prg_opt {
            Some(prg.read().await.clone())
        } else {
            None
        }
    }

    async fn check_mesh(&self) -> MeshHealth {
        let mut peer_healths = vec![];

        // El disco es la FUENTE DE VERDAD: tanto la API HTTP como el CLI (`mesh join`,
        // `mesh add-peer`) persisten ahi. El registro en memoria es un snapshot del arranque
        // (src/cli/server.rs set_peer_registry) y quedaba desactualizado: emparejar por CLI
        // no se reflejaba en /health hasta reiniciar el nodo. Se prefiere el disco.
        let loaded_registry = PeerRegistry::load().ok();
        let reg_opt = self.peer_registry.read().await;

        let peers: Vec<&crate::mesh::PeerInfo> = if let Some(ref registry) = loaded_registry {
            registry.list_peers()
        } else if let Some(ref registry) = *reg_opt {
            registry.list_peers()
        } else {
            vec![]
        };

        let active_peers = peers.len();

        for peer in peers {
            let now = chrono::Utc::now().timestamp();
            let lag = peer_lag_secs(peer, now);

            peer_healths.push(PeerHealth {
                node_id: peer.node_id.to_string(),
                connectivity_ok: lag < 60, // 1 minute threshold for mesh connectivity alert
                sync_lag_secs: lag,
                trust_score: 1.0,
                is_stale: lag > 604800,
            });
        }

        let mut status = HealthLevel::Healthy;
        if peer_healths.iter().any(|p| !p.connectivity_ok) {
            status = HealthLevel::Degraded;
        }

        MeshHealth {
            active_peers,
            peers: peer_healths,
            status,
            maturity: crate::mesh::MeshMaturityReport::default(),
        }
    }

    /// Spawn.
    pub fn spawn(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                let _ = self.run_checks().await;
            }
        });
    }
}

pub static HEALTH: std::sync::LazyLock<Arc<HealthMonitor>> =
    std::sync::LazyLock::new(|| Arc::new(HealthMonitor::new(ConnectionManager::global())));

/// Segundos de desfase de un peer respecto a `now`.
///
/// `last_seen_at` puede ser `None` (peer emparejado pero aun sin handshake verificado): en ese
/// caso se usa `added_at` como referencia. Nunca se usa `0` como sustituto, que producia un lag
/// absurdo (~1.79e9 s) y marcaba el mesh como `degraded` de forma permanente.
fn peer_lag_secs(peer: &crate::mesh::PeerInfo, now: i64) -> u64 {
    let reference = peer.last_seen_at.unwrap_or(peer.added_at);
    if reference > 0 {
        (now - reference).max(0) as u64
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_status_no_verificada_no_es_corrupcion() {
        // El caso que rompia: no se pudo comprobar -> Degraded, jamas Unhealthy.
        assert_eq!(db_status_from(false, false, 1.0, 0), HealthLevel::Degraded);
        // Y una base verificada y sana es Healthy.
        assert_eq!(
            db_status_from(true, true, 1.2, 6_439_592),
            HealthLevel::Healthy
        );
    }

    #[test]
    fn db_status_corrupcion_confirmada_es_unhealthy() {
        assert_eq!(db_status_from(true, false, 1.0, 0), HealthLevel::Unhealthy);
    }

    #[test]
    fn db_status_umbrales_de_fragmentacion_y_wal() {
        assert_eq!(db_status_from(true, true, 45.0, 0), HealthLevel::Degraded);
        assert_eq!(db_status_from(true, true, 70.0, 0), HealthLevel::Unhealthy);
        assert_eq!(
            db_status_from(true, true, 0.0, 300 * 1024 * 1024),
            HealthLevel::Degraded
        );
        assert_eq!(
            db_status_from(true, true, 0.0, 2 * 1024 * 1024 * 1024),
            HealthLevel::Unhealthy
        );
    }

    #[test]
    fn test_peer_lag_secs_uses_added_at_when_never_seen() {
        let mut peer = crate::mesh::PeerInfo {
            node_id: crate::mesh::node::NodeId("xv1-test".into()),
            alias: None,
            endpoint_url: "http://localhost:8006".into(),
            public_key_hex: String::new(),
            added_at: 1_000,
            last_seen_at: None,
            sync_enabled: true,
            is_cloud: false,
            iroh_addr: None,
            shared_workspace_ids: Vec::new(),
            shared_workspace_tokens: std::collections::HashMap::new(),
            capabilities: Vec::new(),
        };
        // Nunca visto: referencia = added_at, NO 0 (que daria lag ~1.79e9 y "degraded" eterno).
        assert_eq!(peer_lag_secs(&peer, 1_060), 60);
        // Recien emparejado: el mesh debe salir sano.
        assert!(peer_lag_secs(&peer, 1_030) < 60);

        // Con last_seen_at presente manda last_seen_at.
        peer.last_seen_at = Some(2_000);
        assert_eq!(peer_lag_secs(&peer, 2_010), 10);

        // added_at 0 (registro incompleto) no produce lag negativo ni absurdo.
        peer.added_at = 0;
        peer.last_seen_at = None;
        assert_eq!(peer_lag_secs(&peer, 5_000), 0);
    }

    #[test]
    fn test_health_status_default() {
        let status = HealthStatus::default();
        assert_eq!(status.status, HealthLevel::Healthy);
        assert_eq!(status.system.status, HealthLevel::Healthy);
        assert_eq!(status.database.status, HealthLevel::Healthy);
    }

    #[tokio::test]
    async fn test_peer_health_stale_flag() {
        let peer_normal = PeerHealth {
            node_id: "node1".into(),
            connectivity_ok: true,
            sync_lag_secs: 100,
            trust_score: 1.0,
            is_stale: false,
        };
        assert!(!peer_normal.is_stale);

        let peer_stale = PeerHealth {
            node_id: "node2".into(),
            connectivity_ok: false,
            sync_lag_secs: 700_000, // > 7 days (604800)
            trust_score: 1.0,
            is_stale: 700_000 > 604800,
        };
        assert!(peer_stale.is_stale);
    }

    #[tokio::test]
    async fn test_health_monitor_initial_state() {
        let cm = ConnectionManager::global();
        let monitor = HealthMonitor::new(cm);
        let status = monitor.get_status().await;
        assert_eq!(status.status, HealthLevel::Healthy);
    }
}
