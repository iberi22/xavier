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
use crate::health::repair::{should_retry_peer, PeerRetryDecision};
use crate::embedding::Embedder;
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
        let integrity_ok;
        let mut fragmentation_percent = 0.0;
        let mut page_count = 0;

        let config = crate::memory::sqlite_vec_store::VecSqliteStoreConfig::from_env();
        let project_id = crate::memory::sqlite_vec_store::project_id_for_path(&config.path);

        let res = self
            .cm
            .with_conn(&project_id, |conn| {
                let integrity: String =
                    conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
                let pc: u32 = conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
                let fc: u32 = conn.query_row("PRAGMA freelist_count", [], |r| r.get(0))?;

                let frag = if pc > 0 {
                    (fc as f32 / pc as f32) * 100.0
                } else {
                    0.0
                };

                Ok((integrity == "ok", frag, pc))
            })
            .await;

        if let Ok((ok, frag, pc)) = res {
            integrity_ok = ok;
            fragmentation_percent = frag;
            page_count = pc;
        } else {
            integrity_ok = false;
        }

        let mut wal_size_bytes = 0;
        let wal_path = config.path.clone();
        let mut os_str = wal_path.into_os_string();
        os_str.push("-wal");
        let wal_path = std::path::PathBuf::from(os_str);
        if let Ok(metadata) = std::fs::metadata(wal_path) {
            wal_size_bytes = metadata.len();
        }

        let mut status = HealthLevel::Healthy;
        if !integrity_ok || fragmentation_percent > 60.0 || wal_size_bytes > 1024 * 1024 * 1024 {
            status = HealthLevel::Unhealthy;
        } else if fragmentation_percent > 30.0 || wal_size_bytes > 256 * 1024 * 1024 {
            status = HealthLevel::Degraded;
        }

        DbHealth {
            integrity_ok,
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
                // Ollama version endpoint or just the base
                let check_url = if url.contains("11434") {
                    format!("{}/api/version", url.trim_end_matches("/v1"))
                } else {
                    url.clone()
                };

                match self.http_client.get(&check_url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        reachable = true;
                        let mut fail_count = self.llm_failure_count.write().await;
                        *fail_count = 0;
                    }
                    _ => {
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

        // First try the in-memory peer registry set on health monitor
        let reg_opt = self.peer_registry.read().await;
        let loaded_registry = if reg_opt.is_none() {
            PeerRegistry::load().ok()
        } else {
            None
        };

        let peers: Vec<&crate::mesh::PeerInfo> = if let Some(ref registry) = *reg_opt {
            registry.list_peers()
        } else if let Some(ref registry) = loaded_registry {
            registry.list_peers()
        } else {
            vec![]
        };

        let active_peers = peers.len();

        for peer in peers {
            let last_seen = peer.last_seen_at.unwrap_or(0);
            let now = chrono::Utc::now().timestamp();
            let lag = (now - last_seen).max(0) as u64;

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

#[cfg(test)]
mod tests {
    use super::*;

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
