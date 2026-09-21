//! Multi-DB Manager for deep federated workspace databases.
//!
//! Handles initialization, listing, querying, and connecting
//! to multiple independent SQLite databases in `{XAVIER_DATA_DIR}/db/{db_id}.sqlite`.

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use crate::settings::XavierSettings;
use crate::workspace::{WorkspaceDb, WorkspaceDbKind};

#[derive(Clone, Default)]
pub struct MultiDbManager {
    databases: Arc<RwLock<HashMap<String, WorkspaceDb>>>,
    /// Opened stores keyed by `db_id`.
    ///
    /// Constructing a store opens the SQLite file and replays the migration set,
    /// so [`MultiDbManager::get_store`] hands out clones of the already
    /// initialised store instead of repeating that work on every call. Entries
    /// are invalidated by [`MultiDbManager::delete_database`].
    stores: Arc<RwLock<HashMap<String, VecSqliteMemoryStore>>>,
}

impl std::fmt::Debug for MultiDbManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `VecSqliteMemoryStore` is not `Debug`, so only the registry is shown.
        f.debug_struct("MultiDbManager")
            .field("databases", &self.databases)
            .finish_non_exhaustive()
    }
}

impl MultiDbManager {
    /// New.
    pub fn new() -> Self {
        Self {
            databases: Arc::new(RwLock::new(HashMap::new())),
            stores: Arc::new(RwLock::new(HashMap::new())),
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
        if !db_id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(anyhow!(
                "Database ID must contain only alphanumeric characters, dashes, or underscores"
            ));
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
        let store = VecSqliteMemoryStore::new(store_config).await?;

        // Store workspace database metadata.
        {
            let mut dbs = self.databases.write().await;
            dbs.insert(db_id.clone(), workspace_db.clone());
        }

        // Keep the freshly initialised store hot: its migrations already ran, so
        // a following `get_store` must not re-open the file and replay them.
        //
        // Locks are never nested (`databases` then `stores`, one at a time) so
        // the ordering against `ConnectionManager::global()`'s pool lock stays
        // trivially acyclic.
        self.stores.write().await.insert(db_id, store);

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

        // Drop the cached store before touching the filesystem: it holds the
        // pooled connections for the file, and an open handle would keep the
        // file alive on Windows.
        self.stores.write().await.remove(db_id);

        if let Some(workspace_db) = removed {
            let path = Path::new(&workspace_db.db_path);
            let project_id = crate::memory::sqlite_vec_store::project_id_for_path(path);
            crate::codebase::connection_manager::ConnectionManager::global()
                .disconnect(&project_id);

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

    /// Get a dynamic memory store connected to the given DB ID.
    ///
    /// Stores are cached per `db_id`: the first call opens the database file and
    /// runs the migration set once, later calls return a cheap clone of that
    /// same initialised store. [`MultiDbManager::delete_database`] invalidates
    /// the entry.
    pub async fn get_store(&self, db_id: &str) -> Result<VecSqliteMemoryStore> {
        if let Some(cached) = self.stores.read().await.get(db_id).cloned() {
            return Ok(cached);
        }

        let workspace_db = self
            .get_database(db_id)
            .await
            .ok_or_else(|| anyhow!("Database not found: {}", db_id))?;

        let store_config = VecSqliteStoreConfig {
            path: PathBuf::from(&workspace_db.db_path),
            embedding_dimensions: 0,
        };
        let store = VecSqliteMemoryStore::new(store_config).await?;

        // Another task may have opened the same database while we were awaiting;
        // keep whichever entry landed first so every caller shares one store.
        let mut cache = self.stores.write().await;
        if let Some(existing) = cache.get(db_id).cloned() {
            return Ok(existing);
        }
        cache.insert(db_id.to_string(), store.clone());
        Ok(store)
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
            .create_database(
                db_id.clone(),
                display_name.clone(),
                WorkspaceDbKind::Personal,
            )
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
        drop(store);

        // 5. Delete DB
        let deleted = manager.delete_database(&db_id).await.unwrap();
        assert!(deleted);

        // Ensure database list is empty
        let dbs_post = manager.list_databases().await;
        assert!(dbs_post.is_empty());

        // Ensure file is deleted
        assert!(!Path::new(&db.db_path).exists());
    }

    /// `get_store` must reuse the initialised store instead of re-opening the
    /// file (and replaying migrations) on every call, and `delete_database` must
    /// invalidate the cached entry before removing the file.
    #[tokio::test]
    async fn test_store_is_cached_and_invalidated_on_delete() {
        let manager = MultiDbManager::new();
        let db_id = "test_cached_store".to_string();

        let db = manager
            .create_database(
                db_id.clone(),
                "Cached".to_string(),
                WorkspaceDbKind::Personal,
            )
            .await
            .unwrap();

        // `create_database` left the store cached, so both reads must return
        // clones of that same instance — a store opened afresh would carry a
        // brand-new dedup config `Arc`.
        let first = manager.get_store(&db_id).await.unwrap();
        let second = manager.get_store(&db_id).await.unwrap();
        assert!(
            Arc::ptr_eq(&first.dedup_config, &second.dedup_config),
            "get_store must return clones of the cached store"
        );
        assert_eq!(
            first.connection_project_id(),
            second.connection_project_id()
        );

        assert!(manager.delete_database(&db_id).await.unwrap());
        assert!(
            manager.stores.read().await.is_empty(),
            "delete_database must invalidate the cached store"
        );
        assert!(
            manager.get_store(&db_id).await.is_err(),
            "a deleted database must no longer be openable"
        );
        assert!(!Path::new(&db.db_path).exists());
    }

    #[tokio::test]
    async fn test_invalid_db_id() {
        let manager = MultiDbManager::new();
        let result = manager
            .create_database(
                "../invalid".to_string(),
                "Invalid".to_string(),
                WorkspaceDbKind::Personal,
            )
            .await;
        assert!(result.is_err());
    }
}
