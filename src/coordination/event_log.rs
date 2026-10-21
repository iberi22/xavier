//! Durable append-only log for the coordination event bus
//!
//! A subscriber task mirrors every `XavierEvent` published on the in-memory
//! broadcast bus into SQLite so agents can replay what they missed after a
//! restart. Publishers are unchanged. Retention is bounded by age and rows.
use crate::coordination::events::{XavierEvent, XavierEventBus};
use crate::espacio::keys::{KeyHandle, DEFAULT_SPACE_ID};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;

/// Associated data kind string bound into the payload seal.
pub const EVENT_PAYLOAD_KIND: &str = "bus.event.payload";

/// Default retention: 7 days.
pub const DEFAULT_MAX_AGE_DAYS: i64 = 7;
/// Default retention: 200k rows.
pub const DEFAULT_MAX_ROWS: i64 = 200_000;
/// How often retention is enforced by the background task.
pub const PRUNE_INTERVAL: Duration = Duration::from_secs(3600);
/// Replay page size limits.
pub const DEFAULT_REPLAY_LIMIT: usize = 100;
pub const MAX_REPLAY_LIMIT: usize = 1000;

/// Retention policy for the event log.
#[derive(Debug, Clone, Copy)]
pub struct RetentionConfig {
    pub max_age_days: i64,
    pub max_rows: i64,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            max_age_days: DEFAULT_MAX_AGE_DAYS,
            max_rows: DEFAULT_MAX_ROWS,
        }
    }
}

impl RetentionConfig {
    /// Reads `XAVIER_EVENT_LOG_MAX_AGE_DAYS` and `XAVIER_EVENT_LOG_MAX_ROWS`.
    pub fn from_env() -> Self {
        let default = Self::default();
        let read = |key: &str, fallback: i64| {
            std::env::var(key)
                .ok()
                .and_then(|v| v.trim().parse::<i64>().ok())
                .filter(|v| *v > 0)
                .unwrap_or(fallback)
        };
        Self {
            max_age_days: read("XAVIER_EVENT_LOG_MAX_AGE_DAYS", default.max_age_days),
            max_rows: read("XAVIER_EVENT_LOG_MAX_ROWS", default.max_rows),
        }
    }
}

/// One persisted event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoggedEvent {
    pub id: i64,
    /// Unix milliseconds.
    pub ts: i64,
    pub kind: String,
    pub session_id: Option<String>,
    pub payload: serde_json::Value,
}

/// SQLite-backed append-only event log.
pub struct EventLog {
    conn: Mutex<Connection>,
    retention: RetentionConfig,
    injected_key: Option<KeyHandle>,
}

impl EventLog {
    /// Canonical db path under a state directory.
    pub fn db_path(state_dir: &Path) -> PathBuf {
        state_dir.join("events").join("bus_events.sqlite3")
    }

    /// Open (creating if needed) the log in `state_dir`.
    pub fn open(state_dir: &Path, retention: RetentionConfig) -> rusqlite::Result<Arc<Self>> {
        Self::open_with_key(state_dir, retention, None)
    }

    /// Open (creating if needed) the log in `state_dir` with an injected key handle.
    pub fn open_with_key(
        state_dir: &Path,
        retention: RetentionConfig,
        key: Option<KeyHandle>,
    ) -> rusqlite::Result<Arc<Self>> {
        let path = Self::db_path(state_dir);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let _ = conn.pragma_update(None, "synchronous", "NORMAL");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS bus_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts INTEGER NOT NULL,
                kind TEXT NOT NULL,
                session_id TEXT,
                payload_json TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_bus_events_ts ON bus_events(ts);",
        )?;
        // Retention runs from the background task, never on the boot path.
        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
            retention,
            injected_key: key,
        }))
    }

    /// Attach an explicit key handle to this event log.
    pub fn with_key(mut self, key: KeyHandle) -> Self {
        self.injected_key = Some(key);
        self
    }

    /// Resolve the sealing key: injected key if set, then default-space key if keystore exists,
    /// or None if no keystore exists. Fails closed if keystore exists but cannot be opened.
    fn resolve_key(&self) -> rusqlite::Result<Option<KeyHandle>> {
        if let Some(ref k) = self.injected_key {
            return Ok(Some(k.clone()));
        }
        use crate::memory::sqlite_vec_store::at_rest;
        if !at_rest::default_keystore_exists() {
            return Ok(None);
        }
        at_rest::default_space_key().map(Some).map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("event log: default-space key unavailable: {e}"),
            )))
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Append one event; returns its id.
    pub fn append(
        &self,
        kind: &str,
        session_id: Option<&str>,
        payload: &serde_json::Value,
    ) -> rusqlite::Result<i64> {
        let key = self.resolve_key()?;
        self.append_with_key(kind, session_id, payload, key.as_ref())
    }

    /// Append one event with an explicit key (`None` writes legacy clear).
    pub fn append_with_key(
        &self,
        kind: &str,
        session_id: Option<&str>,
        payload: &serde_json::Value,
        key: Option<&KeyHandle>,
    ) -> rusqlite::Result<i64> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let now_ms = Utc::now().timestamp_millis();
        let payload_str = payload.to_string();

        let id = match key {
            Some(dek) => {
                tx.execute(
                    "INSERT INTO bus_events (ts, kind, session_id, payload_json) VALUES (?1, ?2, ?3, '')",
                    params![now_ms, kind, session_id],
                )?;
                let row_id = tx.last_insert_rowid();
                let sealed = dek
                    .seal_record(
                        DEFAULT_SPACE_ID,
                        EVENT_PAYLOAD_KIND,
                        &row_id.to_string(),
                        &payload_str,
                    )
                    .map_err(|e| {
                        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(
                            format!("failed to seal event payload: {e}"),
                        )))
                    })?;
                tx.execute(
                    "UPDATE bus_events SET payload_json = ?1 WHERE id = ?2",
                    params![sealed, row_id],
                )?;
                row_id
            }
            None => {
                tx.execute(
                    "INSERT INTO bus_events (ts, kind, session_id, payload_json) VALUES (?1, ?2, ?3, ?4)",
                    params![now_ms, kind, session_id, payload_str],
                )?;
                tx.last_insert_rowid()
            }
        };

        tx.commit()?;
        Ok(id)
    }

    fn query(
        &self,
        where_clause: &str,
        bound: i64,
        limit: usize,
    ) -> rusqlite::Result<Vec<LoggedEvent>> {
        let key = self.resolve_key()?;
        self.query_with_key(where_clause, bound, limit, key.as_ref())
    }

    /// Query events with an explicit key (`None` opens clear rows only).
    pub fn query_with_key(
        &self,
        where_clause: &str,
        bound: i64,
        limit: usize,
        key: Option<&KeyHandle>,
    ) -> rusqlite::Result<Vec<LoggedEvent>> {
        let limit = limit.clamp(1, MAX_REPLAY_LIMIT) as i64;
        let conn = self.conn();
        let sql = format!(
            "SELECT id, ts, kind, session_id, payload_json FROM bus_events \
             WHERE {where_clause} > ?1 ORDER BY id ASC LIMIT ?2"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![bound, limit], |row| {
            let id: i64 = row.get(0)?;
            let ts: i64 = row.get(1)?;
            let kind: String = row.get(2)?;
            let session_id: Option<String> = row.get(3)?;
            let raw: String = row.get(4)?;

            let plain = if crate::espacio::keys::looks_encrypted(&raw) {
                let dek = key.ok_or_else(|| {
                    rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            format!("event {id} is sealed but no decryption key is available"),
                        )),
                    )
                })?;
                dek.open_record(DEFAULT_SPACE_ID, EVENT_PAYLOAD_KIND, &id.to_string(), &raw)
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Text,
                            Box::new(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!("failed to open sealed event {id}: {e}"),
                            )),
                        )
                    })?
            } else {
                raw
            };

            let payload = serde_json::from_str(&plain).unwrap_or(serde_json::Value::Null);
            Ok(LoggedEvent {
                id,
                ts,
                kind,
                session_id,
                payload,
            })
        })?;
        rows.collect()
    }

    /// Events with `id > since_id`, oldest first.
    pub fn since_id(&self, since_id: i64, limit: usize) -> rusqlite::Result<Vec<LoggedEvent>> {
        let key = self.resolve_key()?;
        self.since_id_with_key(since_id, limit, key.as_ref())
    }

    /// Events with `id > since_id` with an explicit key, oldest first.
    pub fn since_id_with_key(
        &self,
        since_id: i64,
        limit: usize,
        key: Option<&KeyHandle>,
    ) -> rusqlite::Result<Vec<LoggedEvent>> {
        self.query_with_key("id", since_id, limit, key)
    }

    /// Events with `ts > since_ms` (unix milliseconds), oldest first.
    pub fn since_ts(&self, since_ms: i64, limit: usize) -> rusqlite::Result<Vec<LoggedEvent>> {
        let key = self.resolve_key()?;
        self.since_ts_with_key(since_ms, limit, key.as_ref())
    }

    /// Events with `ts > since_ms` (unix milliseconds) with an explicit key, oldest first.
    pub fn since_ts_with_key(
        &self,
        since_ms: i64,
        limit: usize,
        key: Option<&KeyHandle>,
    ) -> rusqlite::Result<Vec<LoggedEvent>> {
        self.query_with_key("ts", since_ms, limit, key)
    }

    /// Total rows currently stored.
    pub fn count(&self) -> rusqlite::Result<i64> {
        self.conn()
            .query_row("SELECT COUNT(*) FROM bus_events", [], |r| r.get(0))
    }

    /// Enforce retention (age, then row cap). Returns rows deleted.
    pub fn prune(&self) -> rusqlite::Result<usize> {
        let cutoff =
            (Utc::now() - chrono::Duration::days(self.retention.max_age_days)).timestamp_millis();
        self.prune_with(cutoff)
    }

    fn prune_with(&self, cutoff_ms: i64) -> rusqlite::Result<usize> {
        let conn = self.conn();
        let by_age = conn.execute("DELETE FROM bus_events WHERE ts < ?1", params![cutoff_ms])?;
        let by_rows = conn.execute(
            "DELETE FROM bus_events WHERE id <= \
             (SELECT id FROM bus_events ORDER BY id DESC LIMIT 1 OFFSET ?1)",
            params![self.retention.max_rows],
        )?;
        Ok(by_age + by_rows)
    }
}

/// Parse the `since` query value: an integer is an id, otherwise an RFC 3339 timestamp.
pub fn replay(
    log: &EventLog,
    since: Option<&str>,
    limit: usize,
) -> Result<Vec<LoggedEvent>, String> {
    replay_with_key(log, since, limit, None)
}

/// Parse the `since` query value with an explicit key override.
pub fn replay_with_key(
    log: &EventLog,
    since: Option<&str>,
    limit: usize,
    key: Option<&KeyHandle>,
) -> Result<Vec<LoggedEvent>, String> {
    match since.map(str::trim).filter(|s| !s.is_empty()) {
        None => match key {
            Some(k) => log
                .since_id_with_key(0, limit, Some(k))
                .map_err(|e| e.to_string()),
            None => log.since_id(0, limit).map_err(|e| e.to_string()),
        },
        Some(raw) => {
            if let Ok(id) = raw.parse::<i64>() {
                match key {
                    Some(k) => log
                        .since_id_with_key(id, limit, Some(k))
                        .map_err(|e| e.to_string()),
                    None => log.since_id(id, limit).map_err(|e| e.to_string()),
                }
            } else if let Ok(ts) = DateTime::parse_from_rfc3339(raw) {
                match key {
                    Some(k) => log
                        .since_ts_with_key(ts.timestamp_millis(), limit, Some(k))
                        .map_err(|e| e.to_string()),
                    None => log
                        .since_ts(ts.timestamp_millis(), limit)
                        .map_err(|e| e.to_string()),
                }
            } else {
                Err("since must be an event id or an RFC 3339 timestamp".to_string())
            }
        }
    }
}

/// Split an event into (kind, session_id, payload). Lease tokens are redacted.
fn describe(event: &XavierEvent) -> (String, Option<String>, serde_json::Value) {
    let value = serde_json::to_value(event).unwrap_or(serde_json::Value::Null);
    let (kind, mut payload) = match value {
        serde_json::Value::Object(map) if map.len() == 1 => {
            let (k, v) = map.into_iter().next().unwrap_or_default();
            (k, v)
        }
        other => ("Unknown".to_string(), other),
    };
    if let Some(obj) = payload.as_object_mut() {
        if obj.contains_key("token") {
            obj.insert("token".into(), "[redacted]".into());
        }
    }
    let session_id = payload
        .get("agent_id")
        .or_else(|| payload.get("task_id"))
        .or_else(|| payload.get("task").and_then(|t| t.get("id")))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    (kind, session_id, payload)
}

/// Capacity of the queue between the bus drain loop and the writer task.
const WRITE_QUEUE: usize = 4096;

type Record = (String, Option<String>, serde_json::Value);

/// Mirror every event published on `bus` into `log`, and prune periodically.
///
/// The bus receiver only enqueues (never blocks on SQLite), so slow disk IO
/// cannot make this subscriber lag and starve other bus consumers. A single
/// writer task performs the blocking appends off the async workers.
pub fn spawn_subscriber(bus: &XavierEventBus, log: Arc<EventLog>) {
    let mut receiver = bus.subscribe();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Record>(WRITE_QUEUE);
    tokio::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    if tx.try_send(describe(&event)).is_err() {
                        tracing::warn!("event log write queue full; event dropped");
                    }
                }
                Err(RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "event log subscriber lagged; events dropped");
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
    let writer = Arc::clone(&log);
    tokio::spawn(async move {
        while let Some(first) = rx.recv().await {
            let mut batch = vec![first];
            while let Ok(more) = rx.try_recv() {
                batch.push(more);
            }
            let writer = Arc::clone(&writer);
            let result = tokio::task::spawn_blocking(move || {
                for (kind, session_id, payload) in batch {
                    if let Err(e) = writer.append(&kind, session_id.as_deref(), &payload) {
                        tracing::warn!(error = %e, "event log append failed");
                    }
                }
            })
            .await;
            if let Err(e) = result {
                tracing::warn!(error = %e, "event log writer task failed");
            }
        }
    });
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(PRUNE_INTERVAL);
        loop {
            ticker.tick().await;
            let log = Arc::clone(&log);
            match tokio::task::spawn_blocking(move || log.prune()).await {
                Ok(Err(e)) => tracing::warn!(error = %e, "event log prune failed"),
                Err(e) => tracing::warn!(error = %e, "event log prune task failed"),
                Ok(Ok(_)) => {}
            }
        }
    });
}

static GLOBAL: OnceLock<Arc<EventLog>> = OnceLock::new();

/// Register the process-wide log (first call wins).
pub fn install_global(log: Arc<EventLog>) {
    let _ = GLOBAL.set(log);
}

/// The process-wide log, if the daemon installed one.
pub fn global() -> Option<Arc<EventLog>> {
    GLOBAL.get().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::cycle_breaks::w30_09::{SessionEvent, SessionEventType};

    fn test_key_handle(dir: &std::path::Path) -> KeyHandle {
        use crate::espacio::keys::{KeyRing, StaticNodeKek, UnlockMode};
        let ring = KeyRing::new(dir, std::sync::Arc::new(StaticNodeKek::new([0x42; 32])));
        ring.create_keys("default", UnlockMode::NodeUnlock, None)
            .expect("keys")
            .0
    }

    fn agent_event(n: u32) -> XavierEvent {
        XavierEvent::AgentTaskStarted {
            agent_id: format!("agent-{n}"),
            task_id: format!("task-{n}"),
        }
    }

    async fn wait_for_count(log: &EventLog, want: i64) {
        for _ in 0..1000 {
            if log.count().expect("count") >= want {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("event log never reached {want} rows");
    }

    #[test]
    fn test_adversary_event_payload_encrypted_at_rest_and_legacy_clear_readable() {
        const CANARY: &str = "CANARY-BUS-EVENT-PAYLOAD-9f3b7c";
        let dir = tempfile::tempdir().expect("tempdir");
        let keys_dir = tempfile::tempdir().expect("tempdir");
        let dek = test_key_handle(keys_dir.path());

        let log =
            EventLog::open_with_key(dir.path(), RetentionConfig::default(), Some(dek.clone()))
                .expect("open with key");

        // Real write function
        let payload = serde_json::json!({
            "marker": CANARY,
            "secret_field": 12345
        });
        let (kind, session_id, _) = describe(&XavierEvent::AgentTaskStarted {
            agent_id: "agent-canary".into(),
            task_id: "task-canary".into(),
        });
        let event_id = log
            .append(&kind, session_id.as_deref(), &payload)
            .expect("append");

        // Read raw sqlite file bytes and wal file bytes if present, assert canary is absent
        let db_path = EventLog::db_path(dir.path());
        let db_bytes = std::fs::read(&db_path).expect("read db bytes");
        assert!(
            db_bytes
                .windows(CANARY.len())
                .all(|w| w != CANARY.as_bytes()),
            "plaintext canary found in sqlite db file"
        );

        let wal_path = dir.path().join("events").join("bus_events.sqlite3-wal");
        if wal_path.exists() {
            let wal_bytes = std::fs::read(&wal_path).expect("read wal bytes");
            assert!(
                wal_bytes
                    .windows(CANARY.len())
                    .all(|w| w != CANARY.as_bytes()),
                "plaintext canary found in sqlite wal file"
            );
        }

        // Checkpoint WAL and verify main db file still has no canary
        let _ = log.conn().execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        let db_bytes_post = std::fs::read(&db_path).expect("read db bytes post checkpoint");
        assert!(
            db_bytes_post
                .windows(CANARY.len())
                .all(|w| w != CANARY.as_bytes()),
            "plaintext canary found in sqlite db file after checkpoint"
        );

        // Read it back through real read path and get the plaintext
        let events = log.since_id(0, 10).expect("read back");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, event_id);
        assert_eq!(events[0].payload["marker"], CANARY);

        // Insert a legacy clear row by raw SQL and read it back
        const LEGACY_CANARY: &str = "LEGACY-CLEAR-PAYLOAD-8a1e2d";
        let legacy_json = serde_json::json!({ "legacy": LEGACY_CANARY }).to_string();
        log.conn()
            .execute(
                "INSERT INTO bus_events (ts, kind, session_id, payload_json) VALUES (?1, ?2, ?3, ?4)",
                params![
                    Utc::now().timestamp_millis(),
                    "LegacyEvent",
                    Some("sess-legacy"),
                    legacy_json
                ],
            )
            .expect("raw sql insert legacy clear row");

        let all_events = log.since_id(0, 10).expect("read back including legacy");
        assert_eq!(all_events.len(), 2);
        let sealed_ev = all_events
            .iter()
            .find(|e| e.id == event_id)
            .expect("found sealed");
        assert_eq!(sealed_ev.payload["marker"], CANARY);
        let legacy_ev = all_events
            .iter()
            .find(|e| e.kind == "LegacyEvent")
            .expect("found legacy");
        assert_eq!(legacy_ev.payload["legacy"], LEGACY_CANARY);

        // Read sealed row without key: must be an error, not a panic
        let locked_log = EventLog::open_with_key(dir.path(), RetentionConfig::default(), None)
            .expect("open without key");
        assert!(
            locked_log.since_id(0, 10).is_err(),
            "reading sealed row without key must be an error"
        );

        // Anti-swap verification: if row ID is tampered, authentication must fail
        log.conn()
            .execute(
                "UPDATE bus_events SET id = 9999 WHERE id = ?1",
                params![event_id],
            )
            .expect("swap row id");
        assert!(
            log.since_id(0, 10).is_err(),
            "opening sealed row with mismatched id must fail authentication"
        );
    }

    #[tokio::test]
    async fn published_event_with_encryption_is_logged_and_readable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keys_dir = tempfile::tempdir().expect("tempdir");
        let dek = test_key_handle(keys_dir.path());
        let bus = XavierEventBus::new(16);
        {
            let log =
                EventLog::open_with_key(dir.path(), RetentionConfig::default(), Some(dek.clone()))
                    .expect("open");
            spawn_subscriber(&bus, Arc::clone(&log));
            bus.publish(agent_event(1)).expect("publish");
            wait_for_count(&log, 1).await;
        }
        let reopened = EventLog::open_with_key(dir.path(), RetentionConfig::default(), Some(dek))
            .expect("reopen");
        let events = reopened.since_id(0, 10).expect("query");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "AgentTaskStarted");
        assert_eq!(events[0].session_id.as_deref(), Some("agent-1"));
    }

    #[tokio::test]
    async fn published_event_is_logged_and_survives_reopen() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bus = XavierEventBus::new(16);
        {
            let log = EventLog::open(dir.path(), RetentionConfig::default()).expect("open");
            spawn_subscriber(&bus, Arc::clone(&log));
            bus.publish(agent_event(1)).expect("publish");
            bus.publish(XavierEvent::LeaseRenewed {
                token: "secret-lease".into(),
            })
            .expect("publish");
            wait_for_count(&log, 2).await;
        }
        let reopened = EventLog::open(dir.path(), RetentionConfig::default()).expect("reopen");
        let events = reopened.since_id(0, 10).expect("query");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, "AgentTaskStarted");
        assert_eq!(events[0].session_id.as_deref(), Some("agent-1"));
        assert_eq!(events[1].payload["token"], "[redacted]");
    }

    #[test]
    fn replay_since_id_returns_only_newer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log = EventLog::open(dir.path(), RetentionConfig::default()).expect("open");
        let mut ids = Vec::new();
        for n in 0..5 {
            let (k, s, p) = describe(&agent_event(n));
            ids.push(log.append(&k, s.as_deref(), &p).expect("append"));
        }
        let newer = replay(&log, Some(&ids[2].to_string()), 100).expect("replay");
        assert_eq!(newer.iter().map(|e| e.id).collect::<Vec<_>>(), ids[3..]);
        let limited = replay(&log, None, 2).expect("replay");
        assert_eq!(limited.len(), 2);
        let by_ts = replay(&log, Some("2000-01-01T00:00:00Z"), 100).expect("replay");
        assert_eq!(by_ts.len(), 5);
        assert!(replay(&log, Some("garbage"), 10).is_err());
    }

    #[test]
    fn retention_prunes_by_rows_and_age() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log = EventLog::open(
            dir.path(),
            RetentionConfig {
                max_age_days: 7,
                max_rows: 3,
            },
        )
        .expect("open");
        for n in 0..6 {
            let (k, s, p) = describe(&agent_event(n));
            log.append(&k, s.as_deref(), &p).expect("append");
        }
        log.prune().expect("prune");
        let left = log.since_id(0, 100).expect("query");
        assert_eq!(left.len(), 3);
        assert_eq!(left[0].session_id.as_deref(), Some("agent-3"));

        // Everything is older than a cutoff in the future.
        log.prune_with(Utc::now().timestamp_millis() + 60_000)
            .expect("prune");
        assert_eq!(log.count().expect("count"), 0);
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn subscriber_loop_never_blocks_on_slow_writer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log = EventLog::open(dir.path(), RetentionConfig::default()).expect("open");
        let bus = XavierEventBus::new(16);
        let mut other = bus.subscribe();
        spawn_subscriber(&bus, Arc::clone(&log));
        // Hold the DB lock so every write stalls.
        let guard = log.conn();
        for n in 0..200 {
            bus.publish(agent_event(n)).expect("publish");
            // Another subscriber must keep receiving without Lagged.
            match tokio::time::timeout(Duration::from_secs(2), other.recv()).await {
                Ok(Ok(_)) => {}
                other => panic!("other subscriber starved: {other:?}"),
            }
            // Let the log subscriber drain (bus capacity is only 16).
            tokio::task::yield_now().await;
        }
        drop(guard);
        wait_for_count(&log, 200).await;
    }

    #[tokio::test]
    async fn session_event_endpoint_persists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log = EventLog::open(dir.path(), RetentionConfig::default()).expect("open");
        install_global(Arc::clone(&log));
        let installed = global().expect("global");
        let event = SessionEvent {
            session_id: "sess-persist-test".into(),
            event_type: SessionEventType::Message,
            timestamp: Utc::now(),
            // Multi-byte char straddling byte 200 must not panic the preview.
            content: Some(format!("{}{}", "a".repeat(199), "ñ".repeat(10))),
            metadata: None,
        };
        let _ =
            crate::adapters::inbound::http::routes::session_event_handler(axum::Json(event)).await;
        let found = installed
            .since_id(0, MAX_REPLAY_LIMIT)
            .expect("query")
            .into_iter()
            .any(|e| e.session_id.as_deref() == Some("sess-persist-test"));
        assert!(found, "session event was not recorded");
    }
}
