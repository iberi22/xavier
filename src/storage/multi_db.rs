//! Multi-DB Manager for deep federated workspace databases.
//!
//! Handles initialization, listing, querying, and connecting
//! to multiple independent SQLite databases in `{XAVIER_DATA_DIR}/db/{db_id}.sqlite`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use anyhow::{anyhow, Result};
use tokio::sync::RwLock;

use crate::workspace::{WorkspaceDb, WorkspaceDbKind};
use crate::settings::XavierSettings;
use crate::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};

#[derive(Debug, Clone, Default)]
pub struct MultiDbManager {
    databases: Arc<RwLock<HashMap<String, WorkspaceDb>>>,
}

impl MultiDbManager {
    pub fn new() -> Self {
        Self {
            databases: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Resolve the dynamic path to the DB sqlite file in {XAVIER_DATA_DIR}/db/{db_id}.sqlite
    pub fn resolve_db_path(db_id: &str) -> PathBuf {
        let settings = XavierSettings::current();
        let data_dir = if settings.memory.data_dir.trim().is_empty() {
            PathBuf::from("data")
        } else {
            PathBuf::from(&settings.memory.data_dir)
        };
        data_dir.join("db").join(format!("{}.sqlite", db_id))
    }

    /// Create and initialize a new SQLite database
    pub async fn create_database(
        &self,
        db_id: String,
        display_name: String,
        kind: WorkspaceDbKind,
    ) -> Result<WorkspaceDb> {
        let db_id = db_id.trim().to_string();
        if db_id.is_empty() {
            return Err(anyhow!("Database ID cannot be empty"));
        }

        // Validate alphanumeric/underscore database ID to prevent directory traversal
        if !db_id.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
            return Err(anyhow!("Database ID must contain only alphanumeric characters, dashes, or underscores"));
        }

        let db_path = Self::resolve_db_path(&db_id);

        let workspace_db = WorkspaceDb {
            db_id: db_id.clone(),
            db_path: db_path.to_string_lossy().to_string(),
            display_name,
            kind,
        };

        // Create directory structure if needed
        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Initialize SQLite memory store to ensure schemas are loaded
        let store_config = VecSqliteStoreConfig {
            path: db_path.clone(),
            embedding_dimensions: 0, // falls back to defaults or environment
        };
        let _store = VecSqliteMemoryStore::new(store_config).await?;

        // Store workspace database metadata
        let mut dbs = self.databases.write().await;
        dbs.insert(db_id, workspace_db.clone());

        Ok(workspace_db)
    }

    /// List all registered databases
    pub async fn list_databases(&self) -> Vec<WorkspaceDb> {
        let dbs = self.databases.read().await;
        dbs.values().cloned().collect()
    }

    /// Get a specific workspace database by ID
    pub async fn get_database(&self, db_id: &str) -> Option<WorkspaceDb> {
        let dbs = self.databases.read().await;
        dbs.get(db_id).cloned()
    }

    /// Delete/Remove a database from the registry and remove the physical file
    pub async fn delete_database(&self, db_id: &str) -> Result<bool> {
        let removed = {
            let mut dbs = self.databases.write().await;
            dbs.remove(db_id)
        };

        if let Some(workspace_db) = removed {
            let path = Path::new(&workspace_db.db_path);
            if path.exists() {
                // Delete main SQLite file
                tokio::fs::remove_file(path).await?;
                // Delete associated -wal and -shm files if they exist
                let wal = path.with_extension("sqlite-wal");
                if wal.exists() {
                    let _ = tokio::fs::remove_file(wal).await;
                }
                let shm = path.with_extension("sqlite-shm");
                if shm.exists() {
                    let _ = tokio::fs::remove_file(shm).await;
                }
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get a dynamic memory store connected to the given DB ID
    pub async fn get_store(&self, db_id: &str) -> Result<VecSqliteMemoryStore> {
        let workspace_db = self
            .get_database(db_id)
            .await
            .ok_or_else(|| anyhow!("Database not found: {}", db_id))?;

        let store_config = VecSqliteStoreConfig {
            path: PathBuf::from(&workspace_db.db_path),
            embedding_dimensions: 0,
        };

        VecSqliteMemoryStore::new(store_config).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_multi_db_manager_lifecycle() {
        let manager = MultiDbManager::new();
        let db_id = "test_project_123".to_string();
        let display_name = "Test Project".to_string();

        // 1. Create DB
        let db = manager
            .create_database(db_id.clone(), display_name.clone(), WorkspaceDbKind::Personal)
            .await
            .unwrap();

        assert_eq!(db.db_id, db_id);
        assert_eq!(db.display_name, display_name);
        assert_eq!(db.kind, WorkspaceDbKind::Personal);
        assert!(db.db_path.contains("test_project_123.sqlite"));

        // 2. List DBs
        let dbs = manager.list_databases().await;
        assert_eq!(dbs.len(), 1);
        assert_eq!(dbs[0].db_id, db_id);

        // 3. Get DB
        let retrieved = manager.get_database(&db_id).await.unwrap();
        assert_eq!(retrieved.display_name, display_name);

        // 4. Get Store and perform basic checks
        let store = manager.get_store(&db_id).await;
        assert!(store.is_ok());

        // 5. Delete DB
        let deleted = manager.delete_database(&db_id).await.unwrap();
        assert!(deleted);

        // Ensure database list is empty
        let dbs_post = manager.list_databases().await;
        assert!(dbs_post.is_empty());

        // Ensure file is deleted
        assert!(!Path::new(&db.db_path).exists());
    }

    #[tokio::test]
    async fn test_invalid_db_id() {
        let manager = MultiDbManager::new();
        let result = manager
            .create_database("../invalid".to_string(), "Invalid".to_string(), WorkspaceDbKind::Personal)
            .await;
        assert!(result.is_err());
    }
}
