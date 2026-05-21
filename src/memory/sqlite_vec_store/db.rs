use anyhow::{Context, Result};
use libsql::{Builder, Connection};
use std::path::Path;
use tokio::fs;
use crate::utils::connection_pool::{LibsqlConnectionPool, PoolConfig};

pub(crate) async fn open_pool(path: &Path) -> Result<LibsqlConnectionPool> {
    let path_str = path.to_string_lossy().to_string();
    let db = Builder::new_local(&path_str)
        .build()
        .await
        .with_context(|| {
            format!(
                "failed to open libSQL database at {}",
                path.display()
            )
        })?;

    let pool = LibsqlConnectionPool::new(db, PoolConfig::default());
    Ok(pool)
}

pub(crate) async fn open_connection(path: &Path) -> Result<Connection> {
    let path_str = path.to_string_lossy().to_string();
    let db = Builder::new_local(&path_str)
        .build()
        .await
        .with_context(|| {
            format!(
                "failed to open libSQL database at {}",
                path.display()
            )
        })?;

    let conn = db.connect().context("failed to connect to libSQL database")?;

    // Enable WAL mode with full optimizations
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; \
         PRAGMA synchronous=NORMAL; \
         PRAGMA cache_size=-32768; \
         PRAGMA temp_store=MEMORY; \
         PRAGMA foreign_keys=ON;",
    )
    .await
    .context("failed to set WAL mode")?;

    Ok(conn)
}

pub(crate) async fn ensure_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    Ok(())
}
