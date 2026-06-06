//! Database connection management
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;
use tokio::fs;
use crate::codebase::connection_manager::ConnectionManager;

pub(crate) async fn open_pool(path: &Path) -> Result<()> {
    let project_id = "vec_store";
    ConnectionManager::global().connect(project_id, &path.parent().unwrap_or(Path::new(".")).to_string_lossy())?;
    Ok(())
}

pub(crate) async fn open_connection(_path: &Path) -> Result<Connection> {
    Err(anyhow::anyhow!("open_connection is deprecated, use ConnectionManager instead"))
}

pub(crate) async fn ensure_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    Ok(())
}
