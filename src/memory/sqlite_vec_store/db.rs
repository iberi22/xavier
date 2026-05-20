use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use tokio::fs;

pub(crate) fn open_connection(path: &PathBuf) -> Result<Connection> {
    let conn = Connection::open(path).with_context(|| {
        format!(
            "failed to open SQLite database at {}",
            path.display()
        )
    })?;

    // Enable WAL mode with full optimizations
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; \
         PRAGMA synchronous=NORMAL; \
         PRAGMA wal_autocheckpoint=1000; \
         PRAGMA cache_size=-32768; \
         PRAGMA mmap_size=268435456; \
         PRAGMA temp_store=MEMORY; \
         PRAGMA foreign_keys=ON;",
    )
    .context("failed to set WAL mode")?;

    Ok(conn)
}

pub(crate) async fn ensure_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    Ok(())
}
