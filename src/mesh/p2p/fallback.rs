//! Local Fallback Strategy and Offline Sync Buffer
//!
//! Maintains 100% uptime for local agents when P2P synchronization or Maloca Mesh network connection is lost.
//! Buffers outbound sync events locally in SQLite and automatically replays them upon reconnection.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// Status of a synchronized event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncStatus {
    Pending,
    InFlight,
    Failed,
    Completed,
}

impl SyncStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InFlight => "in_flight",
            Self::Failed => "failed",
            Self::Completed => "completed",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "in_flight" => Self::InFlight,
            "failed" => Self::Failed,
            "completed" => Self::Completed,
            _ => Self::Pending,
        }
    }
}

pub fn current_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Outbound sync event stored in the local buffer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncEvent {
    pub id: String,
    pub event_type: String,
    pub payload: Vec<u8>,
    pub timestamp: i64,
    pub retry_count: u32,
    pub status: SyncStatus,
    pub last_error: Option<String>,
}

impl SyncEvent {
    pub fn new(event_type: impl Into<String>, payload: Vec<u8>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            event_type: event_type.into(),
            payload,
            timestamp: current_timestamp(),
            retry_count: 0,
            status: SyncStatus::Pending,
            last_error: None,
        }
    }
}

/// Summary metrics returned after a buffer replay operation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayResult {
    pub total_processed: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub remaining_pending: usize,
}

/// SQLite-backed offline sync event buffer.
#[derive(Clone)]
pub struct OfflineBuffer {
    conn: Arc<Mutex<Connection>>,
    is_connected: Arc<AtomicBool>,
    max_retries: u32,
    db_path: Option<PathBuf>,
}

impl OfflineBuffer {
    /// Create a new `OfflineBuffer` using a SQLite database file at `db_path`.
    pub fn new(db_path: impl AsRef<Path>) -> Result<Self> {
        let path = db_path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory structure for {:?}", parent))?;
        }
        let conn = Connection::open(&path)
            .with_context(|| format!("Failed to open SQLite offline buffer at {:?}", path))?;

        let buffer = Self {
            conn: Arc::new(Mutex::new(conn)),
            is_connected: Arc::new(AtomicBool::new(true)),
            max_retries: 5,
            db_path: Some(path),
        };
        buffer.init_schema()?;
        Ok(buffer)
    }

    /// Create an in-memory `OfflineBuffer` for testing or temporary operations.
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .context("Failed to open in-memory SQLite buffer")?;

        let buffer = Self {
            conn: Arc::new(Mutex::new(conn)),
            is_connected: Arc::new(AtomicBool::new(true)),
            max_retries: 5,
            db_path: None,
        };
        buffer.init_schema()?;
        Ok(buffer)
    }

    /// Reopen database connection if path exists (e.g., simulating node restart).
    pub fn reopen(&self) -> Result<Self> {
        if let Some(ref path) = self.db_path {
            Self::new(path)
        } else {
            Err(anyhow!("Cannot reopen in-memory SQLite buffer"))
        }
    }

    /// Set maximum retry attempts for failing sync events.
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Initialize SQLite table schema.
    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock poisoned: {}", e))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS offline_sync_events (
                id TEXT PRIMARY KEY,
                event_type TEXT NOT NULL,
                payload BLOB NOT NULL,
                timestamp INTEGER NOT NULL,
                retry_count INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'pending',
                last_error TEXT
            )",
            [],
        )
        .context("Failed to create offline_sync_events table")?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_offline_events_status ON offline_sync_events(status, timestamp)",
            [],
        )
        .context("Failed to create index on status/timestamp")?;

        Ok(())
    }

    /// Returns current mesh network connectivity status.
    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::SeqCst)
    }

    /// Update connection status. Returns `true` if connection state changed from false to true (reconnected).
    pub fn set_connected(&self, connected: bool) -> bool {
        let prev = self.is_connected.swap(connected, Ordering::SeqCst);
        !prev && connected
    }

    /// Enqueue a sync event into the offline buffer.
    pub fn enqueue(&self, event: SyncEvent) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock poisoned: {}", e))?;
        conn.execute(
            "INSERT INTO offline_sync_events (id, event_type, payload, timestamp, retry_count, status, last_error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                event_type = excluded.event_type,
                payload = excluded.payload,
                timestamp = excluded.timestamp,
                retry_count = excluded.retry_count,
                status = excluded.status,
                last_error = excluded.last_error",
            params![
                event.id,
                event.event_type,
                event.payload,
                event.timestamp,
                event.retry_count,
                event.status.as_str(),
                event.last_error,
            ],
        )
        .context("Failed to insert event into offline buffer")?;
        Ok(())
    }

    /// Convenience helper to create and enqueue a new sync event.
    pub fn enqueue_event(&self, event_type: impl Into<String>, payload: Vec<u8>) -> Result<SyncEvent> {
        let event = SyncEvent::new(event_type, payload);
        self.enqueue(event.clone())?;
        Ok(event)
    }

    /// Retrieve all pending or failed events ready for replay (retry_count < max_retries).
    pub fn get_pending(&self) -> Result<Vec<SyncEvent>> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock poisoned: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT id, event_type, payload, timestamp, retry_count, status, last_error
             FROM offline_sync_events
             WHERE status IN ('pending', 'failed') AND retry_count < ?1
             ORDER BY timestamp ASC",
        )?;

        let rows = stmt.query_map(params![self.max_retries], |row| {
            let status_str: String = row.get(5)?;
            Ok(SyncEvent {
                id: row.get(0)?,
                event_type: row.get(1)?,
                payload: row.get(2)?,
                timestamp: row.get(3)?,
                retry_count: row.get(4)?,
                status: SyncStatus::parse(&status_str),
                last_error: row.get(6)?,
            })
        })?;

        let mut events = Vec::new();
        for row in rows {
            events.push(row.context("Error reading sync event row")?);
        }
        Ok(events)
    }

    /// Get total count of pending events in buffer.
    pub fn pending_count(&self) -> Result<usize> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock poisoned: {}", e))?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM offline_sync_events WHERE status IN ('pending', 'failed') AND retry_count < ?1",
            params![self.max_retries],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Get total count of all events in buffer (regardless of status).
    pub fn total_count(&self) -> Result<usize> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock poisoned: {}", e))?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM offline_sync_events",
            [],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Mark an event as completed and remove it from the offline buffer.
    pub fn mark_completed(&self, event_id: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock poisoned: {}", e))?;
        conn.execute("DELETE FROM offline_sync_events WHERE id = ?1", params![event_id])?;
        Ok(())
    }

    /// Mark an event as failed and increment its retry counter.
    pub fn mark_failed(&self, event_id: &str, error_msg: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock poisoned: {}", e))?;
        conn.execute(
            "UPDATE offline_sync_events
             SET status = 'failed',
                 retry_count = retry_count + 1,
                 last_error = ?2
             WHERE id = ?1",
            params![event_id, error_msg],
        )?;
        Ok(())
    }

    /// Clear all completed events from buffer.
    pub fn clear_completed(&self) -> Result<usize> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock poisoned: {}", e))?;
        let removed = conn.execute("DELETE FROM offline_sync_events WHERE status = 'completed'", [])?;
        Ok(removed)
    }

    /// Clear all events in the offline buffer.
    pub fn clear_all(&self) -> Result<usize> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock poisoned: {}", e))?;
        let removed = conn.execute("DELETE FROM offline_sync_events", [])?;
        Ok(removed)
    }

    /// Replay all pending events through `send_fn`.
    /// Each event that succeeds is removed from the buffer; failed events have their retry count incremented.
    pub async fn replay_pending<F, Fut>(&self, send_fn: F) -> Result<ReplayResult>
    where
        F: Fn(SyncEvent) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        if !self.is_connected() {
            return Ok(ReplayResult {
                total_processed: 0,
                succeeded: 0,
                failed: 0,
                remaining_pending: self.pending_count()?,
            });
        }

        let pending = self.get_pending()?;
        let total = pending.len();
        let mut succeeded = 0;
        let mut failed = 0;

        for event in pending {
            if !self.is_connected() {
                break;
            }
            match send_fn(event.clone()).await {
                Ok(_) => {
                    self.mark_completed(&event.id)?;
                    succeeded += 1;
                }
                Err(err) => {
                    self.mark_failed(&event.id, &err.to_string())?;
                    failed += 1;
                }
            }
        }

        let remaining = self.pending_count()?;
        Ok(ReplayResult {
            total_processed: total,
            succeeded,
            failed,
            remaining_pending: remaining,
        })
    }

    /// Synchronously or asynchronously send event if connected; if network is disconnected or sending fails,
    /// transparently buffer the event locally. Returns `true` if sent directly, `false` if buffered.
    pub async fn sync_or_buffer<F, Fut>(&self, event: SyncEvent, send_fn: F) -> Result<bool>
    where
        F: Fn(SyncEvent) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        if self.is_connected() {
            match send_fn(event.clone()).await {
                Ok(_) => Ok(true),
                Err(err) => {
                    let mut err_event = event;
                    err_event.status = SyncStatus::Failed;
                    err_event.retry_count = 1;
                    err_event.last_error = Some(err.to_string());
                    self.enqueue(err_event)?;
                    Ok(false)
                }
            }
        } else {
            self.enqueue(event)?;
            Ok(false)
        }
    }
}
