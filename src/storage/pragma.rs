//! Centralized SQLite PRAGMA configuration for Xavier.
//!
//! Enforces unified performance, safety, and memory consumption pragmas across
//! all SQLite database connections in the codebase.

use rusqlite::{Connection, Result};

/// Applies standard SQLite PRAGMA settings to a database connection.
///
/// Settings applied:
/// - `journal_mode = WAL`: Write-Ahead Logging for concurrency
/// - `synchronous = NORMAL`: Balance safety and IO performance in WAL mode
/// - `cache_size = -8000`: Limit page cache to ~8MB (negative values are KiB)
/// - `mmap_size = 268435456`: Memory-mapped I/O up to 256MB
/// - `temp_store = MEMORY`: Store temporary tables and indices in RAM
/// - `busy_timeout = 5000`: Wait up to 5000ms when database is locked
/// - `foreign_keys = ON`: Enforce foreign key constraints
/// - `wal_autocheckpoint = 1000`: Checkpoint WAL every 1000 pages (~4MB)
pub fn apply_pragmas(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; \
         PRAGMA synchronous=NORMAL; \
         PRAGMA cache_size=-8000; \
         PRAGMA mmap_size=268435456; \
         PRAGMA temp_store=MEMORY; \
         PRAGMA busy_timeout=5000; \
         PRAGMA foreign_keys=ON; \
         PRAGMA wal_autocheckpoint=1000;",
    )
}

/// Checks current WAL size for database at `path` and executes `PRAGMA wal_checkpoint(TRUNCATE)`
/// if `wal_size > threshold_bytes` (defaults to 10MB if 0).
pub fn checkpoint_if_needed(path: &std::path::Path) -> anyhow::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }

    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let wal_path = path.with_file_name(format!("{}-wal", filename));
    let wal_size = std::fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0);

    let threshold = 10 * 1024 * 1024; // 10MB threshold
    if wal_size >= threshold {
        let conn = Connection::open(path)?;
        apply_pragmas(&conn)?;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Checks current WAL size and executes `PRAGMA wal_checkpoint(TRUNCATE)` if threshold is exceeded.
///
/// Returns `Ok(true)` if checkpoint was executed, `Ok(false)` if below threshold.
pub fn maybe_wal_checkpoint(conn: &Connection, threshold_bytes: u64) -> Result<bool> {
    let wal_frames: i64 = conn
        .query_row("PRAGMA wal_checkpoint(PASSIVE);", [], |r| r.get(1))
        .unwrap_or(0);

    // Approximate WAL size: frames * 4096 bytes per page
    let approx_wal_bytes = (wal_frames.max(0) as u64) * 4096;

    if approx_wal_bytes >= threshold_bytes || threshold_bytes == 0 {
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_pragmas() {
        let conn = Connection::open_in_memory().expect("Failed to open in-memory database");
        apply_pragmas(&conn).expect("Failed to apply pragmas");

        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode;", [], |r| r.get(0))
            .expect("Failed to query journal_mode");
        // In-memory databases may report "memory" or "wal" depending on SQLite compilation/flags
        assert!(journal_mode == "memory" || journal_mode == "wal");

        let synchronous: i64 = conn
            .query_row("PRAGMA synchronous;", [], |r| r.get(0))
            .expect("Failed to query synchronous");
        assert_eq!(synchronous, 1); // 1 = NORMAL

        let cache_size: i64 = conn
            .query_row("PRAGMA cache_size;", [], |r| r.get(0))
            .expect("Failed to query cache_size");
        assert_eq!(cache_size, -8000);

        let temp_store: i64 = conn
            .query_row("PRAGMA temp_store;", [], |r| r.get(0))
            .expect("Failed to query temp_store");
        assert_eq!(temp_store, 2); // 2 = MEMORY

        let busy_timeout: i64 = conn
            .query_row("PRAGMA busy_timeout;", [], |r| r.get(0))
            .expect("Failed to query busy_timeout");
        assert_eq!(busy_timeout, 5000);

        let foreign_keys: i64 = conn
            .query_row("PRAGMA foreign_keys;", [], |r| r.get(0))
            .expect("Failed to query foreign_keys");
        assert_eq!(foreign_keys, 1);
    }

    #[test]
    fn test_wal_auto_checkpoint_threshold() {
        let dir = tempfile::tempdir().expect("Failed to create tempdir");
        let db_path = dir.path().join("test_wal.db");
        let conn = Connection::open(&db_path).expect("Failed to open database");
        apply_pragmas(&conn).expect("Failed to apply pragmas");

        // Create table and insert some data to generate WAL frames
        conn.execute(
            "CREATE TABLE test_data (id INTEGER PRIMARY KEY, val TEXT);",
            [],
        )
        .expect("Failed to create table");
        for i in 0..100 {
            conn.execute(
                "INSERT INTO test_data (val) VALUES (?);",
                [format!("value_{i}")],
            )
            .expect("Failed to insert");
        }

        // Run checkpoint with small threshold (1 byte)
        let checkpointed = maybe_wal_checkpoint(&conn, 1).expect("Failed to run checkpoint");
        assert!(checkpointed);

        // Run checkpoint with huge threshold (100MB) -> should not trigger
        let not_checkpointed =
            maybe_wal_checkpoint(&conn, 100 * 1024 * 1024).expect("Failed to run checkpoint");
        assert!(!not_checkpointed);
    }

    #[test]
    fn test_checkpoint_triggers_on_large_wal() {
        let dir = tempfile::tempdir().expect("Failed to create tempdir");
        let db_path = dir.path().join("test_large_wal.db");

        {
            let conn = Connection::open(&db_path).expect("Failed to open database");
            apply_pragmas(&conn).expect("Failed to apply pragmas");
            conn.execute("CREATE TABLE test_data (id INTEGER PRIMARY KEY, val BLOB);", [])
                .expect("Failed to create table");
        }

        // 1. Small WAL case: wal file small/absent -> checkpoint_if_needed returns false
        let triggered = checkpoint_if_needed(&db_path).expect("checkpoint_if_needed failed");
        assert!(!triggered, "Small WAL should not trigger checkpoint");

        // 2. Large WAL case: generate legitimate >10MB WAL file using active uncheckpointed connection
        let conn = Connection::open(&db_path).expect("Failed to open database");
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
            .expect("Failed to disable autocheckpoint");
        let payload = vec![0u8; 1024 * 1024]; // 1MB blob
        for _ in 0..12 {
            conn.execute("INSERT INTO test_data (val) VALUES (?);", [&payload])
                .expect("Failed to insert blob");
        }

        let filename = db_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let wal_path = db_path.with_file_name(format!("{}-wal", filename));
        let wal_size_before = std::fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0);
        assert!(
            wal_size_before >= 10 * 1024 * 1024,
            "WAL should be >= 10MB before checkpoint, got {}",
            wal_size_before
        );

        let triggered = checkpoint_if_needed(&db_path).expect("checkpoint_if_needed failed");
        assert!(triggered, "Large WAL should trigger checkpoint");

        let wal_size_after = std::fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0);
        assert!(
            wal_size_after < wal_size_before,
            "WAL should be truncated after checkpoint"
        );
    }
}
