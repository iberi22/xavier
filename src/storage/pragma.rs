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
pub fn apply_pragmas(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; \
         PRAGMA synchronous=NORMAL; \
         PRAGMA cache_size=-8000; \
         PRAGMA mmap_size=268435456; \
         PRAGMA temp_store=MEMORY; \
         PRAGMA busy_timeout=5000; \
         PRAGMA foreign_keys=ON;",
    )
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
}
