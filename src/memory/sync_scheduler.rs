//! Automatic background bidirectional memory sync scheduler.
//!
//! Periodically triggers bidirectional incremental sync between local memory store
//! and remote cloud memory store using `CloudMemorySync`.

use std::sync::Arc;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::memory::cloud_sync::{CloudMemorySync, SyncReport};
use crate::memory::store::MemoryStore;

// ---------------------------------------------------------------------------
// SyncSchedulerConfig
// ---------------------------------------------------------------------------

/// Configuration for the background sync scheduler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncSchedulerConfig {
    /// Interval in seconds between sync cycles (default 300s).
    #[serde(default = "default_sync_interval_secs")]
    pub sync_interval_secs: u64,
    /// Whether background auto-sync is enabled (default true).
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Specific workspaces to sync. If empty, all discovered workspaces are synced.
    #[serde(default)]
    pub workspaces: Vec<String>,
}

fn default_sync_interval_secs() -> u64 {
    300
}

fn default_enabled() -> bool {
    true
}

impl Default for SyncSchedulerConfig {
    fn default() -> Self {
        Self {
            sync_interval_secs: default_sync_interval_secs(),
            enabled: default_enabled(),
            workspaces: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// SyncStatus
// ---------------------------------------------------------------------------

/// Current telemetry status and health metrics of the memory sync scheduler.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncStatus {
    /// Timestamp of the last attempted sync cycle.
    pub last_sync_time: Option<DateTime<Utc>>,
    /// Whether the last sync cycle completed successfully without errors.
    pub last_sync_success: bool,
    /// Error message from the last failed sync cycle, if any.
    pub last_error: Option<String>,
    /// Number of consecutive failed sync attempts.
    pub consecutive_failures: u32,
    /// Total number of sync cycles executed.
    pub total_sync_runs: u64,
    /// Cumulative count of records pushed to cloud across all cycles.
    pub total_pushed: usize,
    /// Cumulative count of records pulled from cloud across all cycles.
    pub total_pulled: usize,
    /// Cumulative count of conflicts resolved across all cycles.
    pub total_conflicts: usize,
    /// Detailed reports from the most recent sync cycle.
    pub last_reports: Vec<SyncReport>,
}

// ---------------------------------------------------------------------------
// SyncScheduler
// ---------------------------------------------------------------------------

/// Coordinates periodic background memory synchronization.
#[derive(Clone)]
pub struct SyncScheduler {
    /// Bidirectional cloud sync engine.
    sync_engine: Arc<CloudMemorySync>,
    /// Local memory store (e.g. SQLite).
    local_store: Arc<dyn MemoryStore>,
    /// Scheduler configuration.
    config: SyncSchedulerConfig,
    /// Health status and telemetry metrics.
    status: Arc<tokio::sync::RwLock<SyncStatus>>,
    /// Token used to signal cancellation on shutdown.
    cancel_token: CancellationToken,
    /// Handle to the spawned background worker task.
    worker_handle: Arc<tokio::sync::Mutex<Option<JoinHandle<()>>>>,
}

impl SyncScheduler {
    /// Create a new `SyncScheduler`.
    #[must_use]
    pub fn new(
        sync_engine: Arc<CloudMemorySync>,
        local_store: Arc<dyn MemoryStore>,
        config: SyncSchedulerConfig,
    ) -> Self {
        Self {
            sync_engine,
            local_store,
            config,
            status: Arc::new(tokio::sync::RwLock::new(SyncStatus::default())),
            cancel_token: CancellationToken::new(),
            worker_handle: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Access scheduler configuration.
    #[must_use]
    pub fn config(&self) -> &SyncSchedulerConfig {
        &self.config
    }

    /// Fetch a snapshot of current telemetry and health status.
    pub async fn status(&self) -> SyncStatus {
        self.status.read().await.clone()
    }

    /// Check if the background worker task is actively running.
    pub async fn is_running(&self) -> bool {
        let guard = self.worker_handle.lock().await;
        if let Some(ref handle) = *guard {
            !handle.is_finished()
        } else {
            false
        }
    }

    /// Start the background sync worker loop.
    ///
    /// Periodically executes `run_sync_cycle` every `sync_interval_secs`.
    pub async fn start(&self) {
        let mut guard = self.worker_handle.lock().await;

        if let Some(ref handle) = *guard {
            if !handle.is_finished() {
                tracing::info!("SyncScheduler worker is already running.");
                return;
            }
        }

        let scheduler = self.clone();
        let cancel_token = self.cancel_token.clone();
        let interval_secs = scheduler.config.sync_interval_secs.max(1);

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            interval.tick().await;

            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        tracing::info!("SyncScheduler worker terminating on cancellation signal.");
                        break;
                    }
                    _ = interval.tick() => {
                        if !scheduler.config.enabled {
                            continue;
                        }
                        if let Err(err) = scheduler.run_sync_cycle().await {
                            tracing::warn!("SyncScheduler cycle error: {:#}", err);
                        }
                    }
                }
            }
        });

        *guard = Some(handle);
    }

    /// Force execution of a single sync cycle immediately.
    ///
    /// Updates telemetry status and cumulative metrics.
    pub async fn run_sync_cycle(&self) -> Result<Vec<SyncReport>> {
        let reports_result = if self.config.workspaces.is_empty() {
            self.sync_engine
                .sync_all_workspaces(self.local_store.as_ref())
                .await
        } else {
            let mut reports = Vec::new();
            for ws in &self.config.workspaces {
                let report = self
                    .sync_engine
                    .sync_all(self.local_store.as_ref(), ws)
                    .await?;
                reports.push(report);
            }
            Ok(reports)
        };

        let mut status = self.status.write().await;
        status.total_sync_runs += 1;
        status.last_sync_time = Some(Utc::now());

        match reports_result {
            Ok(reports) => {
                let mut cycle_pushed = 0;
                let mut cycle_pulled = 0;
                let mut cycle_conflicts = 0;
                let mut cycle_success = true;

                for r in &reports {
                    cycle_pushed += r.pushed;
                    cycle_pulled += r.pulled;
                    cycle_conflicts += r.conflicts;
                    if !r.success {
                        cycle_success = false;
                    }
                }

                status.last_sync_success = cycle_success;
                if cycle_success {
                    status.last_error = None;
                    status.consecutive_failures = 0;
                } else {
                    status.consecutive_failures += 1;
                    let err_msg = reports
                        .iter()
                        .filter_map(|r| r.error.clone())
                        .collect::<Vec<_>>()
                        .join("; ");
                    status.last_error = Some(if err_msg.is_empty() {
                        "Workspace sync failed".to_string()
                    } else {
                        err_msg
                    });
                }

                status.total_pushed += cycle_pushed;
                status.total_pulled += cycle_pulled;
                status.total_conflicts += cycle_conflicts;
                status.last_reports = reports.clone();

                Ok(reports)
            }
            Err(err) => {
                let err_str = format!("{:#}", err);
                status.last_sync_success = false;
                status.last_error = Some(err_str.clone());
                status.consecutive_failures += 1;
                Err(err)
            }
        }
    }

    /// Stop the background worker loop gracefully and wait for completion.
    pub async fn stop(&self) -> Result<()> {
        self.cancel_token.cancel();
        let mut guard = self.worker_handle.lock().await;
        if let Some(handle) = guard.take() {
            let _ = handle.await;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::TempDir;
    use crate::checkpoint::Checkpoint;
    use crate::domain::memory::belief::BeliefEdge;
    use crate::memory::cloud_sync::CloudSyncConfig;
    use crate::memory::schema::{MemoryLevel, MemoryQueryFilters};
    use crate::memory::store::{DurableWorkspaceState, MemoryBackend, MemoryRecord, SessionTokenRecord};
    use crate::security::clearance::ClearanceLevel;

    #[derive(Default)]
    struct MockStore {
        records: Mutex<Vec<MemoryRecord>>,
        should_fail: Mutex<bool>,
    }

    #[async_trait::async_trait]
    impl MemoryStore for MockStore {
        fn backend(&self) -> MemoryBackend {
            MemoryBackend::Memory
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        async fn health(&self) -> Result<String> {
            if *self.should_fail.lock().unwrap() {
                anyhow::bail!("Database connection failed");
            }
            Ok("ok".to_string())
        }

        async fn get(&self, workspace_id: &str, id: &str) -> Result<Option<MemoryRecord>> {
            if *self.should_fail.lock().unwrap() {
                anyhow::bail!("Database connection failed");
            }
            let store = self.records.lock().unwrap();
            Ok(store.iter().find(|r| r.workspace_id == workspace_id && r.id == id).cloned())
        }

        async fn put(&self, record: MemoryRecord) -> Result<()> {
            if *self.should_fail.lock().unwrap() {
                anyhow::bail!("Database connection failed");
            }
            let mut store = self.records.lock().unwrap();
            if let Some(pos) = store.iter().position(|r| r.id == record.id && r.workspace_id == record.workspace_id) {
                store[pos] = record;
            } else {
                store.push(record);
            }
            Ok(())
        }

        async fn update(&self, record: MemoryRecord) -> Result<()> {
            self.put(record).await
        }

        async fn delete(&self, workspace_id: &str, id: &str) -> Result<Option<MemoryRecord>> {
            let mut store = self.records.lock().unwrap();
            if let Some(pos) = store.iter().position(|r| r.workspace_id == workspace_id && r.id == id) {
                Ok(Some(store.remove(pos)))
            } else {
                Ok(None)
            }
        }

        async fn list(&self, workspace_id: &str) -> Result<Vec<MemoryRecord>> {
            if *self.should_fail.lock().unwrap() {
                anyhow::bail!("Database connection failed");
            }
            let store = self.records.lock().unwrap();
            if workspace_id.is_empty() {
                Ok(store.clone())
            } else {
                Ok(store.iter().filter(|r| r.workspace_id == workspace_id).cloned().collect())
            }
        }

        async fn search(
            &self,
            workspace_id: &str,
            _query: &str,
            _filters: Option<&MemoryQueryFilters>,
        ) -> Result<Vec<MemoryRecord>> {
            self.list(workspace_id).await
        }

        async fn load_workspace_state(&self, _workspace_id: &str) -> Result<DurableWorkspaceState> {
            Ok(DurableWorkspaceState::default())
        }

        async fn save_beliefs(&self, _workspace_id: &str, _beliefs: Vec<BeliefEdge>) -> Result<()> {
            Ok(())
        }

        async fn save_session_token(&self, _workspace_id: &str, _token: SessionTokenRecord) -> Result<()> {
            Ok(())
        }

        async fn is_session_token_valid(&self, _workspace_id: &str, _token: &str) -> Result<bool> {
            Ok(true)
        }

        async fn save_checkpoint(&self, _workspace_id: &str, _checkpoint: Checkpoint) -> Result<()> {
            Ok(())
        }

        async fn load_checkpoint(&self, _workspace_id: &str, _task_id: &str, _name: &str) -> Result<Option<Checkpoint>> {
            Ok(None)
        }

        async fn list_checkpoints(&self, _workspace_id: &str, _task_id: &str) -> Result<Vec<Checkpoint>> {
            Ok(Vec::new())
        }

        async fn delete_checkpoint(&self, _workspace_id: &str, _task_id: &str, _name: &str) -> Result<()> {
            Ok(())
        }
    }

    fn make_test_record(id: &str, workspace_id: &str, content: &str) -> MemoryRecord {
        let now = Utc::now();
        MemoryRecord {
            id: id.to_string(),
            workspace_id: workspace_id.to_string(),
            path: format!("test/{}", id),
            content: content.to_string(),
            metadata: serde_json::Value::Object(serde_json::Map::new()),
            embedding: Vec::new(),
            created_at: now,
            updated_at: now,
            revision: 1,
            primary: true,
            parent_id: None,
            cluster_id: None,
            level: MemoryLevel::Raw,
            relation: None,
            clearance: ClearanceLevel::Unclassified,
            revisions: Vec::new(),
            encrypted_dek: None,
            content_iv: None,
            metadata_iv: None,
            score: 0.0,
            deleted_at: None,
            ..Default::default()
        }
    }

    async fn create_test_scheduler() -> (SyncScheduler, Arc<MockStore>, Arc<MockStore>, TempDir) {
        let local_store = Arc::new(MockStore::default());
        let cloud_store = Arc::new(MockStore::default());

        let tmp_dir = TempDir::new().unwrap();
        let cloud_config = CloudSyncConfig {
            data_dir: tmp_dir.path().to_string_lossy().to_string(),
            node_id: Some("test_node".to_string()),
            ..Default::default()
        };

        let sync_engine = Arc::new(
            CloudMemorySync::new(cloud_store.clone() as Arc<dyn MemoryStore>, cloud_config)
                .await
                .unwrap(),
        );

        let scheduler_config = SyncSchedulerConfig {
            sync_interval_secs: 1,
            enabled: true,
            workspaces: Vec::new(),
        };

        let scheduler = SyncScheduler::new(
            sync_engine,
            local_store.clone() as Arc<dyn MemoryStore>,
            scheduler_config,
        );

        (scheduler, local_store, cloud_store, tmp_dir)
    }

    #[tokio::test]
    async fn test_default_config() {
        let config = SyncSchedulerConfig::default();
        assert_eq!(config.sync_interval_secs, 300);
        assert!(config.enabled);
        assert!(config.workspaces.is_empty());
    }

    #[tokio::test]
    async fn test_single_sync_cycle_execution() {
        let (scheduler, local_store, _cloud_store, _tmp) = create_test_scheduler().await;

        local_store
            .put(make_test_record("r1", "ws1", "local memory content"))
            .await
            .unwrap();

        let reports = scheduler.run_sync_cycle().await.unwrap();
        assert!(!reports.is_empty());

        let status = scheduler.status().await;
        assert_eq!(status.total_sync_runs, 1);
        assert!(status.last_sync_success);
        assert_eq!(status.total_pushed, 1);
        assert_eq!(status.consecutive_failures, 0);
        assert!(status.last_sync_time.is_some());
    }

    #[tokio::test]
    async fn test_background_worker_start_and_stop() {
        let (scheduler, _local, _cloud, _tmp) = create_test_scheduler().await;

        assert!(!scheduler.is_running().await);
        scheduler.start().await;

        assert!(scheduler.is_running().await);

        scheduler.stop().await.unwrap();
        assert!(!scheduler.is_running().await);
    }

    #[tokio::test]
    async fn test_sync_failure_telemetry_tracking() {
        let (scheduler, local_store, _cloud, _tmp) = create_test_scheduler().await;

        // Force local store failure
        *local_store.should_fail.lock().unwrap() = true;

        let result = scheduler.run_sync_cycle().await;
        assert!(result.is_err());

        let status = scheduler.status().await;
        assert_eq!(status.total_sync_runs, 1);
        assert!(!status.last_sync_success);
        assert_eq!(status.consecutive_failures, 1);
        assert!(status.last_error.is_some());
    }
}
