//! Continuous SQLite Write-Ahead Logging (WAL) streamer and point-in-time recovery.
//!
//! Issue #1445: Continuous SQLite WAL replication implementation.
//! Monitors SQLite WAL files for changes, streams incremental WAL segment files,
//! manages periodic full database snapshots, and supports point-in-time recovery.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// Helper to compute the WAL file path corresponding to a SQLite database file.
///
/// In SQLite, WAL mode creates a log file with `-wal` appended to the database filename.
pub fn wal_path_for_db(db_path: &Path) -> PathBuf {
    let mut os_str = db_path.as_os_str().to_os_string();
    os_str.push("-wal");
    PathBuf::from(os_str)
}

/// Convert a `SystemTime` instance into an RFC3339 string.
fn system_time_to_rfc3339(time: SystemTime) -> String {
    chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339()
}

/// Get current UNIX timestamp in seconds.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Get current UNIX timestamp in milliseconds.
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Configuration settings for [`WalStreamer`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalStreamerConfig {
    /// Path to the source SQLite database file.
    pub db_path: PathBuf,
    /// Destination directory for storing snapshots, WAL segments, and manifest.
    pub backup_dir: PathBuf,
    /// Time interval between automatic snapshot creations.
    pub snapshot_interval: Duration,
    /// Optional limit on the maximum byte size per read WAL segment chunk.
    pub max_segment_size: Option<usize>,
    /// Whether to perform a SQLite WAL checkpoint (`TRUNCATE`) when taking a snapshot.
    pub checkpoint_on_snapshot: bool,
}

impl WalStreamerConfig {
    /// Create a new configuration with sensible default parameters.
    pub fn new(db_path: impl Into<PathBuf>, backup_dir: impl Into<PathBuf>) -> Self {
        Self {
            db_path: db_path.into(),
            backup_dir: backup_dir.into(),
            snapshot_interval: Duration::from_secs(3600), // Default: 1 hour
            max_segment_size: None,
            checkpoint_on_snapshot: true,
        }
    }

    /// Set the snapshot creation interval.
    pub fn with_snapshot_interval(mut self, interval: Duration) -> Self {
        self.snapshot_interval = interval;
        self
    }

    /// Enable or disable checkpointing on snapshot creation.
    pub fn with_checkpoint_on_snapshot(mut self, enable: bool) -> Self {
        self.checkpoint_on_snapshot = enable;
        self
    }

    /// Set maximum byte size per WAL segment chunk.
    pub fn with_max_segment_size(mut self, max_size: usize) -> Self {
        self.max_segment_size = Some(max_size);
        self
    }
}

/// Metadata for a full database snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotMetadata {
    /// Unique snapshot identifier.
    pub snapshot_id: String,
    /// Monotonically increasing sequence number.
    pub sequence: u64,
    /// Path to snapshot database file relative to `backup_dir`.
    pub file_path: PathBuf,
    /// File size in bytes.
    pub file_size: u64,
    /// RFC3339 creation timestamp.
    pub created_at: String,
    /// UNIX timestamp in seconds.
    pub timestamp_secs: u64,
    /// UNIX timestamp in milliseconds.
    #[serde(default)]
    pub timestamp_millis: u64,
}

/// Metadata for an incremental WAL segment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WalSegmentMetadata {
    /// Unique segment identifier.
    pub segment_id: String,
    /// Monotonically increasing sequence number.
    pub sequence: u64,
    /// Sequence number of the latest snapshot when this WAL segment was created.
    pub parent_snapshot_seq: u64,
    /// Path to WAL segment file relative to `backup_dir`.
    pub file_path: PathBuf,
    /// Start byte offset in the source WAL file.
    pub start_offset: u64,
    /// End byte offset in the source WAL file.
    pub end_offset: u64,
    /// Segment file size in bytes.
    pub file_size: u64,
    /// RFC3339 creation timestamp.
    pub created_at: String,
    /// UNIX timestamp in seconds.
    pub timestamp_secs: u64,
    /// UNIX timestamp in milliseconds.
    #[serde(default)]
    pub timestamp_millis: u64,
}

/// Persistent manifest indexing all snapshots and WAL segments in a backup repository.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BackupManifest {
    /// Name or identifier of the source database.
    pub database_name: String,
    /// RFC3339 creation timestamp of the manifest.
    pub created_at: String,
    /// RFC3339 timestamp when manifest was last updated.
    pub last_updated_at: String,
    /// List of recorded full database snapshots.
    pub snapshots: Vec<SnapshotMetadata>,
    /// List of recorded incremental WAL segments.
    pub wal_segments: Vec<WalSegmentMetadata>,
    /// Current sequence counter for snapshots.
    pub current_snapshot_seq: u64,
    /// Current sequence counter for WAL segments.
    pub current_wal_seq: u64,
}

impl BackupManifest {
    /// Save manifest as formatted JSON to the specified path.
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let content =
            serde_json::to_string_pretty(self).context("Failed to serialize backup manifest")?;
        fs::write(path, content).context("Failed to write backup manifest file")?;
        Ok(())
    }

    /// Load manifest from a JSON file.
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path).context("Failed to read backup manifest file")?;
        let manifest: Self =
            serde_json::from_str(&content).context("Failed to parse backup manifest JSON")?;
        Ok(manifest)
    }
}

/// Result report for a database recovery operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryReport {
    /// Absolute path to the restored database file.
    pub target_path: PathBuf,
    /// Identifier of the base snapshot used for recovery.
    pub base_snapshot_id: String,
    /// Count of incremental WAL segments applied on top of the base snapshot.
    pub segments_applied: usize,
    /// Requested target point-in-time timestamp (RFC3339), if specified.
    pub point_in_time_requested: Option<String>,
    /// Timestamp when recovery execution completed.
    pub recovered_at: String,
    /// Result of SQLite integrity check (`PRAGMA quick_check;`).
    pub integrity_check: String,
}

/// Continuous WAL change streamer and snapshot manager.
pub struct WalStreamer {
    config: WalStreamerConfig,
    manifest: BackupManifest,
    last_wal_offset: u64,
    last_snapshot_at: Option<SystemTime>,
}

impl WalStreamer {
    /// Create or attach a `WalStreamer` to a backup repository directory.
    pub fn new(config: WalStreamerConfig) -> Result<Self> {
        let snapshots_dir = config.backup_dir.join("snapshots");
        let segments_dir = config.backup_dir.join("wal_segments");
        fs::create_dir_all(&snapshots_dir).context("Failed to create snapshots directory")?;
        fs::create_dir_all(&segments_dir).context("Failed to create wal_segments directory")?;

        let manifest_path = config.backup_dir.join("manifest.json");
        let manifest = if manifest_path.exists() {
            BackupManifest::load_from_file(&manifest_path)?
        } else {
            let db_name = config
                .db_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "database".to_string());
            let now = chrono::Utc::now().to_rfc3339();
            let m = BackupManifest {
                database_name: db_name,
                created_at: now.clone(),
                last_updated_at: now,
                snapshots: Vec::new(),
                wal_segments: Vec::new(),
                current_snapshot_seq: 0,
                current_wal_seq: 0,
            };
            m.save_to_file(&manifest_path)?;
            m
        };

        let last_wal_offset = manifest
            .wal_segments
            .last()
            .map(|s| s.end_offset)
            .unwrap_or(0);

        Ok(Self {
            config,
            manifest,
            last_wal_offset,
            last_snapshot_at: None,
        })
    }

    /// Access the streamer's configuration.
    pub fn config(&self) -> &WalStreamerConfig {
        &self.config
    }

    /// Access the streamer's current manifest.
    pub fn manifest(&self) -> &BackupManifest {
        &self.manifest
    }

    /// Access current tracked WAL byte offset.
    pub fn last_wal_offset(&self) -> u64 {
        self.last_wal_offset
    }

    /// Access timestamp of the last snapshot taken by this streamer instance.
    pub fn last_snapshot_at(&self) -> Option<SystemTime> {
        self.last_snapshot_at
    }

    /// Create a full snapshot of the source database.
    pub fn create_snapshot(&mut self) -> Result<SnapshotMetadata> {
        let now = SystemTime::now();
        let timestamp_str = system_time_to_rfc3339(now);
        let timestamp_secs = now_secs();
        let timestamp_millis = now_millis();

        // Checkpoint source database if it exists and checkpointing is enabled
        if self.config.db_path.exists() {
            if self.config.checkpoint_on_snapshot {
                if let Ok(conn) = Connection::open(&self.config.db_path) {
                    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
                }
            }
        } else {
            let conn = Connection::open(&self.config.db_path)
                .context("Failed to initialize database for snapshot")?;
            let _ = crate::storage::apply_pragmas(&conn);
        }

        let seq = self.manifest.current_snapshot_seq + 1;
        let snapshot_filename = format!("snapshot_{:06}_{}.db", seq, timestamp_secs);
        let relative_path = PathBuf::from("snapshots").join(&snapshot_filename);
        let full_snapshot_path = self.config.backup_dir.join(&relative_path);

        // Perform SQLite snapshot: WAL checkpoint + file copy
        {
            let conn = Connection::open(&self.config.db_path)
                .context("Failed to open source database for snapshot backup")?;
            // Checkpoint WAL into main database before copy
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .context("Failed to checkpoint WAL")?;
        }
        fs::copy(&self.config.db_path, &full_snapshot_path)
            .context("Failed to copy database file for snapshot")?;

        let file_size = fs::metadata(&full_snapshot_path)
            .map(|m| m.len())
            .unwrap_or(0);

        let snapshot_metadata = SnapshotMetadata {
            snapshot_id: format!("snap_{:06}", seq),
            sequence: seq,
            file_path: relative_path,
            file_size,
            created_at: timestamp_str,
            timestamp_secs,
            timestamp_millis,
        };

        self.manifest.current_snapshot_seq = seq;
        self.manifest.snapshots.push(snapshot_metadata.clone());
        self.manifest.last_updated_at = chrono::Utc::now().to_rfc3339();

        let wal_path = wal_path_for_db(&self.config.db_path);
        self.last_wal_offset = if wal_path.exists() {
            fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };

        self.last_snapshot_at = Some(now);

        let manifest_path = self.config.backup_dir.join("manifest.json");
        self.manifest.save_to_file(&manifest_path)?;

        info!(
            "Created full snapshot {} ({} bytes)",
            snapshot_metadata.snapshot_id, snapshot_metadata.file_size
        );

        Ok(snapshot_metadata)
    }

    /// Read newly written WAL bytes from the WAL file and emit an incremental WAL segment.
    pub fn stream_wal_changes(&mut self) -> Result<Option<WalSegmentMetadata>> {
        let wal_path = wal_path_for_db(&self.config.db_path);
        if !wal_path.exists() {
            return Ok(None);
        }

        let meta = fs::metadata(&wal_path).context("Failed to read metadata for WAL file")?;
        let file_len = meta.len();

        // Detect WAL file truncation/reset (e.g. after a checkpoint)
        if file_len < self.last_wal_offset {
            debug!(
                "WAL file truncated from {} to {} bytes; resetting offset to 0",
                self.last_wal_offset, file_len
            );
            self.last_wal_offset = 0;
        }

        if file_len == self.last_wal_offset {
            return Ok(None);
        }

        let unread_bytes = (file_len - self.last_wal_offset) as usize;
        let bytes_to_read = if let Some(max_sz) = self.config.max_segment_size {
            unread_bytes.min(max_sz)
        } else {
            unread_bytes
        };

        if bytes_to_read == 0 {
            return Ok(None);
        }

        let mut file = File::open(&wal_path).context("Failed to open WAL file")?;
        file.seek(SeekFrom::Start(self.last_wal_offset))
            .context("Failed to seek to current WAL offset")?;

        let mut buffer = vec![0u8; bytes_to_read];
        file.read_exact(&mut buffer)
            .context("Failed to read WAL change bytes")?;

        let seq = self.manifest.current_wal_seq + 1;
        let now = SystemTime::now();
        let timestamp_str = system_time_to_rfc3339(now);
        let timestamp_secs = now_secs();
        let timestamp_millis = now_millis();

        let parent_snapshot_seq = self
            .manifest
            .snapshots
            .last()
            .map(|s| s.sequence)
            .unwrap_or(0);

        let segment_filename = format!("wal_seg_{:06}_{}.wal", seq, timestamp_secs);
        let relative_path = PathBuf::from("wal_segments").join(&segment_filename);
        let full_segment_path = self.config.backup_dir.join(&relative_path);

        fs::write(&full_segment_path, &buffer).context("Failed to write WAL segment file")?;

        let start_offset = self.last_wal_offset;
        let end_offset = self.last_wal_offset + bytes_to_read as u64;
        self.last_wal_offset = end_offset;

        let segment_metadata = WalSegmentMetadata {
            segment_id: format!("seg_{:06}", seq),
            sequence: seq,
            parent_snapshot_seq,
            file_path: relative_path,
            start_offset,
            end_offset,
            file_size: bytes_to_read as u64,
            created_at: timestamp_str,
            timestamp_secs,
            timestamp_millis,
        };

        self.manifest.current_wal_seq = seq;
        self.manifest.wal_segments.push(segment_metadata.clone());
        self.manifest.last_updated_at = chrono::Utc::now().to_rfc3339();

        let manifest_path = self.config.backup_dir.join("manifest.json");
        self.manifest.save_to_file(&manifest_path)?;

        info!(
            "Streamed WAL segment {} ({} bytes, offsets {}-{})",
            segment_metadata.segment_id, bytes_to_read, start_offset, end_offset
        );

        Ok(Some(segment_metadata))
    }

    /// Alias for [`stream_wal_changes`].
    pub fn sync_wal(&mut self) -> Result<Option<WalSegmentMetadata>> {
        self.stream_wal_changes()
    }

    /// Check whether `snapshot_interval` has elapsed, creating a snapshot if due.
    pub fn check_and_snapshot_interval(&mut self) -> Result<Option<SnapshotMetadata>> {
        let is_due = match self.last_snapshot_at {
            None => true,
            Some(last) => {
                SystemTime::now().duration_since(last).unwrap_or_default()
                    >= self.config.snapshot_interval
            }
        };

        if is_due {
            let snap = self.create_snapshot()?;
            Ok(Some(snap))
        } else {
            Ok(None)
        }
    }

    /// Restore database from a backup directory to a target file path.
    ///
    /// If `point_in_time` is `None`, restores to the latest available state.
    /// If `point_in_time` is `Some(time)`, restores state at or before `time`.
    pub fn recover(
        backup_dir: &Path,
        target_path: &Path,
        point_in_time: Option<SystemTime>,
    ) -> Result<RecoveryReport> {
        if !backup_dir.exists() {
            return Err(anyhow!("Backup directory does not exist: {:?}", backup_dir));
        }

        let manifest_path = backup_dir.join("manifest.json");
        if !manifest_path.exists() {
            return Err(anyhow!(
                "Backup manifest file manifest.json not found in {:?}",
                backup_dir
            ));
        }

        let manifest = BackupManifest::load_from_file(&manifest_path)?;
        if manifest.snapshots.is_empty() {
            return Err(anyhow!("No snapshots found in backup manifest"));
        }

        let cutoff_millis = point_in_time
            .map(|t| t.duration_since(UNIX_EPOCH).unwrap().as_millis() as u64)
            .unwrap_or(u64::MAX);

        let get_snap_millis = |s: &SnapshotMetadata| {
            if s.timestamp_millis > 0 {
                s.timestamp_millis
            } else {
                s.timestamp_secs * 1000
            }
        };

        let base_snapshot = manifest
            .snapshots
            .iter()
            .filter(|s| get_snap_millis(s) <= cutoff_millis)
            .max_by_key(|s| s.sequence)
            .ok_or_else(|| {
                anyhow!(
                    "No snapshot found created at or before requested cutoff_millis ({})",
                    cutoff_millis
                )
            })?;

        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).context("Failed to create target recovery directory")?;
        }

        let full_snap_path = backup_dir.join(&base_snapshot.file_path);
        fs::copy(&full_snap_path, target_path).with_context(|| {
            format!(
                "Failed to copy snapshot file {:?} to {:?}",
                full_snap_path, target_path
            )
        })?;

        let get_seg_millis = |s: &WalSegmentMetadata| {
            if s.timestamp_millis > 0 {
                s.timestamp_millis
            } else {
                s.timestamp_secs * 1000
            }
        };

        let base_snap_millis = get_snap_millis(base_snapshot);

        let mut applicable_segments: Vec<&WalSegmentMetadata> = manifest
            .wal_segments
            .iter()
            .filter(|seg| {
                let seg_m = get_seg_millis(seg);
                (seg.parent_snapshot_seq == base_snapshot.sequence
                    || seg_m >= base_snap_millis)
                    && seg_m <= cutoff_millis
            })
            .collect();

        applicable_segments.sort_by_key(|s| s.sequence);
        let segments_applied_count = applicable_segments.len();

        if !applicable_segments.is_empty() {
            let mut cycles: Vec<Vec<&WalSegmentMetadata>> = Vec::new();
            for seg in applicable_segments {
                if seg.start_offset == 0 || cycles.is_empty() {
                    cycles.push(vec![seg]);
                } else {
                    cycles.last_mut().unwrap().push(seg);
                }
            }

            let target_wal = wal_path_for_db(target_path);

            for cycle in cycles {
                let mut wal_file = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&target_wal)
                    .context("Failed to open target WAL file for write")?;

                for seg in cycle {
                    let seg_full_path = backup_dir.join(&seg.file_path);
                    let seg_bytes = fs::read(&seg_full_path).with_context(|| {
                        format!("Failed to read WAL segment file {:?}", seg_full_path)
                    })?;
                    wal_file
                        .write_all(&seg_bytes)
                        .context("Failed to write WAL segment bytes into target WAL")?;
                }
                wal_file.flush()?;
                drop(wal_file);

                let conn = Connection::open(target_path)
                    .context("Failed to open target database during WAL recovery replay")?;
                let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
                drop(conn);

                if target_wal.exists() {
                    let _ = fs::remove_file(&target_wal);
                }
            }
        }

        let conn = Connection::open(target_path)
            .context("Failed to open recovered database for integrity check")?;
        let integrity: String = conn
            .query_row("PRAGMA quick_check;", [], |r| r.get(0))
            .unwrap_or_else(|e| format!("check_failed: {}", e));
        drop(conn);

        let pit_req_str = point_in_time.map(system_time_to_rfc3339);
        let recovered_at = chrono::Utc::now().to_rfc3339();

        info!(
            "Recovered database to {:?} using snapshot {} with {} WAL segments. Integrity: {}",
            target_path, base_snapshot.snapshot_id, segments_applied_count, integrity
        );

        Ok(RecoveryReport {
            target_path: target_path.to_path_buf(),
            base_snapshot_id: base_snapshot.snapshot_id.clone(),
            segments_applied: segments_applied_count,
            point_in_time_requested: pit_req_str,
            recovered_at,
            integrity_check: integrity,
        })
    }

    /// Restore database at a specific point in time.
    pub fn recover_to_point(
        backup_dir: &Path,
        target_path: &Path,
        point_in_time: SystemTime,
    ) -> Result<RecoveryReport> {
        Self::recover(backup_dir, target_path, Some(point_in_time))
    }

    /// Verify backup repository validity and manifest presence.
    pub fn verify_backup(backup_dir: &Path) -> Result<bool> {
        if !backup_dir.exists() {
            return Ok(false);
        }
        let manifest_path = backup_dir.join("manifest.json");
        if !manifest_path.exists() {
            return Ok(false);
        }
        let manifest = BackupManifest::load_from_file(&manifest_path)?;
        for snap in &manifest.snapshots {
            if !backup_dir.join(&snap.file_path).exists() {
                return Ok(false);
            }
        }
        for seg in &manifest.wal_segments {
            if !backup_dir.join(&seg.file_path).exists() {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_db(db_path: &Path) -> Connection {
        let conn = Connection::open(db_path).expect("Failed to create test DB");
        crate::storage::apply_pragmas(&conn).expect("Failed to apply pragmas on test DB");
        conn.execute_batch(
            "CREATE TABLE test_data (id INTEGER PRIMARY KEY, content TEXT, created_at TEXT);",
        )
        .expect("Failed to initialize test DB schema");
        conn
    }

    #[test]
    fn test_wal_streamer_config_builder() {
        let db_path = PathBuf::from("/tmp/test.db");
        let backup_dir = PathBuf::from("/tmp/backup");
        let config = WalStreamerConfig::new(&db_path, &backup_dir)
            .with_snapshot_interval(Duration::from_secs(120))
            .with_checkpoint_on_snapshot(false)
            .with_max_segment_size(8192);

        assert_eq!(config.db_path, db_path);
        assert_eq!(config.backup_dir, backup_dir);
        assert_eq!(config.snapshot_interval, Duration::from_secs(120));
        assert!(!config.checkpoint_on_snapshot);
        assert_eq!(config.max_segment_size, Some(8192));
    }

    #[test]
    fn test_wal_streamer_initialization() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("main.db");
        let backup_dir = temp_dir.path().join("backup");

        let streamer = WalStreamer::new(WalStreamerConfig::new(&db_path, &backup_dir))
            .expect("Failed to initialize WalStreamer");

        assert!(backup_dir.join("snapshots").exists());
        assert!(backup_dir.join("wal_segments").exists());
        assert!(backup_dir.join("manifest.json").exists());
        assert_eq!(streamer.manifest().snapshots.len(), 0);
        assert_eq!(streamer.manifest().wal_segments.len(), 0);
    }

    #[test]
    fn test_create_snapshot_empty_db() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("main.db");
        let backup_dir = temp_dir.path().join("backup");

        let _conn = create_test_db(&db_path);

        let mut streamer = WalStreamer::new(WalStreamerConfig::new(&db_path, &backup_dir)).unwrap();
        let snap = streamer.create_snapshot().unwrap();

        assert_eq!(snap.sequence, 1);
        assert_eq!(streamer.manifest().snapshots.len(), 1);
        assert!(backup_dir.join(&snap.file_path).exists());
    }

    #[test]
    fn test_create_snapshot_with_data() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("main.db");
        let backup_dir = temp_dir.path().join("backup");

        let conn = create_test_db(&db_path);
        conn.execute(
            "INSERT INTO test_data (content, created_at) VALUES ('hello', 'now')",
            [],
        )
        .unwrap();

        let mut streamer = WalStreamer::new(WalStreamerConfig::new(&db_path, &backup_dir)).unwrap();
        let snap = streamer.create_snapshot().unwrap();

        let snap_full_path = backup_dir.join(&snap.file_path);
        let snap_conn = Connection::open(&snap_full_path).unwrap();
        let count: i64 = snap_conn
            .query_row("SELECT COUNT(*) FROM test_data", [], |r| r.get(0))
            .unwrap();

        assert_eq!(count, 1);
    }

    #[test]
    fn test_stream_wal_changes_single_write() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("main.db");
        let backup_dir = temp_dir.path().join("backup");

        let conn = create_test_db(&db_path);
        let mut streamer = WalStreamer::new(WalStreamerConfig::new(&db_path, &backup_dir)).unwrap();

        // Perform database write
        conn.execute(
            "INSERT INTO test_data (content, created_at) VALUES ('row1', 't1')",
            [],
        )
        .unwrap();

        let seg_opt = streamer.stream_wal_changes().unwrap();
        assert!(seg_opt.is_some());

        let seg = seg_opt.unwrap();
        assert_eq!(seg.sequence, 1);
        assert!(seg.file_size > 0);
        assert!(backup_dir.join(&seg.file_path).exists());
    }

    #[test]
    fn test_stream_wal_changes_no_new_data() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("main.db");
        let backup_dir = temp_dir.path().join("backup");

        let _conn = create_test_db(&db_path);
        let mut streamer = WalStreamer::new(WalStreamerConfig::new(&db_path, &backup_dir)).unwrap();

        // First stream captures initial schema changes
        let _ = streamer.stream_wal_changes().unwrap();

        // Second stream without new writes should return None
        let seg_opt = streamer.stream_wal_changes().unwrap();
        assert!(seg_opt.is_none());
    }

    #[test]
    fn test_stream_wal_changes_multiple_batches() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("main.db");
        let backup_dir = temp_dir.path().join("backup");

        let conn = create_test_db(&db_path);
        let mut streamer = WalStreamer::new(WalStreamerConfig::new(&db_path, &backup_dir)).unwrap();

        conn.execute(
            "INSERT INTO test_data (content, created_at) VALUES ('batch1', 't1')",
            [],
        )
        .unwrap();
        let seg1 = streamer.stream_wal_changes().unwrap().unwrap();

        conn.execute(
            "INSERT INTO test_data (content, created_at) VALUES ('batch2', 't2')",
            [],
        )
        .unwrap();
        let seg2 = streamer.stream_wal_changes().unwrap().unwrap();

        assert_eq!(seg1.sequence, 1);
        assert_eq!(seg2.sequence, 2);
        assert_eq!(streamer.manifest().wal_segments.len(), 2);
    }

    #[test]
    fn test_snapshot_interval_timing() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("main.db");
        let backup_dir = temp_dir.path().join("backup");

        let _conn = create_test_db(&db_path);

        let config = WalStreamerConfig::new(&db_path, &backup_dir)
            .with_snapshot_interval(Duration::from_secs(60));
        let mut streamer = WalStreamer::new(config).unwrap();

        // Initial check triggers snapshot because last_snapshot_at is None
        let snap1 = streamer.check_and_snapshot_interval().unwrap();
        assert!(snap1.is_some());

        // Immediate subsequent check should not trigger snapshot
        let snap2 = streamer.check_and_snapshot_interval().unwrap();
        assert!(snap2.is_none());

        // Manually set last_snapshot_at to past to trigger next snapshot
        streamer.last_snapshot_at = Some(std::time::SystemTime::now() - Duration::from_secs(120));

        let snap3 = streamer.check_and_snapshot_interval().unwrap();
        assert!(snap3.is_some());
    }

    #[test]
    fn test_snapshot_interval_not_due() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("main.db");
        let backup_dir = temp_dir.path().join("backup");

        let _conn = create_test_db(&db_path);

        let config = WalStreamerConfig::new(&db_path, &backup_dir)
            .with_snapshot_interval(Duration::from_secs(3600));
        let mut streamer = WalStreamer::new(config).unwrap();

        let _ = streamer.create_snapshot().unwrap();
        let snap_opt = streamer.check_and_snapshot_interval().unwrap();

        assert!(snap_opt.is_none());
    }

    #[test]
    fn test_recover_latest_snapshot_only() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("main.db");
        let backup_dir = temp_dir.path().join("backup");
        let restored_path = temp_dir.path().join("restored.db");

        let conn = create_test_db(&db_path);
        conn.execute(
            "INSERT INTO test_data (content, created_at) VALUES ('snap_item', 't0')",
            [],
        )
        .unwrap();

        let mut streamer = WalStreamer::new(WalStreamerConfig::new(&db_path, &backup_dir)).unwrap();
        let _snap = streamer.create_snapshot().unwrap();

        let report = WalStreamer::recover(&backup_dir, &restored_path, None).unwrap();

        assert_eq!(report.segments_applied, 0);
        assert_eq!(report.integrity_check, "ok");

        let res_conn = Connection::open(&restored_path).unwrap();
        let count: i64 = res_conn
            .query_row("SELECT COUNT(*) FROM test_data", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_recover_snapshot_and_wal_segments() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("main.db");
        let backup_dir = temp_dir.path().join("backup");
        let restored_path = temp_dir.path().join("restored.db");

        let conn = create_test_db(&db_path);
        conn.execute(
            "INSERT INTO test_data (content, created_at) VALUES ('initial', 't0')",
            [],
        )
        .unwrap();

        let mut streamer = WalStreamer::new(WalStreamerConfig::new(&db_path, &backup_dir)).unwrap();
        let _snap = streamer.create_snapshot().unwrap();

        // Perform additional writes after snapshot
        conn.execute(
            "INSERT INTO test_data (content, created_at) VALUES ('post_snap_1', 't1')",
            [],
        )
        .unwrap();
        let _seg1 = streamer.stream_wal_changes().unwrap();

        conn.execute(
            "INSERT INTO test_data (content, created_at) VALUES ('post_snap_2', 't2')",
            [],
        )
        .unwrap();
        let _seg2 = streamer.stream_wal_changes().unwrap();

        let report = WalStreamer::recover(&backup_dir, &restored_path, None).unwrap();

        assert!(report.segments_applied >= 1);
        assert_eq!(report.integrity_check, "ok");

        let res_conn = Connection::open(&restored_path).unwrap();
        let count: i64 = res_conn
            .query_row("SELECT COUNT(*) FROM test_data", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn test_point_in_time_recovery() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("main.db");
        let backup_dir = temp_dir.path().join("backup");
        let restored_path = temp_dir.path().join("restored.db");

        let conn = create_test_db(&db_path);
        let mut streamer = WalStreamer::new(WalStreamerConfig::new(&db_path, &backup_dir)).unwrap();

        conn.execute(
            "INSERT INTO test_data (content, created_at) VALUES ('v1', 't1')",
            [],
        )
        .unwrap();
        let _snap = streamer.create_snapshot().unwrap();
        let pit_time = SystemTime::now();

        // Subsequent write after pit_time cutoff
        std::thread::sleep(Duration::from_millis(50));
        conn.execute(
            "INSERT INTO test_data (content, created_at) VALUES ('v2_future', 't2')",
            [],
        )
        .unwrap();
        let _seg_future = streamer.stream_wal_changes().unwrap();

        let report = WalStreamer::recover_to_point(&backup_dir, &restored_path, pit_time).unwrap();

        assert_eq!(report.integrity_check, "ok");

        let res_conn = Connection::open(&restored_path).unwrap();
        let count: i64 = res_conn
            .query_row("SELECT COUNT(*) FROM test_data", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_wal_reset_checkpoint_handling() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("main.db");
        let backup_dir = temp_dir.path().join("backup");

        let conn = create_test_db(&db_path);
        let mut streamer = WalStreamer::new(WalStreamerConfig::new(&db_path, &backup_dir)).unwrap();

        conn.execute(
            "INSERT INTO test_data (content, created_at) VALUES ('row1', 't1')",
            [],
        )
        .unwrap();
        let seg1 = streamer.stream_wal_changes().unwrap().unwrap();
        assert_eq!(seg1.sequence, 1);

        // Explicitly truncate WAL file to simulate SQLite checkpoint
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .unwrap();

        conn.execute(
            "INSERT INTO test_data (content, created_at) VALUES ('row2', 't2')",
            [],
        )
        .unwrap();

        // Streamer should handle reset gracefully and capture new segment
        let seg2 = streamer.stream_wal_changes().unwrap().unwrap();
        assert_eq!(seg2.sequence, 2);
    }

    #[test]
    fn test_manifest_persistence_and_reload() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("main.db");
        let backup_dir = temp_dir.path().join("backup");

        let conn = create_test_db(&db_path);
        {
            let mut streamer =
                WalStreamer::new(WalStreamerConfig::new(&db_path, &backup_dir)).unwrap();
            let _snap = streamer.create_snapshot().unwrap();
            conn.execute(
                "INSERT INTO test_data (content, created_at) VALUES ('data', 't1')",
                [],
            )
            .unwrap();
            let _seg = streamer.stream_wal_changes().unwrap();
        }

        // Re-open streamer from existing backup directory
        let reloaded_streamer =
            WalStreamer::new(WalStreamerConfig::new(&db_path, &backup_dir)).unwrap();

        assert_eq!(reloaded_streamer.manifest().snapshots.len(), 1);
        assert_eq!(reloaded_streamer.manifest().wal_segments.len(), 1);
        assert_eq!(reloaded_streamer.manifest().current_snapshot_seq, 1);
        assert_eq!(reloaded_streamer.manifest().current_wal_seq, 1);
    }

    #[test]
    fn test_recovery_nonexistent_backup_dir() {
        let temp_dir = TempDir::new().unwrap();
        let invalid_backup_dir = temp_dir.path().join("nonexistent_backup");
        let target_path = temp_dir.path().join("target.db");

        let res = WalStreamer::recover(&invalid_backup_dir, &target_path, None);
        assert!(res.is_err());
    }

    #[test]
    fn test_verify_backup_helper() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("main.db");
        let backup_dir = temp_dir.path().join("backup");

        let conn = create_test_db(&db_path);
        let mut streamer = WalStreamer::new(WalStreamerConfig::new(&db_path, &backup_dir)).unwrap();

        let _snap = streamer.create_snapshot().unwrap();
        conn.execute(
            "INSERT INTO test_data (content, created_at) VALUES ('val', 'now')",
            [],
        )
        .unwrap();
        let _seg = streamer.stream_wal_changes().unwrap();

        let valid = WalStreamer::verify_backup(&backup_dir).unwrap();
        assert!(valid);
    }

    // --- Stress / edge-case tests (8 new) ---

    #[test]
    fn stress_concurrent_writes_multiple_threads() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("main.db");
        let backup_dir = temp_dir.path().join("backup");

        let conn = create_test_db(&db_path);
        let mut streamer = WalStreamer::new(WalStreamerConfig::new(&db_path, &backup_dir)).unwrap();

        // Spawn multiple threads each inserting rows via separate connections
        let mut handles = vec![];
        for thread_id in 0..4 {
            let db = db_path.clone();
            handles.push(std::thread::spawn(move || {
                let c = Connection::open(&db).unwrap();
                for i in 0..25 {
                    c.execute(
                        &format!(
                            "INSERT INTO test_data (content, created_at) VALUES ('t{}_r{}', 'now')",
                            thread_id, i
                        ),
                        [],
                    )
                    .unwrap();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        // Stream should capture all concurrent writes
        let seg = streamer.stream_wal_changes().unwrap();
        assert!(seg.is_some(), "Should have WAL data from concurrent writes");
        let seg = seg.unwrap();
        assert!(seg.file_size > 0);

        // Verify total row count matches what was inserted (100 rows + schema)
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM test_data", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 100);
    }

    #[test]
    fn stress_large_frame_8kb_plus() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("main.db");
        let backup_dir = temp_dir.path().join("backup");

        let conn = create_test_db(&db_path);
        let mut streamer = WalStreamer::new(WalStreamerConfig::new(&db_path, &backup_dir)).unwrap();

        // Insert a row with 16KB content — will generate a large WAL frame
        let large_content = "X".repeat(16 * 1024);
        conn.execute(
            "INSERT INTO test_data (content, created_at) VALUES (?1, 'now')",
            [&large_content as &dyn rusqlite::types::ToSql],
        )
        .unwrap();

        let seg = streamer.stream_wal_changes().unwrap().unwrap();
        // WAL segment should contain at least the large frame data
        assert!(
            seg.file_size >= 8192,
            "Expected WAL segment >= 8KB for large frame, got {} bytes",
            seg.file_size
        );

        // Verify segment file exists and is readable
        let seg_path = backup_dir.join(&seg.file_path);
        let seg_bytes = std::fs::read(&seg_path).unwrap();
        assert_eq!(seg_bytes.len() as u64, seg.file_size);
    }

    #[test]
    fn stress_frame_ordering_preserved_under_load() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("main.db");
        let backup_dir = temp_dir.path().join("backup");

        let conn = create_test_db(&db_path);
        let mut streamer = WalStreamer::new(WalStreamerConfig::new(&db_path, &backup_dir)).unwrap();

        // Write 20 sequential batches, each one streamed immediately
        let mut segments = vec![];
        for i in 0..20 {
            conn.execute(
                &format!(
                    "INSERT INTO test_data (content, created_at) VALUES ('seq_{}', 'now')",
                    i
                ),
                [],
            )
            .unwrap();
            let seg = streamer.stream_wal_changes().unwrap().unwrap();
            segments.push(seg);
        }

        // Verify monotonically increasing sequences
        for window in segments.windows(2) {
            assert!(
                window[1].sequence > window[0].sequence,
                "Sequence not monotonically increasing: {} -> {}",
                window[0].sequence,
                window[1].sequence
            );
        }
        // Verify offsets are contiguous (each segment starts where previous ended)
        for window in segments.windows(2) {
            assert_eq!(
                window[0].end_offset, window[1].start_offset,
                "Offset gap/overlap between segments: end={} start={}",
                window[0].end_offset, window[1].start_offset
            );
        }
        assert_eq!(segments.len(), 20);
    }

    #[test]
    fn stress_corrupt_truncated_wal_segment_handled() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("main.db");
        let backup_dir = temp_dir.path().join("backup");

        let conn = create_test_db(&db_path);
        let mut streamer = WalStreamer::new(WalStreamerConfig::new(&db_path, &backup_dir)).unwrap();

        conn.execute(
            "INSERT INTO test_data (content, created_at) VALUES ('before_corrupt', 't1')",
            [],
        )
        .unwrap();
        let seg1 = streamer.stream_wal_changes().unwrap().unwrap();

        // Delete the segment file — verify_backup checks file existence, not content
        let seg_path = backup_dir.join(&seg1.file_path);
        std::fs::remove_file(&seg_path).unwrap();

        // Verify backup should report false since segment file is missing
        let valid = WalStreamer::verify_backup(&backup_dir).unwrap();
        assert!(!valid, "Verify should detect missing segment file");

        // The streamer itself can still continue operating — stream new data
        conn.execute(
            "INSERT INTO test_data (content, created_at) VALUES ('after_corrupt', 't2')",
            [],
        )
        .unwrap();
        let seg2 = streamer.stream_wal_changes().unwrap();
        assert!(
            seg2.is_some(),
            "Streamer should continue after segment file deletion"
        );

        // Truncation test: truncate WAL file to smaller size to simulate checkpoint
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .unwrap();
        conn.execute(
            "INSERT INTO test_data (content, created_at) VALUES ('post_trunc', 't3')",
            [],
        )
        .unwrap();
        let seg3 = streamer.stream_wal_changes().unwrap();
        assert!(
            seg3.is_some(),
            "Streamer should handle WAL truncation gracefully"
        );
    }

    #[test]
    fn stress_cleanup_after_backup_cycle() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("main.db");
        let backup_dir = temp_dir.path().join("backup");

        let conn = create_test_db(&db_path);
        let mut streamer = WalStreamer::new(WalStreamerConfig::new(&db_path, &backup_dir)).unwrap();

        // Take snapshot (checkpoint truncates WAL), then write data and stream
        let snap = streamer.create_snapshot().unwrap();

        // Write data AFTER snapshot so WAL has new content to stream
        conn.execute(
            "INSERT INTO test_data (content, created_at) VALUES ('cycle_data', 't1')",
            [],
        )
        .unwrap();
        let seg = streamer
            .stream_wal_changes()
            .unwrap()
            .expect("Should have WAL data after post-snapshot write");

        // All files should exist and be non-empty
        let snap_path = backup_dir.join(&snap.file_path);
        let seg_path = backup_dir.join(&seg.file_path);
        let manifest_path = backup_dir.join("manifest.json");

        assert!(
            snap_path.exists(),
            "Snapshot file should exist after backup"
        );
        assert!(seg_path.exists(), "Segment file should exist after backup");
        assert!(manifest_path.exists(), "Manifest should exist after backup");

        assert!(std::fs::metadata(&snap_path).unwrap().len() > 0);
        assert!(std::fs::metadata(&seg_path).unwrap().len() > 0);

        // Manifest should be valid JSON and parseable
        let manifest = BackupManifest::load_from_file(&manifest_path).unwrap();
        assert_eq!(manifest.snapshots.len(), 1);
        assert_eq!(manifest.wal_segments.len(), 1);

        // Verify the full backup chain is valid
        assert!(WalStreamer::verify_backup(&backup_dir).unwrap());

        // After recovery, no stale WAL files should linger in target
        let target = temp_dir.path().join("restored.db");
        let report = WalStreamer::recover(&backup_dir, &target, None).unwrap();
        assert_eq!(report.integrity_check, "ok");
        let target_wal = wal_path_for_db(&target);
        // WAL should have been cleaned up after checkpoint during recovery
        assert!(
            !target_wal.exists(),
            "Target WAL should be cleaned up after recovery"
        );
    }

    #[test]
    fn stress_reconnect_resume_from_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("main.db");
        let backup_dir = temp_dir.path().join("backup");

        let conn = create_test_db(&db_path);

        // Phase 1: create streamer, take snapshot, write data, stream WAL
        {
            let mut streamer =
                WalStreamer::new(WalStreamerConfig::new(&db_path, &backup_dir)).unwrap();
            let _snap = streamer.create_snapshot().unwrap();
            // Write after snapshot so WAL has content to stream
            conn.execute(
                "INSERT INTO test_data (content, created_at) VALUES ('phase1', 't1')",
                [],
            )
            .unwrap();
            let _seg = streamer.stream_wal_changes().unwrap();
        }
        // streamer dropped — simulates process restart

        // Phase 2: reconnect with new streamer, write more data, stream again
        let mut streamer2 =
            WalStreamer::new(WalStreamerConfig::new(&db_path, &backup_dir)).unwrap();

        // Manifest should be loaded with state from phase 1
        assert_eq!(streamer2.manifest().snapshots.len(), 1);
        assert_eq!(streamer2.manifest().wal_segments.len(), 1);
        assert_eq!(streamer2.manifest().current_wal_seq, 1);
        assert!(streamer2.last_wal_offset() > 0);

        // New writes should stream from where we left off
        conn.execute(
            "INSERT INTO test_data (content, created_at) VALUES ('phase2', 't2')",
            [],
        )
        .unwrap();
        let seg = streamer2.stream_wal_changes().unwrap();
        assert!(seg.is_some(), "New streamer should pick up new WAL changes");
        let seg = seg.unwrap();
        assert_eq!(seg.sequence, 2, "Sequence should continue from phase 1");

        // Verify full backup is valid
        assert!(WalStreamer::verify_backup(&backup_dir).unwrap());
    }

    #[test]
    fn stress_empty_wal_no_changes() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("main.db");
        let backup_dir = temp_dir.path().join("backup");

        let _conn = create_test_db(&db_path);
        let mut streamer = WalStreamer::new(WalStreamerConfig::new(&db_path, &backup_dir)).unwrap();

        // Stream with no new data beyond initial schema
        let _ = streamer.stream_wal_changes().unwrap();

        // Multiple subsequent streams with no new writes should all return None
        for _ in 0..5 {
            let result = streamer.stream_wal_changes().unwrap();
            assert!(result.is_none(), "Empty WAL should return None");
        }

        // Manifest should be stable — no spurious segments created
        assert_eq!(streamer.manifest().wal_segments.len(), 1); // only initial schema segment
        let last_offset = streamer.last_wal_offset();

        // Checkpoint to truncate WAL, then stream again — should handle empty/truncated WAL
        _conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .unwrap();
        let result = streamer.stream_wal_changes().unwrap();
        // After checkpoint, WAL may be empty or smaller — streamer handles this
        // offset should be reset (file smaller than last tracked offset)
        if let Some(changes) = result.as_ref() {
            // If there's data, it should be a new segment
            assert_eq!(changes.sequence, 2);
        } else {
            // If None, offset was reset and no new data — verify offset was handled
            assert!(streamer.last_wal_offset() <= last_offset || streamer.last_wal_offset() == 0);
        }
    }

    #[test]
    fn stress_rapid_sequential_writes() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("main.db");
        let backup_dir = temp_dir.path().join("backup");

        let conn = create_test_db(&db_path);
        let mut streamer = WalStreamer::new(WalStreamerConfig::new(&db_path, &backup_dir)).unwrap();

        // Perform 50 rapid writes then stream once — should capture all in one segment
        for i in 0..50 {
            conn.execute(
                &format!(
                    "INSERT INTO test_data (content, created_at) VALUES ('rapid_{}', 'now')",
                    i
                ),
                [],
            )
            .unwrap();
        }

        let seg = streamer.stream_wal_changes().unwrap().unwrap();
        assert!(seg.file_size > 0);
        assert_eq!(seg.sequence, 1);

        // All 50 rows should be queryable
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM test_data", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 50);

        // Now test with max_segment_size limiting: force chunked streaming
        let mut chunked_streamer = WalStreamer::new(
            WalStreamerConfig::new(&db_path, &backup_dir).with_max_segment_size(256),
        )
        .unwrap();

        // More rapid writes
        for i in 50..100 {
            conn.execute(
                &format!(
                    "INSERT INTO test_data (content, created_at) VALUES ('rapid_{}', 'now')",
                    i
                ),
                [],
            )
            .unwrap();
        }

        let mut total_streamed = 0;
        let mut chunk_count = 0;
        while let Some(chunk_seg) = chunked_streamer.stream_wal_changes().unwrap() {
            total_streamed += chunk_seg.file_size;
            chunk_count += 1;
        }

        assert!(
            chunk_count >= 2,
            "Expected multiple chunks with max_segment_size=256, got {}",
            chunk_count
        );
        assert!(total_streamed > 0);

        let final_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM test_data", [], |r| r.get(0))
            .unwrap();
        assert_eq!(final_count, 100);
    }
}
