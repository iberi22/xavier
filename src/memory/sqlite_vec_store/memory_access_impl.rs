//! Access-instrumentation persistence for the SQLite vector backend.
//!
//! Two tables, deliberately outside `memory_records`:
//!
//! - `memory_access` — one row per `(workspace_id, memory_id)` holding an access
//!   counter and first/last timestamps. Writes are buffered in
//!   `crate::memory::access::RECORDER` and land here as one batched upsert, so
//!   a top-k read costs a fraction of a statement per row instead of one.
//! - `maintenance_meta` — key/value state for maintenance jobs. Currently only
//!   `observation_started_at`, the clock that tells a future prune how long the
//!   access signal has actually been observed.
//!
//! `INSERT OR IGNORE` seeds the clock exactly once. If a restart re-seeded it,
//! every candidate's observation window would silently restart and no prune
//! could ever mature.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::params;

use super::VecSqliteMemoryStore;
use crate::memory::access::AccessStats;

/// Table holding the durable per-record access signal.
pub(crate) const TABLE_MEMORY_ACCESS: &str = "memory_access";
/// Table holding maintenance clocks and flags.
pub(crate) const TABLE_MAINTENANCE_META: &str = "maintenance_meta";

const CREATE_MEMORY_ACCESS: &str = r#"
CREATE TABLE IF NOT EXISTS memory_access (
    workspace_id TEXT NOT NULL,
    memory_id TEXT NOT NULL,
    access_count INTEGER NOT NULL DEFAULT 0,
    first_accessed_at TEXT,
    last_accessed_at TEXT,
    PRIMARY KEY (workspace_id, memory_id)
);
"#;

const CREATE_MAINTENANCE_META: &str = r#"
CREATE TABLE IF NOT EXISTS maintenance_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
"#;

impl VecSqliteMemoryStore {
    /// Create the access tables and seed the observation clock if unset.
    pub fn ensure_access_instrumentation_conn(
        conn: &rusqlite::Connection,
        observation_started_at: DateTime<Utc>,
    ) -> Result<()> {
        conn.execute_batch(CREATE_MEMORY_ACCESS)
            .context("failed to create memory_access table")?;
        conn.execute_batch(CREATE_MAINTENANCE_META)
            .context("failed to create maintenance_meta table")?;

        // INSERT OR IGNORE, not OR REPLACE: the first observation ever recorded
        // is the reference point. Overwriting it would restart the clock.
        conn.execute(
            "INSERT OR IGNORE INTO maintenance_meta (key, value, updated_at) VALUES (?1, ?2, ?3)",
            params![
                crate::memory::access::OBSERVATION_STARTED_AT_KEY,
                observation_started_at.to_rfc3339(),
                observation_started_at.to_rfc3339(),
            ],
        )
        .context("failed to seed observation_started_at")?;

        // Recency queries always filter by workspace then order by recency.
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_memory_access_ws_last \
             ON memory_access (workspace_id, last_accessed_at DESC);",
        )
        .context("failed to create memory_access index")?;

        Ok(())
    }

    /// Upsert a batch of buffered accesses in one transaction.
    pub fn record_accesses_conn(
        conn: &rusqlite::Connection,
        workspace_id: &str,
        entries: &[(String, u64)],
    ) -> Result<usize> {
        if entries.is_empty() {
            return Ok(0);
        }
        let now = Utc::now().to_rfc3339();
        let tx = conn
            .unchecked_transaction()
            .context("failed to begin access transaction")?;
        let written = {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO memory_access \
                     (workspace_id, memory_id, access_count, first_accessed_at, last_accessed_at) \
                 VALUES (?1, ?2, ?3, ?4, ?4) \
                 ON CONFLICT(workspace_id, memory_id) DO UPDATE SET \
                     access_count = access_count + excluded.access_count, \
                     last_accessed_at = excluded.last_accessed_at",
            )?;
            let mut written = 0usize;
            for (memory_id, delta) in entries {
                if *delta == 0 {
                    continue;
                }
                written += stmt.execute(params![workspace_id, memory_id, *delta as i64, now])?;
            }
            written
        };
        tx.commit().context("failed to commit access transaction")?;
        Ok(written)
    }

    /// Read every access statistic recorded for a workspace.
    pub fn load_access_stats_conn(
        conn: &rusqlite::Connection,
        workspace_id: &str,
    ) -> Result<Vec<(String, AccessStats)>> {
        let mut stmt = conn.prepare(
            "SELECT memory_id, access_count, first_accessed_at, last_accessed_at \
             FROM memory_access WHERE workspace_id = ?1 ORDER BY memory_id",
        )?;
        let rows = stmt.query_map(params![workspace_id], |row| {
            let first: Option<String> = row.get(2)?;
            let last: Option<String> = row.get(3)?;
            Ok((
                row.get::<_, String>(0)?,
                AccessStats {
                    access_count: row.get::<_, i64>(1)?.max(0) as u64,
                    first_accessed_at: first
                        .as_deref()
                        .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    last_accessed_at: last
                        .as_deref()
                        .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                },
            ))
        })?;

        let mut stats = Vec::new();
        for row in rows {
            stats.push(row?);
        }
        Ok(stats)
    }

    /// Read one maintenance key.
    pub fn read_maintenance_meta_conn(
        conn: &rusqlite::Connection,
        key: &str,
    ) -> Result<Option<DateTime<Utc>>> {
        let mut stmt = conn.prepare("SELECT value FROM maintenance_meta WHERE key = ?1 LIMIT 1")?;
        let mut rows = stmt.query(params![key])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let value: String = row.get(0)?;
        Ok(DateTime::parse_from_rfc3339(&value)
            .ok()
            .map(|dt| dt.with_timezone(&Utc)))
    }
}

// ---------------------------------------------------------------------------
// MemoryStore wiring
// ---------------------------------------------------------------------------
//
// The four `MemoryStore` methods (`ensure_access_instrumentation`,
// `record_accesses`, `load_access_stats`, `read_maintenance_meta`) are
// implemented on `VecSqliteMemoryStore` in `store_impl.rs`, where the single
// `impl MemoryStore for VecSqliteMemoryStore` block already lives. Only the
// synchronous `*_conn` helpers above live here.
