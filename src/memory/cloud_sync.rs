//! Cloud Memory Sync — bidirectional LWW sync between local and cloud backends.
//!
//! Provides `CloudMemorySync` which orchestrates push/pull/sync-all operations
//! between a local `MemoryStore` (e.g. SQLite) and a remote/cloud `MemoryStore`
//! (e.g. Supabase, Postgres/Neon).
//!
//! ## Conflict Resolution (LWW)
//!
//! 1. Higher `updated_at` timestamp wins.
//! 2. Same timestamp → higher node_id (lexicographic) wins.
//! 3. Batches of up to 100 records per request (pagination for pull).
//!
//! ## Last Sync State
//!
//! Persisted to `last_sync.json` in the data directory so incremental syncs
//! only transfer records modified since the last run.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::memory::store::{MemoryRecord, MemoryStore};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of records in a single batch.
pub const SYNC_BATCH_SIZE: usize = 100;

/// Name of the file where last-sync timestamps are persisted.
const LAST_SYNC_FILE: &str = "last_sync.json";

// ---------------------------------------------------------------------------
// LastSyncState
// ---------------------------------------------------------------------------

/// Persisted state tracking the last successful sync per workspace.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LastSyncState {
    /// Map of workspace_id → ISO 8601 timestamp of last successful sync.
    #[serde(default)]
    pub workspaces: std::collections::HashMap<String, String>,
    /// Map of workspace_id → number of records known at last sync.
    #[serde(default)]
    pub record_counts: std::collections::HashMap<String, usize>,
}

impl LastSyncState {
    /// Load from a file, or return default if the file doesn't exist.
    pub async fn load(path: &Path) -> Result<Self> {
        if fs::try_exists(path).await.unwrap_or(false) {
            let payload = fs::read_to_string(path).await?;
            let state: Self = serde_json::from_str(&payload)
                .context("failed to parse last_sync.json")?;
            Ok(state)
        } else {
            Ok(Self::default())
        }
    }

    /// Save to a file atomically (write to temp, then rename).
    pub async fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let tmp = path.with_extension("json.tmp");
        let payload = serde_json::to_string_pretty(self)?;
        fs::write(&tmp, &payload).await?;
        fs::rename(&tmp, path).await?;
        Ok(())
    }

    /// Get the last sync timestamp for a workspace.
    pub fn last_sync_at(&self, workspace_id: &str) -> Option<DateTime<Utc>> {
        self.workspaces
            .get(workspace_id)
            .and_then(|ts| DateTime::parse_from_rfc3339(ts).ok())
            .map(|dt| dt.with_timezone(&Utc))
    }

    /// Update the last sync timestamp for a workspace.
    pub fn update_sync_at(&mut self, workspace_id: &str, count: usize) {
        self.workspaces
            .insert(workspace_id.to_string(), Utc::now().to_rfc3339());
        self.record_counts
            .insert(workspace_id.to_string(), count);
    }
}

// ---------------------------------------------------------------------------
// SyncReport
// ---------------------------------------------------------------------------

/// Summary of a sync operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncReport {
    /// Workspace that was synced.
    pub workspace_id: String,
    /// Number of records pushed from local to cloud.
    pub pushed: usize,
    /// Number of records pulled from cloud to local.
    pub pulled: usize,
    /// Number of conflicts resolved (local was overridden by cloud, or vice versa).
    pub conflicts: usize,
    /// Duration of the sync operation in milliseconds.
    pub duration_ms: u64,
    /// Whether the sync completed successfully.
    pub success: bool,
    /// Error message if failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl SyncReport {
    fn new(workspace_id: &str) -> Self {
        Self {
            workspace_id: workspace_id.to_string(),
            pushed: 0,
            pulled: 0,
            conflicts: 0,
            duration_ms: 0,
            success: true,
            error: None,
        }
    }
}

// ---------------------------------------------------------------------------
// CloudMemorySync
// ---------------------------------------------------------------------------

/// Orchestrates bidirectional sync between a local store and a cloud store.
///
/// Uses LWW (Last Writer Wins) conflict resolution. Persists sync state
/// to `last_sync.json` so incremental syncs only transfer deltas.
pub struct CloudMemorySync {
    /// The local store (e.g. SQLite).
    local: Arc<dyn MemoryStore>,
    /// The cloud store (e.g. Supabase or Postgres).
    cloud: Arc<dyn MemoryStore>,
    /// This node's identifier (used for LWW tie-breaking).
    pub node_id: String,
    /// Directory where `last_sync.json` is stored.
    data_dir: PathBuf,
}

impl CloudMemorySync {
    /// Create a new `CloudMemorySync`.
    pub fn new(
        local: Arc<dyn MemoryStore>,
        cloud: Arc<dyn MemoryStore>,
        node_id: String,
        data_dir: PathBuf,
    ) -> Self {
        Self {
            local,
            cloud,
            node_id,
            data_dir,
        }
    }

    /// Path to the last-sync state file.
    fn last_sync_path(&self) -> PathBuf {
        self.data_dir.join(LAST_SYNC_FILE)
    }

    /// Load the persisted last-sync state.
    async fn load_state(&self) -> Result<LastSyncState> {
        LastSyncState::load(&self.last_sync_path()).await
    }

    /// Save the last-sync state.
    async fn save_state(&self, state: &LastSyncState) -> Result<()> {
        state.save(&self.last_sync_path()).await
    }

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// Push local records that are newer than the last sync to the cloud.
    ///
    /// Only pushes records whose `updated_at` is strictly greater than
    /// the last sync timestamp for that workspace. Records are sent in
    /// batches of up to `SYNC_BATCH_SIZE`.
    pub async fn push_to_cloud(&self, workspace_id: &str) -> Result<SyncReport> {
        let start = std::time::Instant::now();
        let mut report = SyncReport::new(workspace_id);

        // Determine the "since" timestamp from last sync state
        let state = self.load_state().await?;
        let since = state.last_sync_at(workspace_id);

        // Collect records from local store that are newer than `since`
        let local_records = self.local.list(workspace_id).await?;
        let to_push: Vec<&MemoryRecord> = local_records
            .iter()
            .filter(|r| since.is_none_or(|s| r.updated_at > s))
            .collect();

        // If nothing to push, return early
        if to_push.is_empty() {
            report.duration_ms = start.elapsed().as_millis() as u64;
            return Ok(report);
        }

        // Push in batches
        let _total = to_push.len();
        let mut conflicts = 0usize;

        for chunk in to_push.chunks(SYNC_BATCH_SIZE) {
            for record in chunk {
                // LWW check: if cloud already has a newer version, skip
                let existing = self.cloud.get(workspace_id, &record.id).await?;
                let had_existing = existing.is_some();
                if let Some(ref cloud_record) = existing {
                    if cloud_record.updated_at > record.updated_at {
                        // Cloud is newer — skip, will be pulled later
                        continue;
                    } else if cloud_record.updated_at == record.updated_at {
                        // Same timestamp — tie-break by node_id
                        let local_node = self.lww_node_id(record);
                        let cloud_node = self.lww_node_id(cloud_record);
                        if cloud_node > local_node {
                            continue;
                        }
                    }
                }

                // If we get here, local is newer or non-existent in cloud
                if had_existing {
                    conflicts += 1;
                }
                self.cloud.put((*record).clone()).await?;
                report.pushed += 1;
            }
        }

        report.conflicts = conflicts;
        report.duration_ms = start.elapsed().as_millis() as u64;

        // Update last-sync state
        let mut new_state = self.load_state().await?;
        new_state.update_sync_at(workspace_id, local_records.len());
        self.save_state(&new_state).await?;

        Ok(report)
    }

    /// Pull cloud records that are newer than the last sync to the local store.
    ///
    /// Fetches records in batches of up to `SYNC_BATCH_SIZE` with offset-based
    /// pagination. Each batch is merged locally using LWW resolution.
    pub async fn pull_from_cloud(&self, workspace_id: &str) -> Result<SyncReport> {
        let start = std::time::Instant::now();
        let mut report = SyncReport::new(workspace_id);

        // Determine the "since" timestamp from last sync state
        let state = self.load_state().await?;
        let since = state.last_sync_at(workspace_id);

        // Collect all cloud records
        let cloud_records = self.cloud.list(workspace_id).await?;
        let to_pull: Vec<&MemoryRecord> = cloud_records
            .iter()
            .filter(|r| since.is_none_or(|s| r.updated_at > s))
            .collect();

        if to_pull.is_empty() {
            report.duration_ms = start.elapsed().as_millis() as u64;
            return Ok(report);
        }

        // Apply LWW merge for each record in batches
        let mut conflicts = 0usize;
        for chunk in to_pull.chunks(SYNC_BATCH_SIZE) {
            for record in chunk {
                let existing = self.local.get(workspace_id, &record.id).await?;
                match existing {
                    None => {
                        // New record from cloud — accept
                        self.local.put((*record).clone()).await?;
                        report.pulled += 1;
                    }
                    Some(local_record) => {
                        // LWW: keep the newer one
                        if record.updated_at > local_record.updated_at {
                            conflicts += 1;
                            self.local.put((*record).clone()).await?;
                            report.pulled += 1;
                        } else if record.updated_at == local_record.updated_at {
                            let local_node = self.lww_node_id(&local_record);
                            let cloud_node = self.lww_node_id(record);
                            if cloud_node > local_node {
                                conflicts += 1;
                                self.local.put((*record).clone()).await?;
                                report.pulled += 1;
                            }
                        }
                        // else local is newer, skip
                    }
                }
            }
        }

        report.conflicts = conflicts;
        report.duration_ms = start.elapsed().as_millis() as u64;

        // Update last-sync state
        let local_records = self.local.list(workspace_id).await?;
        let mut new_state = self.load_state().await?;
        new_state.update_sync_at(workspace_id, local_records.len());
        self.save_state(&new_state).await?;

        Ok(report)
    }

    /// Full bidirectional sync: pull from cloud → merge → push to cloud.
    ///
    /// 1. Pull cloud records newer than last sync → merge into local (LWW).
    /// 2. Push local records newer than last sync → merge into cloud (LWW).
    ///
    /// State is saved once at the END so both phases see the original `since`
    /// timestamp and neither misses records created mid-sync.
    ///
    /// Returns a combined report.
    pub async fn sync_all(&self, workspace_id: &str) -> Result<SyncReport> {
        let start = std::time::Instant::now();
        let mut report = SyncReport::new(workspace_id);

        // Capture the "since" timestamp ONCE before any mutations
        let state_before = self.load_state().await?;
        let since = state_before.last_sync_at(workspace_id);

        // Step 1: Pull cloud records → local (LWW)
        {
            let cloud_records = self.cloud.list(workspace_id).await?;
            let to_pull: Vec<&MemoryRecord> = cloud_records
                .iter()
                .filter(|r| since.is_none_or(|s| r.updated_at > s))
                .collect();

            for chunk in to_pull.chunks(SYNC_BATCH_SIZE) {
                for record in chunk {
                    let existing = self.local.get(workspace_id, &record.id).await?;
                    if let Some(local_record) = existing {
                        if record.updated_at > local_record.updated_at {
                            report.conflicts += 1;
                            self.local.put((*record).clone()).await?;
                            report.pulled += 1;
                        } else if record.updated_at == local_record.updated_at {
                            let cloud_node = self.lww_node_id(record);
                            let local_node = self.lww_node_id(&local_record);
                            if cloud_node > local_node {
                                report.conflicts += 1;
                                self.local.put((*record).clone()).await?;
                                report.pulled += 1;
                            }
                        }
                    } else {
                        self.local.put((*record).clone()).await?;
                        report.pulled += 1;
                    }
                }
            }
        }

        // Step 2: Push local records → cloud (LWW)
        {
            let local_records = self.local.list(workspace_id).await?;
            let to_push: Vec<&MemoryRecord> = local_records
                .iter()
                .filter(|r| since.is_none_or(|s| r.updated_at > s))
                .collect();

            for chunk in to_push.chunks(SYNC_BATCH_SIZE) {
                for record in chunk {
                    let existing = self.cloud.get(workspace_id, &record.id).await?;
                    let had_existing = existing.is_some();
                    if let Some(ref cloud_record) = existing {
                        if cloud_record.updated_at > record.updated_at {
                            continue;
                        } else if cloud_record.updated_at == record.updated_at {
                            let cloud_node = self.lww_node_id(cloud_record);
                            let local_node = self.lww_node_id(record);
                            if cloud_node > local_node {
                                continue;
                            }
                        }
                    }
                    if had_existing {
                        report.conflicts += 1;
                    }
                    self.cloud.put((*record).clone()).await?;
                    report.pushed += 1;
                }
            }
        }

        // Step 3: Save state once with the final record counts
        let local_count = self.local.list(workspace_id).await?.len();
        let mut new_state = self.load_state().await?;
        new_state.update_sync_at(workspace_id, local_count);
        self.save_state(&new_state).await?;

        report.duration_ms = start.elapsed().as_millis() as u64;

        Ok(report)
    }

    /// Sync all known workspaces bidirectionally.
    ///
    /// Collects the set of workspace IDs from both stores, deduplicates,
    /// and runs `sync_all` on each.
    pub async fn sync_all_workspaces(&self) -> Result<Vec<SyncReport>> {
        // Collect workspace IDs from both sides
        let local_records = self.local.list("").await?;
        let cloud_records = self.cloud.list("").await?;

        let mut workspace_ids: HashSet<String> = HashSet::new();
        for r in &local_records {
            workspace_ids.insert(r.workspace_id.clone());
        }
        for r in &cloud_records {
            workspace_ids.insert(r.workspace_id.clone());
        }

        let mut reports = Vec::new();
        for ws_id in workspace_ids {
            match self.sync_all(&ws_id).await {
                Ok(report) => reports.push(report),
                Err(e) => {
                    reports.push(SyncReport {
                        workspace_id: ws_id,
                        pushed: 0,
                        pulled: 0,
                        conflicts: 0,
                        duration_ms: 0,
                        success: false,
                        error: Some(format!("{e:#}")),
                    });
                }
            }
        }

        Ok(reports)
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Derive the LWW node id from a record's metadata (or fallback to this node's id).
    fn lww_node_id(&self, record: &MemoryRecord) -> String {
        record
            .metadata
            .get("node_id")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.node_id)
            .to_string()
    }
}

// ---------------------------------------------------------------------------
// CloudSyncConfig
// ---------------------------------------------------------------------------

/// Configuration for cloud sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudSyncConfig {
    /// Batch size for push/pull operations.
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    /// Whether to auto-sync on startup.
    #[serde(default)]
    pub auto_sync_on_startup: bool,
    /// Interval in seconds between automatic syncs (0 = disabled).
    #[serde(default)]
    pub auto_sync_interval_secs: u64,
    /// Directory for storing `last_sync.json`.
    pub data_dir: String,
}

fn default_batch_size() -> usize {
    SYNC_BATCH_SIZE
}

impl Default for CloudSyncConfig {
    fn default() -> Self {
        Self {
            batch_size: SYNC_BATCH_SIZE,
            auto_sync_on_startup: false,
            auto_sync_interval_secs: 0,
            data_dir: "data".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::sync::manifest::tests::TestStore;
    use chrono::TimeDelta;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn make_record(
        id: &str,
        workspace_id: &str,
        content: &str,
        updated_at: DateTime<Utc>,
        revision: u64,
        node_id: &str,
    ) -> MemoryRecord {
        let mut meta = serde_json::Map::new();
        meta.insert("node_id".to_string(), serde_json::Value::String(node_id.to_string()));
        MemoryRecord {
            id: id.to_string(),
            workspace_id: workspace_id.to_string(),
            path: format!("test/{}", id),
            content: content.to_string(),
            metadata: serde_json::Value::Object(meta),
            embedding: Vec::new(),
            created_at: updated_at,
            updated_at,
            revision,
            primary: true,
            parent_id: None,
            cluster_id: None,
            level: crate::memory::schema::MemoryLevel::Raw,
            relation: None,
            clearance: crate::memory::schema::ClearanceLevel::Unclassified,
            revisions: Vec::new(),
            encrypted_dek: None,
            content_iv: None,
            metadata_iv: None,
        }
    }

    fn create_cloud_sync(local: TestStore, cloud: TestStore) -> (CloudMemorySync, TempDir) {
        let tmp = TempDir::new().unwrap();
        let sync = CloudMemorySync::new(
            Arc::new(local),
            Arc::new(cloud),
            "node_test".to_string(),
            tmp.path().to_path_buf(),
        );
        (sync, tmp)
    }

    /// Helper: create bare TestStore (no fields initializer because it's pub(crate) in manifest)
    fn new_test_store() -> TestStore {
        TestStore {
            records: std::sync::Mutex::new(Vec::new()),
        }
    }

    #[tokio::test]
    async fn test_push_local_to_cloud() {
        let local = Arc::new(new_test_store());
        let cloud = Arc::new(new_test_store());
        let sync = CloudMemorySync::new(
            local.clone(),
            cloud.clone(),
            "node_test".to_string(),
            TempDir::new().unwrap().path().to_path_buf(),
        );

        local.put(make_record("r1", "episodic", "local content", Utc::now(), 1, "node_test")).await.unwrap();

        let report = sync.push_to_cloud("episodic").await.unwrap();
        assert_eq!(report.pushed, 1, "should push 1 record");
        assert_eq!(report.conflicts, 0, "no conflicts on first push");

        let cloud_recs = cloud.list("episodic").await.unwrap();
        assert_eq!(cloud_recs.len(), 1);
        assert_eq!(cloud_recs[0].content, "local content");
    }

    #[tokio::test]
    async fn test_push_empty_no_new_records() {
        let (sync, _tmp) = create_cloud_sync(new_test_store(), new_test_store());
        let report = sync.push_to_cloud("episodic").await.unwrap();
        assert_eq!(report.pushed, 0, "nothing to push");
    }

    #[tokio::test]
    async fn test_pull_cloud_to_local() {
        let local = Arc::new(new_test_store());
        let cloud = Arc::new(new_test_store());
        let sync = CloudMemorySync::new(
            local.clone(),
            cloud.clone(),
            "node_test".to_string(),
            TempDir::new().unwrap().path().to_path_buf(),
        );

        // Add a record to cloud
        cloud.put(make_record("r1", "episodic", "cloud content", Utc::now(), 1, "node_cloud")).await.unwrap();

        let report = sync.pull_from_cloud("episodic").await.unwrap();
        assert_eq!(report.pulled, 1, "should pull 1 record");
        assert_eq!(report.conflicts, 0, "no conflicts on first pull");

        let local_recs = local.list("episodic").await.unwrap();
        assert_eq!(local_recs.len(), 1);
        assert_eq!(local_recs[0].content, "cloud content");
    }

    #[tokio::test]
    async fn test_bidirectional_sync() {
        let local = Arc::new(new_test_store());
        let cloud = Arc::new(new_test_store());
        let sync = CloudMemorySync::new(
            local.clone(),
            cloud.clone(),
            "node_test".to_string(),
            TempDir::new().unwrap().path().to_path_buf(),
        );

        local.put(make_record("a", "episodic", "from local", Utc::now(), 1, "node_test")).await.unwrap();
        cloud.put(make_record("b", "episodic", "from cloud", Utc::now() + TimeDelta::seconds(1), 1, "node_cloud")).await.unwrap();

        let report = sync.sync_all("episodic").await.unwrap();
        assert!(report.pushed >= 1 || report.pulled >= 1, "should sync something");

        assert_eq!(local.list("episodic").await.unwrap().len(), 2, "local should have both records");
        assert_eq!(cloud.list("episodic").await.unwrap().len(), 2, "cloud should have both records");
    }

    #[tokio::test]
    async fn test_lww_cloud_newer_wins_on_pull() {
        let local = Arc::new(new_test_store());
        let cloud = Arc::new(new_test_store());
        let sync = CloudMemorySync::new(
            local.clone(),
            cloud.clone(),
            "node_test".to_string(),
            TempDir::new().unwrap().path().to_path_buf(),
        );

        let now = Utc::now();
        local.put(make_record("r1", "episodic", "old local", now, 1, "node_local")).await.unwrap();
        cloud.put(make_record("r1", "episodic", "newer cloud", now + TimeDelta::seconds(10), 2, "node_cloud")).await.unwrap();

        let report = sync.pull_from_cloud("episodic").await.unwrap();
        assert_eq!(report.pulled, 1, "should pull the cloud version");
        assert_eq!(report.conflicts, 1, "should detect conflict");

        let fetched = local.get("episodic", "r1").await.unwrap().unwrap();
        assert_eq!(fetched.content, "newer cloud", "newer cloud should win");
    }

    #[tokio::test]
    async fn test_lww_same_timestamp_node_id_tiebreak() {
        let local = Arc::new(new_test_store());
        let cloud = Arc::new(new_test_store());
        let sync = CloudMemorySync::new(
            local.clone(),
            cloud.clone(),
            "node_test".to_string(),
            TempDir::new().unwrap().path().to_path_buf(),
        );

        let now = Utc::now();
        local.put(make_record("r1", "episodic", "from A", now, 1, "A")).await.unwrap();
        cloud.put(make_record("r1", "episodic", "from B", now, 1, "B")).await.unwrap();

        let report = sync.pull_from_cloud("episodic").await.unwrap();
        assert_eq!(report.conflicts, 1, "should detect conflict");

        let fetched = local.get("episodic", "r1").await.unwrap().unwrap();
        assert_eq!(fetched.content, "from B", "B > A → B wins");
    }

    #[tokio::test]
    async fn test_persist_last_sync_state() {
        let local = Arc::new(new_test_store());
        let tmp = TempDir::new().unwrap();
        let sync = CloudMemorySync::new(
            local.clone(),
            Arc::new(new_test_store()),
            "node_test".to_string(),
            tmp.path().to_path_buf(),
        );

        local.put(make_record("r1", "episodic", "content", Utc::now(), 1, "node_test")).await.unwrap();
        sync.push_to_cloud("episodic").await.unwrap();

        let state_path = tmp.path().join("last_sync.json");
        assert!(state_path.exists(), "last_sync.json should exist");

        let state = LastSyncState::load(&state_path).await.unwrap();
        assert!(state.last_sync_at("episodic").is_some(), "should have timestamp");
    }

    #[tokio::test]
    async fn test_batch_100_records() {
        let local = Arc::new(new_test_store());
        let cloud = Arc::new(new_test_store());
        let sync = CloudMemorySync::new(
            local.clone(),
            cloud.clone(),
            "node_test".to_string(),
            TempDir::new().unwrap().path().to_path_buf(),
        );

        for i in 0..150u64 {
            local.put(make_record(
                &format!("r{}", i),
                "episodic",
                &format!("content {}", i),
                Utc::now() + TimeDelta::milliseconds(i as i64),
                i,
                "node_test",
            )).await.unwrap();
        }

        let report = sync.push_to_cloud("episodic").await.unwrap();
        assert_eq!(report.pushed, 150, "should push all 150 records");

        assert_eq!(cloud.list("episodic").await.unwrap().len(), 150, "cloud should have all 150 records");
    }

    #[tokio::test]
    async fn test_sync_all_workspaces() {
        let local = Arc::new(new_test_store());
        let cloud = Arc::new(new_test_store());
        let sync = CloudMemorySync::new(
            local.clone(),
            cloud.clone(),
            "node_test".to_string(),
            TempDir::new().unwrap().path().to_path_buf(),
        );

        local.put(make_record("r1", "workspace_a", "from local a", Utc::now(), 1, "node_test")).await.unwrap();
        cloud.put(make_record("r2", "workspace_b", "from cloud b", Utc::now() + TimeDelta::seconds(1), 1, "node_cloud")).await.unwrap();

        let reports = sync.sync_all_workspaces().await.unwrap();
        assert_eq!(reports.len(), 2, "should sync 2 workspaces");

        for r in &reports {
            assert!(r.success, "sync should succeed for workspace {}", r.workspace_id);
        }
    }

    #[tokio::test]
    async fn test_incremental_sync_after_full_sync() {
        let local = Arc::new(new_test_store());
        let cloud = Arc::new(new_test_store());
        let sync = CloudMemorySync::new(
            local.clone(),
            cloud.clone(),
            "node_test".to_string(),
            TempDir::new().unwrap().path().to_path_buf(),
        );

        let rec1 = make_record("r1", "episodic", "first", Utc::now(), 1, "node_test");
        local.put(rec1).await.unwrap();
        let report1 = sync.push_to_cloud("episodic").await.unwrap();
        assert_eq!(report1.pushed, 1);

        let rec2 = make_record("r2", "episodic", "second", Utc::now() + TimeDelta::seconds(5), 1, "node_test");
        local.put(rec2).await.unwrap();
        let report2 = sync.push_to_cloud("episodic").await.unwrap();
        assert_eq!(report2.pushed, 1, "should only push the new record incrementally");

        assert_eq!(cloud.list("episodic").await.unwrap().len(), 2, "cloud should have both records");
    }
}
