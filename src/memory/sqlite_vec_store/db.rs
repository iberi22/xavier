//! Database connection management
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::memory::connection_provider::{ConnectionProvider, GlobalConnectionProvider};
use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;
use tokio::fs;

#[expect(dead_code, reason = "Utility para abrir pool desde fuera del modulo")]
/// Open pool.
pub(crate) async fn open_pool(path: &Path) -> Result<()> {
    let project_id = super::project_id_for_path(path);
    GlobalConnectionProvider::new().connect_with_path(&project_id, path.to_path_buf())?;
    Ok(())
}

/// Ensure dir.
pub(crate) async fn ensure_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    Ok(())
}
