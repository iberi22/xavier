//! Database connection management
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::codebase::connection_manager::ConnectionManager;
use anyhow::Result;
use rusqlite::Connection;
use sha2::Digest;
use std::path::Path;
use tokio::fs;

#[allow(dead_code)]
pub(crate) async fn open_pool(path: &Path) -> Result<()> {
    let project_id = super::project_id_for_path(path);
    ConnectionManager::global().connect_with_path(&project_id, path.to_path_buf())?;
    Ok(())
}

#[allow(dead_code)]
pub(crate) async fn open_connection(_path: &Path) -> Result<Connection> {
    Err(anyhow::anyhow!(
        "open_connection is deprecated, use ConnectionManager instead"
    ))
}

pub(crate) async fn ensure_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    Ok(())
}
