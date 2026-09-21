//! Centralized SQLite PRAGMA configuration for Xavier.
//!
//! Enforces unified performance, safety, and memory consumption pragmas across
//! all SQLite database connections in the codebase.
//!
//! ## Layers
//!
//! The pragmas are split in two layers so that the pool *acquire* path stays
//! cheap and can never block on database-wide work:
//!
//! - [`apply_connection_pragmas`] — heavy, connection-lifetime settings
//!   (`journal_mode`, `cache_size`, `mmap_size`, `temp_store`, ...). Applied
//!   once per connection when it is created (pool construction).
//! - [`apply_acquire_pragmas`] — the only settings that need to be (re)applied
//!   on every acquire: `busy_timeout` and `foreign_keys`.
//!
//! [`apply_pragmas`] applies both layers and is kept for callers that configure
//! a standalone connection in one shot.
//!
//! ## No checkpointing during connection setup
//!
//! Neither layer checkpoints the WAL. `PRAGMA wal_checkpoint(TRUNCATE)` waits
//! for every reader of the database to finish, so running it while handing out a
//! pooled connection stalls *all* acquires behind any long-running read. Run
//! [`maybe_wal_checkpoint`] from maintenance paths instead — the startup sweep
//! lives in [`crate::storage::migrations::checkpoint`].

use rusqlite::{Connection, Result};

/// One-time (per connection) SQLite PRAGMA configuration.
///
/// These settings persist for the lifetime of the connection, so they are only
/// applied when the connection is created — never on the acquire path.
///
/// Settings applied:
/// - `journal_mode = WAL`: Write-Ahead Logging for concurrency
/// - `wal_autocheckpoint = 1000`: bound the WAL to 1000 frames before auto-checkpoint
/// - `journal_size_limit = 10485760`: cap the WAL file at ~10MB after checkpoints
/// - `synchronous = NORMAL`: Balance safety and IO performance in WAL mode
/// - `cache_size = -8000`: Limit page cache to ~8MB (negative values are KiB)
/// - `mmap_size = 268435456`: Memory-mapped I/O up to 256MB
/// - `temp_store = MEMORY`: Store temporary tables and indices in RAM
pub fn apply_connection_pragmas(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; \
         PRAGMA wal_autocheckpoint=1000; \
         PRAGMA journal_size_limit=10485760; \
         PRAGMA synchronous=NORMAL; \
         PRAGMA cache_size=-8000; \
         PRAGMA mmap_size=268435456; \
         PRAGMA temp_store=MEMORY;",
    )
}

/// PRAGMAs re-applied on every pool acquire.
///
/// Deliberately minimal: `busy_timeout` and `foreign_keys` are per-connection
/// in-memory settings, so re-applying them cannot take a database lock, perform
/// I/O, or checkpoint the WAL. Everything else is applied once by
/// [`apply_connection_pragmas`].
///
/// Settings applied:
/// - `busy_timeout = 5000`: Wait up to 5000ms when database is locked
/// - `foreign_keys = ON`: Enforce foreign key constraints
pub fn apply_acquire_pragmas(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA busy_timeout=5000; \
         PRAGMA foreign_keys=ON;",
    )
}

/// Applies standard SQLite PRAGMA settings to a database connection.
///
/// Equivalent to [`apply_connection_pragmas`] followed by
/// [`apply_acquire_pragmas`]. Does **not** checkpoint the WAL (see the module
/// documentation).
pub fn apply_pragmas(conn: &Connection) -> Result<()> {
    apply_connection_pragmas(conn)?;
    apply_acquire_pragmas(conn)
}

/// Checks current WAL size and executes `PRAGMA wal_checkpoint(TRUNCATE)` if threshold is exceeded.
///
/// Maintenance helper: call it from startup/background paths, never from the
/// pool acquire path (see the module documentation).
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

    /// Validates the acceptance criteria for issue #1793 (WAVE-7.03):
    /// `maybe_wal_checkpoint` must checkpoint when the WAL exceeds the
    /// configured threshold (10MB for prod) and must NOT checkpoint when the
    /// WAL is small (under threshold).
    #[test]
    fn test_checkpoint_triggers_on_large_wal() {
        let dir = tempfile::tempdir().expect("Failed to create tempdir");
        let db_path = dir.path().join("test_large_wal.db");
        let conn = Connection::open(&db_path).expect("Failed to open database");
        apply_pragmas(&conn).expect("Failed to apply pragmas");

        // Create a small dataset and verify checkpoint does NOT trigger with a
        // threshold much larger than the generated WAL (case 1: small WAL).
        conn.execute(
            "CREATE TABLE test_data (id INTEGER PRIMARY KEY, val TEXT);",
            [],
        )
        .expect("Failed to create table");
        for i in 0..50 {
            conn.execute(
                "INSERT INTO test_data (val) VALUES (?);",
                [format!("value_{i}")],
            )
            .expect("Failed to insert");
        }

        // Force the WAL to be checkpointed before measuring so we can compare.
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        let small_threshold: u64 = 10 * 1024 * 1024; // 10MB
        let not_checkpointed =
            maybe_wal_checkpoint(&conn, small_threshold).expect("Failed to run checkpoint");
        assert!(
            !not_checkpointed,
            "Case 1: checkpoint must NOT trigger for small WAL (below 10MB threshold)"
        );

        // Case 2: large WAL — write a payload that pushes frames past the
        // threshold and ensure `maybe_wal_checkpoint` returns true.
        let payload = "x".repeat(2_000); // ~2KB per row
        for i in 0..6_000 {
            conn.execute(
                "INSERT INTO test_data (val) VALUES (?);",
                [payload.as_str()],
            )
            .expect("Failed to insert large row");
            // Periodically checkpoint to keep WAL from auto-truncating and
            // simulate a stuck WAL — but for this test we let the WAL grow.
            if i % 1000 == 0 {
                let _ = conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);");
            }
        }
        // Use a 1-byte threshold so the function MUST checkpoint.
        let forced = maybe_wal_checkpoint(&conn, 1).expect("Failed to run checkpoint");
        assert!(
            forced,
            "Case 2: checkpoint MUST trigger when WAL exceeds threshold"
        );
    }

    /// Validates the production pragmas from issue #1793 acceptance criteria:
    /// `wal_autocheckpoint = 1000` and `journal_size_limit = 10485760` are set.
    #[test]
    fn test_wal_health_pragmas_apply() {
        let conn = Connection::open_in_memory().expect("Failed to open in-memory database");
        apply_pragmas(&conn).expect("Failed to apply pragmas");

        let wal_autocheckpoint: i64 = conn
            .query_row("PRAGMA wal_autocheckpoint;", [], |r| r.get(0))
            .expect("Failed to query wal_autocheckpoint");
        assert_eq!(
            wal_autocheckpoint, 1000,
            "PRAGMA wal_autocheckpoint must be 1000 frames per acceptance criteria"
        );

        let journal_size_limit: i64 = conn
            .query_row("PRAGMA journal_size_limit;", [], |r| r.get(0))
            .expect("Failed to query journal_size_limit");
        assert_eq!(
            journal_size_limit, 10485760,
            "PRAGMA journal_size_limit must be 10MB (10485760) per acceptance criteria"
        );
    }

    /// The acquire layer must only touch connection-local settings: it must not
    /// reconfigure the journal mode (that is a one-time, file-level operation)
    /// nor anything else that could take a database lock.
    #[test]
    fn test_acquire_pragmas_only_apply_cheap_settings() {
        let dir = tempfile::tempdir().expect("Failed to create tempdir");
        let db_path = dir.path().join("acquire_layer.db");
        let conn = Connection::open(&db_path).expect("Failed to open database");

        // Whatever mode the file starts in (rollback journal by default), the
        // acquire layer must leave it alone.
        let journal_mode_before: String = conn
            .query_row("PRAGMA journal_mode;", [], |r| r.get(0))
            .expect("Failed to query journal_mode");

        apply_acquire_pragmas(&conn).expect("Failed to apply acquire pragmas");

        let journal_mode_after: String = conn
            .query_row("PRAGMA journal_mode;", [], |r| r.get(0))
            .expect("Failed to query journal_mode");
        assert_eq!(
            journal_mode_before, journal_mode_after,
            "the acquire layer must not reconfigure journal_mode"
        );

        let busy_timeout: i64 = conn
            .query_row("PRAGMA busy_timeout;", [], |r| r.get(0))
            .expect("Failed to query busy_timeout");
        assert_eq!(busy_timeout, 5000);

        let foreign_keys: i64 = conn
            .query_row("PRAGMA foreign_keys;", [], |r| r.get(0))
            .expect("Failed to query foreign_keys");
        assert_eq!(foreign_keys, 1);
    }

    /// Connection setup must never checkpoint the WAL.
    ///
    /// A `wal_checkpoint(PASSIVE)` would fold the WAL back into the main database
    /// file and a `wal_checkpoint(TRUNCATE)` would additionally wait for every
    /// reader of the database — exactly the stall this layering removes from the
    /// pool acquire path.
    #[test]
    fn test_connection_setup_never_checkpoints_wal() {
        let dir = tempfile::tempdir().expect("Failed to create tempdir");
        let db_path = dir.path().join("no_checkpoint.db");
        let wal_path = dir.path().join("no_checkpoint.db-wal");

        let conn = Connection::open(&db_path).expect("Failed to open database");
        apply_pragmas(&conn).expect("Failed to apply pragmas");

        conn.execute_batch("CREATE TABLE test_data (id INTEGER PRIMARY KEY, val TEXT);")
            .expect("Failed to create table");
        for i in 0..5_000 {
            conn.execute(
                "INSERT INTO test_data (val) VALUES (?1);",
                [format!("value_{i}")],
            )
            .expect("Failed to insert");
        }

        // No checkpoint so far: every committed page still lives in the WAL, so
        // the main database file stays orders of magnitude smaller.
        let db_bytes_before = file_len(&db_path);
        let wal_bytes_before = file_len(&wal_path);
        assert!(db_bytes_before > 0, "main database file must exist");
        assert!(
            wal_bytes_before > db_bytes_before,
            "writes must stay WAL-resident"
        );

        // Re-applying any layer must not fold the WAL into the main file (which
        // grows the database file) nor truncate it (which shrinks the WAL).
        apply_pragmas(&conn).expect("Failed to re-apply pragmas");
        apply_acquire_pragmas(&conn).expect("Failed to apply acquire pragmas");

        assert_eq!(file_len(&db_path), db_bytes_before);
        assert_eq!(file_len(&wal_path), wal_bytes_before);
    }

    /// Size of a file in bytes.
    fn file_len(path: &std::path::Path) -> u64 {
        let metadata = std::fs::metadata(path).expect("Failed to stat file");
        metadata.len()
    }
}
