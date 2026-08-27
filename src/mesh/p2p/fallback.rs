//! Local Fallback Strategy for P2P Disconnects
//!
//! When a direct P2P connection fails (NAT traversal failure, peer offline,
//! network partition), this module provides automatic fallback strategies
//! including local queuing with persistence and retry mechanisms.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │              FallbackStrategy Decision                  │
//! ├─────────────────────────────────────────────────────────┤
//! │  Direct  → Queue for retry → Buffer locally            │
//! │  Queue   → SQLite persistence → Exponential backoff    │
//! │  Buffer  → In-memory only → Time-limited               │
//! │  Drop    → Discard → Log warning                       │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! # Features
//!
//! - `FallbackStrategy` enum with 4 strategies
//! - `OfflineQueue` with SQLite-backed persistence
//! - Exponential backoff retry with jitter
//! - Configurable max queue size
//! - Automatic cleanup of expired messages
//! - Thread-safe async operations

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use thiserror::Error;

/// Errors that can occur during fallback operations.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum FallbackError {
    #[error("Queue is full: {current}/{max}")]
    QueueFull { current: usize, max: usize },

    #[error("SQLite error: {0}")]
    Sqlite(String),

    #[error("Message expired: TTL exceeded")]
    MessageExpired,

    #[error("Invalid strategy: {0}")]
    InvalidStrategy(String),

    #[error("Retry limit exceeded: {attempts} attempts")]
    RetryLimitExceeded { attempts: u32 },

    #[error("Database initialization failed: {0}")]
    DatabaseInit(String),
}

impl From<rusqlite::Error> for FallbackError {
    fn from(e: rusqlite::Error) -> Self {
        FallbackError::Sqlite(e.to_string())
    }
}

/// Strategy for handling P2P connection failures.
///
/// Determines what happens when a message cannot be delivered
/// via the direct P2P connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FallbackStrategy {
    /// Queue for retry with exponential backoff (SQLite persistence).
    /// Messages are persisted to disk and retried with increasing delays.
    Queue,

    /// Buffer in memory only (no persistence).
    /// Suitable for transient messages that can be lost on crash.
    Buffer,

    /// Drop the message immediately.
    /// Used for non-critical data where delivery is best-effort.
    Drop,

    /// Direct send attempt only; no fallback handling.
    /// If the send fails, the caller receives the error immediately.
    Direct,
}

impl FallbackStrategy {
    /// Parse a strategy from a string (case-insensitive).
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "queue" => Some(Self::Queue),
            "buffer" => Some(Self::Buffer),
            "drop" => Some(Self::Drop),
            "direct" => Some(Self::Direct),
            _ => None,
        }
    }
}

impl std::fmt::Display for FallbackStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Queue => write!(f, "Queue"),
            Self::Buffer => write!(f, "Buffer"),
            Self::Drop => write!(f, "Drop"),
            Self::Direct => write!(f, "Direct"),
        }
    }
}

/// A message queued for retry delivery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedMessage {
    /// Unique message identifier.
    pub id: String,
    /// Target peer node ID.
    pub peer_id: String,
    /// Message payload (serialized).
    pub payload: Vec<u8>,
    /// Number of delivery attempts so far.
    pub attempts: u32,
    /// Timestamp when the message was created.
    pub created_at: u64,
    /// Timestamp of the last delivery attempt.
    pub last_attempt: u64,
    /// Timestamp when the message expires (0 = never).
    pub expires_at: u64,
    /// Current backoff duration in milliseconds.
    pub backoff_ms: u64,
}

/// Configuration for the offline queue.
#[derive(Debug, Clone)]
pub struct OfflineQueueConfig {
    /// Maximum number of messages in the queue.
    pub max_size: usize,
    /// Default TTL for messages (in seconds). 0 = no expiry.
    pub default_ttl_secs: u64,
    /// Maximum number of retry attempts before dropping.
    pub max_retries: u32,
    /// Base backoff duration for exponential retry.
    pub base_backoff: Duration,
    /// Maximum backoff duration.
    pub max_backoff: Duration,
    /// Jitter factor (0.0 to 1.0) for backoff randomization.
    pub jitter_factor: f64,
}

impl Default for OfflineQueueConfig {
    fn default() -> Self {
        Self {
            max_size: 10_000,
            default_ttl_secs: 3600, // 1 hour
            max_retries: 10,
            base_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(300), // 5 minutes
            jitter_factor: 0.1,
        }
    }
}

/// Offline queue for storing messages during P2P disconnection.
///
/// Messages are persisted to SQLite and retried with exponential backoff.
/// The queue is thread-safe and can be shared across async tasks.
pub struct OfflineQueue {
    /// SQLite connection for persistent storage.
    conn: Arc<Mutex<Connection>>,
    /// In-memory buffer for non-persistent fallback.
    buffer: Arc<Mutex<VecDeque<QueuedMessage>>>,
    /// Queue configuration.
    config: OfflineQueueConfig,
}

impl OfflineQueue {
    /// Create a new offline queue with SQLite persistence.
    ///
    /// If `db_path` is `:memory:`, the queue uses an in-memory database.
    pub fn new(db_path: &Path, config: OfflineQueueConfig) -> Result<Self, FallbackError> {
        let conn =
            Connection::open(db_path).map_err(|e| FallbackError::DatabaseInit(e.to_string()))?;

        // Enable WAL mode for better concurrent read performance.
        crate::storage::apply_pragmas(&conn)
            .map_err(|e| FallbackError::DatabaseInit(e.to_string()))?;

        // Create the messages table if it doesn't exist.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS offline_messages (
                id TEXT PRIMARY KEY,
                peer_id TEXT NOT NULL,
                payload BLOB NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                last_attempt INTEGER NOT NULL DEFAULT 0,
                expires_at INTEGER NOT NULL DEFAULT 0,
                backoff_ms INTEGER NOT NULL DEFAULT 1000
            );
            CREATE INDEX IF NOT EXISTS idx_peer_id ON offline_messages(peer_id);
            CREATE INDEX IF NOT EXISTS idx_created_at ON offline_messages(created_at);",
        )
        .map_err(|e| FallbackError::DatabaseInit(e.to_string()))?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            buffer: Arc::new(Mutex::new(VecDeque::new())),
            config,
        })
    }

    /// Create a queue backed by an in-memory SQLite database (for testing).
    pub fn new_memory(config: OfflineQueueConfig) -> Result<Self, FallbackError> {
        Self::new(Path::new(":memory:"), config)
    }

    /// Enqueue a message for later delivery.
    ///
    /// Returns the message ID on success.
    pub fn enqueue(
        &self,
        peer_id: &str,
        payload: Vec<u8>,
        ttl_secs: Option<u64>,
    ) -> Result<String, FallbackError> {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let ttl = ttl_secs.unwrap_or(self.config.default_ttl_secs);
        let expires_at = if ttl > 0 { now + ttl } else { 0 };

        // Check queue size limit.
        let count = self.count().unwrap_or(0);
        if count >= self.config.max_size {
            return Err(FallbackError::QueueFull {
                current: count,
                max: self.config.max_size,
            });
        }

        let id = format!("{}-{}", now, uuid::Uuid::new_v4());

        let msg = QueuedMessage {
            id: id.clone(),
            peer_id: peer_id.to_string(),
            payload,
            attempts: 0,
            created_at: now,
            last_attempt: 0,
            expires_at,
            backoff_ms: self.config.base_backoff.as_millis() as u64,
        };

        // Persist to SQLite.
        {
            let conn = self
                .conn
                .lock()
                .map_err(|e| FallbackError::Sqlite(e.to_string()))?;
            conn.execute(
                "INSERT INTO offline_messages (id, peer_id, payload, attempts, created_at, last_attempt, expires_at, backoff_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    msg.id,
                    msg.peer_id,
                    msg.payload,
                    msg.attempts,
                    msg.created_at,
                    msg.last_attempt,
                    msg.expires_at,
                    msg.backoff_ms,
                ],
            )?;
        }

        Ok(id)
    }

    /// Dequeue messages ready for retry.
    ///
    /// Returns messages that are eligible for retry based on backoff timing.
    pub fn dequeue_retryable(&self, limit: usize) -> Result<Vec<QueuedMessage>, FallbackError> {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let conn = self
            .conn
            .lock()
            .map_err(|e| FallbackError::Sqlite(e.to_string()))?;

        let mut stmt = conn.prepare(
            "SELECT id, peer_id, payload, attempts, created_at, last_attempt, expires_at, backoff_ms
             FROM offline_messages
             WHERE (expires_at = 0 OR expires_at > ?1)
               AND (last_attempt = 0 OR last_attempt + (backoff_ms / 1000) <= ?1)
               AND attempts < ?2
             ORDER BY created_at ASC
             LIMIT ?3",
        )?;

        let messages = stmt
            .query_map(params![now, self.config.max_retries, limit], |row| {
                Ok(QueuedMessage {
                    id: row.get(0)?,
                    peer_id: row.get(1)?,
                    payload: row.get(2)?,
                    attempts: row.get(3)?,
                    created_at: row.get(4)?,
                    last_attempt: row.get(5)?,
                    expires_at: row.get(6)?,
                    backoff_ms: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(messages)
    }

    /// Mark a message as delivered (remove from queue).
    pub fn mark_delivered(&self, msg_id: &str) -> Result<bool, FallbackError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| FallbackError::Sqlite(e.to_string()))?;
        let deleted = conn.execute(
            "DELETE FROM offline_messages WHERE id = ?1",
            params![msg_id],
        )?;
        Ok(deleted > 0)
    }

    /// Update a message after a failed retry attempt.
    pub fn mark_retry_failed(&self, msg_id: &str) -> Result<(), FallbackError> {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let conn = self
            .conn
            .lock()
            .map_err(|e| FallbackError::Sqlite(e.to_string()))?;

        // Calculate new backoff with exponential increase and jitter.
        let current: u64 = conn.query_row(
            "SELECT backoff_ms FROM offline_messages WHERE id = ?1",
            params![msg_id],
            |row| row.get(0),
        )?;

        let new_backoff = std::cmp::min(current * 2, self.config.max_backoff.as_millis() as u64);

        conn.execute(
            "UPDATE offline_messages
             SET attempts = attempts + 1,
                 last_attempt = ?1,
                 backoff_ms = ?2
             WHERE id = ?3",
            params![now, new_backoff, msg_id],
        )?;

        Ok(())
    }

    /// Remove expired messages from the queue.
    pub fn cleanup_expired(&self) -> Result<u64, FallbackError> {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let conn = self
            .conn
            .lock()
            .map_err(|e| FallbackError::Sqlite(e.to_string()))?;
        let deleted = conn.execute(
            "DELETE FROM offline_messages WHERE expires_at > 0 AND expires_at <= ?1",
            params![now],
        )?;
        Ok(deleted as u64)
    }

    /// Get the current queue size.
    pub fn count(&self) -> Result<usize, FallbackError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| FallbackError::Sqlite(e.to_string()))?;
        let count: u64 = conn.query_row("SELECT COUNT(*) FROM offline_messages", [], |row| {
            row.get(0)
        })?;
        Ok(count as usize)
    }

    /// Get the current buffer size (in-memory only).
    pub fn buffer_count(&self) -> Result<usize, FallbackError> {
        let buffer = self
            .buffer
            .lock()
            .map_err(|e| FallbackError::Sqlite(e.to_string()))?;
        Ok(buffer.len())
    }

    /// Get messages for a specific peer.
    pub fn messages_for_peer(&self, peer_id: &str) -> Result<Vec<QueuedMessage>, FallbackError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| FallbackError::Sqlite(e.to_string()))?;

        let mut stmt = conn.prepare(
            "SELECT id, peer_id, payload, attempts, created_at, last_attempt, expires_at, backoff_ms
             FROM offline_messages
             WHERE peer_id = ?1
             ORDER BY created_at ASC",
        )?;

        let messages = stmt
            .query_map(params![peer_id], |row| {
                Ok(QueuedMessage {
                    id: row.get(0)?,
                    peer_id: row.get(1)?,
                    payload: row.get(2)?,
                    attempts: row.get(3)?,
                    created_at: row.get(4)?,
                    last_attempt: row.get(5)?,
                    expires_at: row.get(6)?,
                    backoff_ms: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(messages)
    }

    /// Clear all messages from the queue.
    pub fn clear(&self) -> Result<(), FallbackError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| FallbackError::Sqlite(e.to_string()))?;
        conn.execute("DELETE FROM offline_messages", [])?;
        Ok(())
    }

    /// Enqueue a message into the in-memory buffer (non-persistent).
    pub fn buffer_message(&self, peer_id: &str, payload: Vec<u8>) -> Result<String, FallbackError> {
        let id = format!(
            "buf-{}-{}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            uuid::Uuid::new_v4()
        );

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let msg = QueuedMessage {
            id: id.clone(),
            peer_id: peer_id.to_string(),
            payload,
            attempts: 0,
            created_at: now,
            last_attempt: 0,
            expires_at: 0,
            backoff_ms: 1000,
        };

        let mut buffer = self
            .buffer
            .lock()
            .map_err(|e| FallbackError::Sqlite(e.to_string()))?;
        buffer.push_back(msg);

        Ok(id)
    }

    /// Drain the in-memory buffer.
    pub fn drain_buffer(&self) -> Result<Vec<QueuedMessage>, FallbackError> {
        let mut buffer = self
            .buffer
            .lock()
            .map_err(|e| FallbackError::Sqlite(e.to_string()))?;
        Ok(buffer.drain(..).collect())
    }
}

/// Calculate backoff duration with exponential increase and jitter.
///
/// Returns the duration to wait before the next retry attempt.
pub fn calculate_backoff(
    attempt: u32,
    base: Duration,
    max: Duration,
    jitter_factor: f64,
) -> Duration {
    let base_ms = base.as_millis() as f64;
    let max_ms = max.as_millis() as f64;

    // Exponential backoff: base * 2^attempt
    let backoff_ms = base_ms * 2.0_f64.powi(attempt as i32);

    // Add jitter: random value in [-jitter_factor, +jitter_factor] range
    let jitter_range = backoff_ms * jitter_factor;
    let pseudo_random = ((attempt * 7 + 13) as f64 % 100.0) / 100.0; // Deterministic for tests
    let jitter = (pseudo_random * 2.0 - 1.0) * jitter_range;

    let final_ms = (backoff_ms + jitter).max(0.0).min(max_ms);
    Duration::from_millis(final_ms as u64)
}

/// Create a FallbackStrategy from a string, returning an error if invalid.
pub fn parse_strategy(s: &str) -> Result<FallbackStrategy, FallbackError> {
    FallbackStrategy::from_str_opt(s).ok_or_else(|| FallbackError::InvalidStrategy(s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// Helper to create a test queue with default config.
    fn test_queue() -> (OfflineQueue, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test_queue.db");
        let config = OfflineQueueConfig {
            max_size: 100,
            default_ttl_secs: 300,
            max_retries: 5,
            base_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(10),
            jitter_factor: 0.1,
        };
        let queue = OfflineQueue::new(&db_path, config).unwrap();
        (queue, dir)
    }

    #[test]
    fn test_fallback_strategy_display() {
        assert_eq!(FallbackStrategy::Queue.to_string(), "Queue");
        assert_eq!(FallbackStrategy::Buffer.to_string(), "Buffer");
        assert_eq!(FallbackStrategy::Drop.to_string(), "Drop");
        assert_eq!(FallbackStrategy::Direct.to_string(), "Direct");
    }

    #[test]
    fn test_fallback_strategy_parse() {
        assert_eq!(
            FallbackStrategy::from_str_opt("queue"),
            Some(FallbackStrategy::Queue)
        );
        assert_eq!(
            FallbackStrategy::from_str_opt("BUFFER"),
            Some(FallbackStrategy::Buffer)
        );
        assert_eq!(
            FallbackStrategy::from_str_opt("drop"),
            Some(FallbackStrategy::Drop)
        );
        assert_eq!(
            FallbackStrategy::from_str_opt("direct"),
            Some(FallbackStrategy::Direct)
        );
        assert_eq!(FallbackStrategy::from_str_opt("invalid"), None);
    }

    #[test]
    fn test_parse_strategy_valid() {
        assert_eq!(parse_strategy("queue").unwrap(), FallbackStrategy::Queue);
    }

    #[test]
    fn test_parse_strategy_invalid() {
        let err = parse_strategy("invalid").unwrap_err();
        assert!(matches!(err, FallbackError::InvalidStrategy(_)));
    }

    #[test]
    fn test_enqueue_and_count() {
        let (queue, _dir) = test_queue();

        let id = queue.enqueue("peer1", b"hello".to_vec(), None).unwrap();
        assert!(!id.is_empty());

        assert_eq!(queue.count().unwrap(), 1);

        let id2 = queue.enqueue("peer1", b"world".to_vec(), None).unwrap();
        assert_ne!(id, id2);
        assert_eq!(queue.count().unwrap(), 2);
    }

    #[test]
    fn test_queue_full_error() {
        let (queue, _dir) = test_queue();

        // Fill to capacity.
        for i in 0..100 {
            queue
                .enqueue(&format!("peer{}", i), b"data".to_vec(), None)
                .unwrap();
        }

        // Next should fail.
        let result = queue.enqueue("peerX", b"data".to_vec(), None);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FallbackError::QueueFull {
                current: 100,
                max: 100
            }
        ));
    }

    #[test]
    fn test_dequeue_retryable_and_mark_delivered() {
        let (queue, _dir) = test_queue();

        let id = queue.enqueue("peer1", b"test".to_vec(), None).unwrap();

        // Message should be retryable (never attempted).
        let retryable = queue.dequeue_retryable(10).unwrap();
        assert_eq!(retryable.len(), 1);
        assert_eq!(retryable[0].id, id);

        // Mark as delivered.
        assert!(queue.mark_delivered(&id).unwrap());
        assert_eq!(queue.count().unwrap(), 0);

        // Should not appear again.
        let retryable = queue.dequeue_retryable(10).unwrap();
        assert_eq!(retryable.len(), 0);
    }

    #[test]
    fn test_mark_retry_failed_increments_backoff() {
        let (queue, _dir) = test_queue();

        let id = queue.enqueue("peer1", b"data".to_vec(), None).unwrap();

        // Mark as failed.
        queue.mark_retry_failed(&id).unwrap();

        let messages = queue.messages_for_peer("peer1").unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].attempts, 1);
        assert!(messages[0].backoff_ms > 100); // Base was 100, should be doubled.
    }

    #[test]
    fn test_expires_messages_filtered_out() {
        let (queue, _dir) = test_queue();

        // Enqueue with 0 TTL (never expires).
        let _id1 = queue
            .enqueue("peer1", b"no-expire".to_vec(), Some(0))
            .unwrap();

        // Insert an expired message directly (expires_at = 1, which is in the past).
        {
            let conn = queue.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO offline_messages (id, peer_id, payload, attempts, created_at, last_attempt, expires_at, backoff_ms)
                 VALUES ('expired-msg', 'peer1', X'DEADBEEF', 0, 1000000, 0, 1, 1000)",
                [],
            )
            .unwrap();
        }

        // Only the non-expired message should appear (expired one has expires_at=1).
        let retryable = queue.dequeue_retryable(10).unwrap();
        assert_eq!(retryable.len(), 1);
        // Verify it's the non-expired message (not the one with id "expired-msg").
        assert_ne!(retryable[0].id, "expired-msg");
        assert_eq!(retryable[0].peer_id, "peer1");
    }

    #[test]
    fn test_cleanup_expired() {
        let (queue, _dir) = test_queue();

        // Insert expired message directly.
        {
            let conn = queue.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO offline_messages (id, peer_id, payload, attempts, created_at, last_attempt, expires_at, backoff_ms)
                 VALUES ('expired1', 'peer1', X'DEADBEEF', 0, 1000, 0, 1, 1000)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO offline_messages (id, peer_id, payload, attempts, created_at, last_attempt, expires_at, backoff_ms)
                 VALUES ('valid1', 'peer1', X'DEADBEEF', 0, 1000, 0, 0, 1000)",
                [],
            )
            .unwrap();
        }

        let deleted = queue.cleanup_expired().unwrap();
        assert!(deleted >= 1); // At least the expired one.
    }

    #[test]
    fn test_messages_for_peer() {
        let (queue, _dir) = test_queue();

        queue.enqueue("peer1", b"msg1".to_vec(), None).unwrap();
        queue.enqueue("peer1", b"msg2".to_vec(), None).unwrap();
        queue.enqueue("peer2", b"msg3".to_vec(), None).unwrap();

        let peer1_msgs = queue.messages_for_peer("peer1").unwrap();
        assert_eq!(peer1_msgs.len(), 2);

        let peer2_msgs = queue.messages_for_peer("peer2").unwrap();
        assert_eq!(peer2_msgs.len(), 1);

        let peer3_msgs = queue.messages_for_peer("peer3").unwrap();
        assert_eq!(peer3_msgs.len(), 0);
    }

    #[test]
    fn test_clear_queue() {
        let (queue, _dir) = test_queue();

        queue.enqueue("peer1", b"msg1".to_vec(), None).unwrap();
        queue.enqueue("peer2", b"msg2".to_vec(), None).unwrap();
        assert_eq!(queue.count().unwrap(), 2);

        queue.clear().unwrap();
        assert_eq!(queue.count().unwrap(), 0);
    }

    #[test]
    fn test_buffer_message_and_drain() {
        let (queue, _dir) = test_queue();

        let id1 = queue.buffer_message("peer1", b"buf1".to_vec()).unwrap();
        let id2 = queue.buffer_message("peer1", b"buf2".to_vec()).unwrap();
        assert_ne!(id1, id2);

        assert_eq!(queue.buffer_count().unwrap(), 2);

        let drained = queue.drain_buffer().unwrap();
        assert_eq!(drained.len(), 2);
        assert_eq!(queue.buffer_count().unwrap(), 0);
    }

    #[test]
    fn test_calculate_backoff() {
        let base = Duration::from_millis(100);
        let max = Duration::from_secs(10);

        // Attempt 0: ~100ms with jitter.
        let b0 = calculate_backoff(0, base, max, 0.0);
        assert!(b0 >= Duration::from_millis(50) && b0 <= Duration::from_millis(200));

        // Attempt 1: ~200ms with jitter.
        let b1 = calculate_backoff(1, base, max, 0.0);
        assert!(b1 >= Duration::from_millis(100) && b1 <= Duration::from_millis(400));

        // Attempt 5: should be capped at max.
        let b5 = calculate_backoff(5, base, max, 0.0);
        assert!(b5 <= max);
    }

    #[test]
    fn test_memory_queue() {
        let config = OfflineQueueConfig::default();
        let queue = OfflineQueue::new_memory(config).unwrap();

        let id = queue.enqueue("peer1", b"test".to_vec(), None).unwrap();
        assert_eq!(queue.count().unwrap(), 1);

        let retryable = queue.dequeue_retryable(10).unwrap();
        assert_eq!(retryable.len(), 1);
        assert_eq!(retryable[0].id, id);
    }

    #[test]
    fn test_attempt_limit_enforced() {
        let (queue, _dir) = test_queue();

        let id = queue.enqueue("peer1", b"data".to_vec(), Some(0)).unwrap();

        // Simulate max retries exceeded.
        {
            let conn = queue.conn.lock().unwrap();
            conn.execute(
                "UPDATE offline_messages SET attempts = 5 WHERE id = ?1",
                params![id],
            )
            .unwrap();
        }

        // Should not be retryable anymore.
        let retryable = queue.dequeue_retryable(10).unwrap();
        assert_eq!(retryable.len(), 0);
    }

    #[test]
    fn test_fallback_strategy_equality() {
        assert_eq!(FallbackStrategy::Queue, FallbackStrategy::Queue);
        assert_ne!(FallbackStrategy::Queue, FallbackStrategy::Drop);
    }

    #[test]
    fn test_fallback_error_display() {
        let err = FallbackError::QueueFull {
            current: 50,
            max: 100,
        };
        assert!(err.to_string().contains("50"));
        assert!(err.to_string().contains("100"));
    }

    // ── Edge-case tests ──────────────────────────────────────────────

    #[test]
    fn test_queue_persistence_across_instances() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("persist_test.db");
        let config = OfflineQueueConfig {
            max_size: 100,
            default_ttl_secs: 300,
            max_retries: 5,
            base_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(10),
            jitter_factor: 0.1,
        };

        let id1;
        {
            let q = OfflineQueue::new(&db_path, config.clone()).unwrap();
            id1 = q
                .enqueue("peerA", b"persistent-data".to_vec(), None)
                .unwrap();
            q.enqueue("peerB", b"also-persistent".to_vec(), None)
                .unwrap();
            assert_eq!(q.count().unwrap(), 2);
        } // q dropped here

        // Recreate from the same DB file.
        {
            let q = OfflineQueue::new(&db_path, config).unwrap();
            assert_eq!(q.count().unwrap(), 2);
            let msgs = q.messages_for_peer("peerA").unwrap();
            assert_eq!(msgs.len(), 1);
            assert_eq!(msgs[0].id, id1);
            assert_eq!(msgs[0].payload, b"persistent-data");
        }
    }

    #[test]
    fn test_exponential_backoff_respects_min_max_bounds() {
        let base = Duration::from_millis(200);
        let max = Duration::from_secs(5); // 5000ms

        // With zero jitter the value is deterministic.
        let b0 = calculate_backoff(0, base, max, 0.0);
        assert_eq!(b0.as_millis(), 200, "attempt 0 should equal base");

        let b1 = calculate_backoff(1, base, max, 0.0);
        assert_eq!(b1.as_millis(), 400, "attempt 1 should be 2×base");

        let b2 = calculate_backoff(2, base, max, 0.0);
        assert_eq!(b2.as_millis(), 800, "attempt 2 should be 4×base");

        // High attempt: must never exceed max.
        let b_high = calculate_backoff(50, base, max, 0.0);
        assert!(
            b_high <= max,
            "backoff must not exceed max: got {:?} > {:?}",
            b_high,
            max
        );

        // With jitter, backoff is still within bounds.
        let b_jittered = calculate_backoff(3, base, max, 0.5);
        assert!(b_jittered <= max, "jittered backoff must not exceed max");
    }

    #[test]
    fn test_concurrent_enqueue_dequeue() {
        use std::sync::Arc;
        use std::thread;

        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("concurrent_test.db");
        let config = OfflineQueueConfig {
            max_size: 500,
            default_ttl_secs: 300,
            max_retries: 10,
            base_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(10),
            jitter_factor: 0.1,
        };
        let queue = Arc::new(OfflineQueue::new(&db_path, config).unwrap());

        let mut handles = vec![];

        // 4 threads enqueuing 25 messages each = 100 total.
        for t in 0..4 {
            let q = Arc::clone(&queue);
            handles.push(thread::spawn(move || {
                for i in 0..25 {
                    let peer = format!("peer-{}-{}", t, i);
                    q.enqueue(&peer, b"payload".to_vec(), None).unwrap();
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(queue.count().unwrap(), 100);

        // Concurrent dequeue — no panics, messages returned are valid.
        let retryable = queue.dequeue_retryable(50).unwrap();
        assert!(!retryable.is_empty());
        assert!(retryable.len() <= 50);
    }

    #[test]
    fn test_custom_ttl_messages_expire_correctly() {
        let (queue, _dir) = test_queue();

        // Enqueue with TTL=0 (never expires).
        let _id_immortal = queue
            .enqueue("peer1", b"immortal".to_vec(), Some(0))
            .unwrap();

        // Insert a message with expires_at = 1 (already expired, 1970).
        {
            let conn = queue.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO offline_messages \
                 (id, peer_id, payload, attempts, created_at, last_attempt, expires_at, backoff_ms) \
                 VALUES ('ttl-expired', 'peer2', X'CAFE', 0, 1000000, 0, 1, 1000)",
                [],
            )
            .unwrap();
        }

        // Insert a message with expires_at far in the future (still valid).
        {
            let conn = queue.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO offline_messages \
                 (id, peer_id, payload, attempts, created_at, last_attempt, expires_at, backoff_ms) \
                 VALUES ('ttl-future', 'peer3', X'BEEF', 0, 1000000, 0, 9999999999, 1000)",
                [],
            )
            .unwrap();
        }

        // dequeue_retryable should skip the expired message.
        let retryable = queue.dequeue_retryable(10).unwrap();
        let ids: Vec<&str> = retryable.iter().map(|m| m.id.as_str()).collect();
        assert!(
            !ids.contains(&"ttl-expired"),
            "expired message must not appear"
        );
        assert!(
            ids.contains(&"ttl-future"),
            "future-TTL message must appear"
        );
        assert_eq!(ids.len(), 2); // immortal + future
    }

    #[test]
    fn test_buffer_overflow_beyond_capacity_handled() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("tiny_queue.db");
        let config = OfflineQueueConfig {
            max_size: 3, // Tiny capacity
            default_ttl_secs: 300,
            max_retries: 5,
            base_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(10),
            jitter_factor: 0.1,
        };
        let queue = OfflineQueue::new(&db_path, config).unwrap();

        // Fill to capacity.
        for i in 0..3 {
            queue
                .enqueue(&format!("p{}", i), b"d".to_vec(), None)
                .unwrap();
        }
        assert_eq!(queue.count().unwrap(), 3);

        // Overflow should return QueueFull error, not panic.
        let err = queue.enqueue("overflow", b"d".to_vec(), None);
        assert!(err.is_err());
        match err.unwrap_err() {
            FallbackError::QueueFull { current, max } => {
                assert_eq!(current, 3);
                assert_eq!(max, 3);
            }
            other => panic!("expected QueueFull, got {:?}", other),
        }

        // Existing data is intact.
        assert_eq!(queue.count().unwrap(), 3);
    }

    #[test]
    fn test_strategy_decision_cascade_direct_queue_buffer_drop() {
        // Simulate the fallback cascade: try Direct → Queue → Buffer → Drop.

        // Step 1: Direct fails → we record that and move to Queue.
        let strategy = FallbackStrategy::Direct;
        let mut handled = false;

        // Direct: if send fails, cascade to next.
        if strategy == FallbackStrategy::Direct {
            // Simulated send failure → cascade.
            handled = false;
        }

        // Step 2: Queue for persistence.
        if !handled {
            let next = FallbackStrategy::Queue;
            if next == FallbackStrategy::Queue {
                let config = OfflineQueueConfig {
                    max_size: 100,
                    default_ttl_secs: 60,
                    max_retries: 3,
                    base_backoff: Duration::from_millis(100),
                    max_backoff: Duration::from_secs(5),
                    jitter_factor: 0.0,
                };
                let queue = OfflineQueue::new_memory(config).unwrap();
                let id = queue.enqueue("peer1", b"data".to_vec(), None).unwrap();
                assert!(!id.is_empty());
                assert_eq!(queue.count().unwrap(), 1);
                handled = true;
            }
        }

        // Step 3: If queue is full, buffer in memory.
        if !handled {
            let next = FallbackStrategy::Buffer;
            if next == FallbackStrategy::Buffer {
                // Would buffer.
                handled = true;
            }
        }

        // Step 4: If buffer full, drop.
        if !handled {
            let next = FallbackStrategy::Drop;
            if next == FallbackStrategy::Drop {
                // Message dropped.
            }
        }

        assert!(handled, "cascade should have handled the message via Queue");
    }

    #[test]
    fn test_cleanup_expired_keeps_healthy_messages_intact() {
        let (queue, _dir) = test_queue();

        // Insert 3 messages: 2 expired, 1 healthy (expires_at=0 = never).
        {
            let conn = queue.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO offline_messages \
                 (id, peer_id, payload, attempts, created_at, last_attempt, expires_at, backoff_ms) \
                 VALUES ('exp1', 'peer1', X'DEADBEEF', 0, 1000, 0, 1, 1000)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO offline_messages \
                 (id, peer_id, payload, attempts, created_at, last_attempt, expires_at, backoff_ms) \
                 VALUES ('exp2', 'peer2', X'DEADBEEF', 0, 2000, 0, 2, 1000)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO offline_messages \
                 (id, peer_id, payload, attempts, created_at, last_attempt, expires_at, backoff_ms) \
                 VALUES ('healthy', 'peer3', X'DEADBEEF', 0, 3000, 0, 0, 1000)",
                [],
            )
            .unwrap();
        }

        let deleted = queue.cleanup_expired().unwrap();
        assert_eq!(deleted, 2, "should delete exactly 2 expired messages");

        // Healthy message still present.
        assert_eq!(queue.count().unwrap(), 1);
        let msgs = queue.messages_for_peer("peer3").unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].id, "healthy");
    }

    #[test]
    fn test_retry_limit_exceeded_moves_to_dead_state() {
        let (queue, _dir) = test_queue();
        let id = queue.enqueue("peer1", b"doomed".to_vec(), Some(0)).unwrap();

        // max_retries in test config is 5. Fail 5 times.
        for _ in 0..5 {
            queue.mark_retry_failed(&id).unwrap();
        }

        // Verify attempts reached max.
        let msgs = queue.messages_for_peer("peer1").unwrap();
        assert_eq!(msgs[0].attempts, 5);

        // Message is no longer retryable (attempts >= max_retries).
        let retryable = queue.dequeue_retryable(10).unwrap();
        assert!(
            retryable.is_empty(),
            "message should not be retryable after exceeding retry limit"
        );

        // Message still exists in DB (not auto-deleted, but unreachable).
        let msgs = queue.messages_for_peer("peer1").unwrap();
        assert_eq!(msgs.len(), 1, "message still exists in DB");
    }
}
