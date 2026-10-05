//! Multi-DB Manager for deep federated workspace databases.
//!
//! Handles initialization, listing, querying, and connecting
//! to multiple independent SQLite databases in `{XAVIER_DATA_DIR}/db/{db_id}.sqlite`.
//!
//! The registry (id, kind, display name, creation time, optional opaque
//! `config`) is persisted next to the files in `{db_dir}/registry.json`
//! (atomic temp + rename) and reloaded by [`MultiDbManager::open`]. Listing
//! reconciles the registry with the files on disk: files without an entry are
//! reported as `orphaned`, entries whose file is gone as `missing`. Orphans are
//! never deleted automatically. The registry holds no secrets.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::domain::cycle_breaks::w30_12::{DefaultVecStoreBackend, VecStore, VecStoreBackend};
use crate::settings::XavierSettings;
use crate::workspace::{WorkspaceDb, WorkspaceDbKind};

const REGISTRY_FILE: &str = "registry.json";
const REGISTRY_VERSION: u32 = 1;
const MAX_DB_ID_LEN: usize = 64;

/// Persisted registry entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DbRecord {
    db_id: String,
    kind: WorkspaceDbKind,
    display_name: String,
    created_at: String,
    /// Informational only; the effective path is always rebuilt from the id.
    path: String,
    /// Reserved for future per-db configuration; stored verbatim, unused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    config: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RegistryFile {
    version: u32,
    databases: Vec<DbRecord>,
}

/// Honest listing entry: registry state reconciled with the files on disk.
#[derive(Debug, Clone, Serialize)]
pub struct DbListing {
    pub db_id: String,
    pub db_path: String,
    pub display_name: String,
    /// `None` for orphaned files (unknown).
    pub kind: Option<WorkspaceDbKind>,
    pub created_at: Option<String>,
    /// File exists on disk but has no registry entry.
    pub orphaned: bool,
    /// Registry entry exists but its file is missing.
    pub missing: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}

/// Strict allowlist: 1..=64 ASCII chars of `[A-Za-z0-9_-]`.
pub fn validate_db_id(db_id: &str) -> Result<()> {
    if db_id.is_empty() {
        return Err(anyhow!("Database ID cannot be empty"));
    }
    if db_id.len() > MAX_DB_ID_LEN {
        return Err(anyhow!(
            "Database ID must be at most {} characters",
            MAX_DB_ID_LEN
        ));
    }
    if !db_id
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        return Err(anyhow!(
            "Database ID must contain only ASCII letters, digits, dashes, or underscores"
        ));
    }
    Ok(())
}

fn default_db_dir() -> PathBuf {
    let settings = XavierSettings::current();
    let data_dir = if settings.memory.data_dir.trim().is_empty() {
        PathBuf::from("data")
    } else {
        PathBuf::from(&settings.memory.data_dir)
    };
    data_dir.join("db")
}

#[derive(Clone, Default)]
pub struct MultiDbManager {
    databases: Arc<RwLock<HashMap<String, DbRecord>>>,
    /// Directory holding the sqlite files **and** `registry.json`. `None`
    /// resolves the directory from settings and keeps the registry in RAM only
    /// (legacy [`MultiDbManager::new`]).
    ///
    /// This is the *database* directory (`{XAVIER_DATA_DIR}/db`), not the data
    /// dir: `persist` writes `registry.json` straight into `root` and
    /// `path_for` joins `<db_id>.sqlite` directly onto it. Tests must use
    /// [`MultiDbManager::with_root`] so every file they create lands inside a
    /// tempdir instead of the real `{XAVIER_DATA_DIR}/db`.
    root: Option<PathBuf>,
    /// Opened stores keyed by `db_id`.
    ///
    /// Constructing a store opens the SQLite file and replays the migration set,
    /// so [`MultiDbManager::get_store`] hands out clones of the already
    /// initialised store instead of repeating that work on every call. Entries
    /// are invalidated by [`MultiDbManager::delete_database`].
    stores: Arc<RwLock<HashMap<String, VecStore>>>,
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
    /// New manager with a RAM-only registry (no persistence).
    ///
    /// Paths still resolve from [`XavierSettings`], i.e. the real
    /// `{XAVIER_DATA_DIR}/db`; only the registry file is skipped. Tests must
    /// use [`MultiDbManager::with_root`].
    pub fn new() -> Self {
        Self {
            root: None,
            databases: Arc::new(RwLock::new(HashMap::new())),
            stores: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// New manager whose database directory is an explicit path.
    ///
    /// Every database file this manager creates lands directly under `<root>/`,
    /// which is the *database* directory (`registry.json` lives there too), so
    /// passing a tempdir keeps tests fully inside the tempdir.
    pub fn with_root(root: PathBuf) -> Self {
        Self {
            root: Some(root),
            databases: Arc::new(RwLock::new(HashMap::new())),
            stores: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Open a manager rooted at `db_dir`, loading `registry.json` if present.
    ///
    /// An unreadable/corrupt registry is moved aside (`registry.json.corrupt`)
    /// and the manager starts empty; the files then show up as orphaned.
    pub fn open(db_dir: impl Into<PathBuf>) -> Result<Self> {
        let root: PathBuf = db_dir.into();
        std::fs::create_dir_all(&root)?;
        let reg_path = root.join(REGISTRY_FILE);
        let mut map = HashMap::new();
        if reg_path.exists() {
            let parsed = std::fs::read_to_string(&reg_path)
                .map_err(anyhow::Error::from)
                .and_then(|raw| {
                    serde_json::from_str::<RegistryFile>(&raw).map_err(anyhow::Error::from)
                });
            match parsed {
                Ok(file) => {
                    for mut rec in file.databases {
                        if validate_db_id(&rec.db_id).is_err() {
                            tracing::warn!("multi_db: skipping registry entry with invalid id");
                            continue;
                        }
                        rec.path = root
                            .join(format!("{}.sqlite", rec.db_id))
                            .to_string_lossy()
                            .to_string();
                        map.insert(rec.db_id.clone(), rec);
                    }
                }
                Err(e) => {
                    tracing::warn!("multi_db: registry unreadable ({e}); moving aside");
                    let _ = std::fs::rename(&reg_path, root.join("registry.json.corrupt"));
                }
            }
        }
        Ok(Self {
            databases: Arc::new(RwLock::new(map)),
            root: Some(root),
            stores: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Open the manager on `{data_dir}/db` from the current settings.
    pub fn open_default() -> Result<Self> {
        Self::open(default_db_dir())
    }

    fn db_dir(&self) -> PathBuf {
        self.root.clone().unwrap_or_else(default_db_dir)
    }

    fn path_for(&self, db_id: &str) -> PathBuf {
        self.db_dir().join(format!("{}.sqlite", db_id))
    }

    /// Resolve the dynamic path to the DB sqlite file in {XAVIER_DATA_DIR}/db/{db_id}.sqlite
    pub fn resolve_db_path(db_id: &str) -> PathBuf {
        default_db_dir().join(format!("{}.sqlite", db_id))
    }

    /// Atomically write the registry (temp + rename). No-op without a root.
    fn persist(&self, map: &HashMap<String, DbRecord>) -> Result<()> {
        let Some(root) = &self.root else {
            return Ok(());
        };
        let mut databases: Vec<DbRecord> = map.values().cloned().collect();
        databases.sort_by(|a, b| a.db_id.cmp(&b.db_id));
        let body = serde_json::to_vec_pretty(&RegistryFile {
            version: REGISTRY_VERSION,
            databases,
        })?;
        let tmp = root.join(format!("{REGISTRY_FILE}.tmp"));
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&body)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, root.join(REGISTRY_FILE))?;
        Ok(())
    }

    /// Create and initialize a new SQLite database
    pub async fn create_database(
        &self,
        db_id: String,
        display_name: String,
        kind: WorkspaceDbKind,
    ) -> Result<WorkspaceDb> {
        let db_id = db_id.trim().to_string();
        validate_db_id(&db_id)?;

        if self.databases.read().await.contains_key(&db_id) {
            return Err(anyhow!("Database already registered: {}", db_id));
        }

        // `validate_db_id` (called above) is the single traversal guard: ASCII
        // alphanumerics plus `_`/`-`, capped at MAX_DB_ID_LEN. Do not re-add a
        // weaker inline check here.
        let db_path = self.path_for(&db_id);

        let workspace_db = WorkspaceDb {
            db_id: db_id.clone(),
            db_path: db_path.to_string_lossy().to_string(),
            display_name: display_name.clone(),
            kind,
        };

        // Create directory structure if needed
        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Initialize SQLite memory store to ensure schemas are loaded
        let store = DefaultVecStoreBackend::open(db_path.clone()).await?;

        // Store workspace database metadata and persist the registry.
        {
            let mut dbs = self.databases.write().await;
            dbs.insert(
                db_id.clone(),
                DbRecord {
                    db_id: db_id.clone(),
                    kind,
                    display_name,
                    created_at: chrono::Utc::now().to_rfc3339(),
                    path: workspace_db.db_path.clone(),
                    config: None,
                },
            );
            if let Err(e) = self.persist(&dbs) {
                dbs.remove(&db_id);
                return Err(e);
            }
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

    /// List all registered databases (registry entries only).
    pub async fn list_databases(&self) -> Vec<WorkspaceDb> {
        let dbs = self.databases.read().await;
        dbs.values().map(Self::to_workspace_db).collect()
    }

    fn to_workspace_db(rec: &DbRecord) -> WorkspaceDb {
        WorkspaceDb {
            db_id: rec.db_id.clone(),
            db_path: rec.path.clone(),
            display_name: rec.display_name.clone(),
            kind: rec.kind,
        }
    }

    /// Honest listing: registry entries (flagged `missing` when the file is
    /// gone) plus `*.sqlite` files without an entry (flagged `orphaned`).
    pub async fn list_entries(&self) -> Vec<DbListing> {
        let mut out: Vec<DbListing> = {
            let dbs = self.databases.read().await;
            dbs.values()
                .map(|r| DbListing {
                    db_id: r.db_id.clone(),
                    db_path: r.path.clone(),
                    display_name: r.display_name.clone(),
                    kind: Some(r.kind),
                    created_at: Some(r.created_at.clone()),
                    orphaned: false,
                    missing: !Path::new(&r.path).exists(),
                    config: r.config.clone(),
                })
                .collect()
        };

        if let Ok(mut rd) = tokio::fs::read_dir(self.db_dir()).await {
            while let Ok(Some(ent)) = rd.next_entry().await {
                let path = ent.path();
                if path.extension().and_then(|e| e.to_str()) != Some("sqlite") {
                    continue;
                }
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                if validate_db_id(stem).is_err() || out.iter().any(|d| d.db_id == stem) {
                    continue;
                }
                out.push(DbListing {
                    db_id: stem.to_string(),
                    db_path: path.to_string_lossy().to_string(),
                    display_name: stem.to_string(),
                    kind: None,
                    created_at: None,
                    orphaned: true,
                    missing: false,
                    config: None,
                });
            }
        }
        out.sort_by(|a, b| a.db_id.cmp(&b.db_id));
        out
    }

    /// Get a specific workspace database by ID
    pub async fn get_database(&self, db_id: &str) -> Option<WorkspaceDb> {
        let dbs = self.databases.read().await;
        dbs.get(db_id).map(Self::to_workspace_db)
    }

    /// Delete/Remove a database from the registry and remove the physical file.
    ///
    /// An id that is not registered but has a file on disk (an orphan) is
    /// removed too, since the caller names it explicitly. Returns `false` when
    /// neither exists.
    pub async fn delete_database(&self, db_id: &str) -> Result<bool> {
        validate_db_id(db_id)?;

        let removed = {
            let mut dbs = self.databases.write().await;
            let removed = dbs.remove(db_id);
            if let Some(rec) = &removed {
                if let Err(e) = self.persist(&dbs) {
                    // Keep RAM and disk consistent.
                    dbs.insert(db_id.to_string(), rec.clone());
                    return Err(e);
                }
            }
            removed
        };

        // Drop the cached store before touching the filesystem: it holds the
        // pooled connections for the file, and an open handle would keep the
        // file alive on Windows.
        self.stores.write().await.remove(db_id);

        let path = self.path_for(db_id);
        if removed.is_none() && !path.exists() {
            return Ok(false);
        }

        let project_id = DefaultVecStoreBackend::project_id_for_path(&path);
        crate::codebase::connection_manager::ConnectionManager::global().disconnect(&project_id);

        if path.exists() {
            // Delete main SQLite file
            tokio::fs::remove_file(&path).await?;
        }
        // Delete associated -wal and -shm files if they exist
        let wal = path.with_extension("sqlite-wal");
        if wal.exists() {
            let _ = tokio::fs::remove_file(wal).await;
        }
        let shm = path.with_extension("sqlite-shm");
        if shm.exists() {
            let _ = tokio::fs::remove_file(shm).await;
        }
        Ok(true)
    }

    /// Get a dynamic memory store connected to the given DB ID.
    ///
    /// Stores are cached per `db_id`: the first call opens the database file and
    /// runs the migration set once, later calls return a cheap clone of that
    /// same initialised store. [`MultiDbManager::delete_database`] invalidates
    /// the entry.
    pub async fn get_store(&self, db_id: &str) -> Result<VecStore> {
        if let Some(cached) = self.stores.read().await.get(db_id).cloned() {
            return Ok(cached);
        }

        let workspace_db = self
            .get_database(db_id)
            .await
            .ok_or_else(|| anyhow!("Database not found: {}", db_id))?;

        let store = DefaultVecStoreBackend::open(PathBuf::from(&workspace_db.db_path)).await?;

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
        let dir = tempfile::tempdir().unwrap();
        let manager = MultiDbManager::with_root(dir.path().to_path_buf());
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
        let dir = tempfile::tempdir().unwrap();
        let manager = MultiDbManager::with_root(dir.path().to_path_buf());
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
        let dir = tempfile::tempdir().unwrap();
        let manager = MultiDbManager::with_root(dir.path().to_path_buf());
        let result = manager
            .create_database(
                "../invalid".to_string(),
                "Invalid".to_string(),
                WorkspaceDbKind::Personal,
            )
            .await;
        assert!(result.is_err());
    }

    /// T1 acceptance: the whole `create` → `list` → `get` → `get_store` →
    /// `delete` lifecycle must never touch the settings-resolved data dir
    /// (`{XAVIER_DATA_DIR}/db`). Before `with_root`, path resolution went
    /// straight to `XavierSettings::current()`, so every `cargo test` run left
    /// a `test_*.sqlite` file behind in the real `data/db/`.
    #[tokio::test]
    async fn lifecycle_never_touches_settings_data_dir() {
        let settings_data_dir = XavierSettings::current().memory.data_dir.trim().to_string();
        let settings_db_dir = if settings_data_dir.is_empty() {
            PathBuf::from("data").join("db")
        } else {
            PathBuf::from(&settings_data_dir).join("db")
        };
        // Normalise so the comparison below works with any cwd spelling.
        let settings_db_dir = std::fs::canonicalize(&settings_db_dir).ok();

        let dir = tempfile::tempdir().unwrap();
        let manager = MultiDbManager::with_root(dir.path().to_path_buf());
        let db_id = "lifecycle_hygiene_probe".to_string();

        let db = manager
            .create_database(
                db_id.clone(),
                "Hygiene".to_string(),
                WorkspaceDbKind::Personal,
            )
            .await
            .unwrap();

        // 1. The file must live inside the tempdir...
        let db_path = std::fs::canonicalize(&db.db_path).unwrap();
        let tmp_root = std::fs::canonicalize(dir.path()).unwrap();
        assert!(
            db_path.starts_with(&tmp_root),
            "the created database must live inside the tempdir, got {}",
            db_path.display()
        );

        // 2. ...and must NOT be the settings-resolved one.
        if let Some(settings_db_dir) = settings_db_dir.as_ref() {
            assert_ne!(
                db_path,
                settings_db_dir.join(format!("{}.sqlite", db_id)),
                "with_root must override the settings-resolved data dir"
            );
        }

        // 3. Drive the rest of the lifecycle; a leak in any of these steps
        //    (re-open, store cache, delete) would show up here.
        assert!(manager.get_store(&db_id).await.is_ok());
        assert_eq!(manager.list_databases().await.len(), 1);
        assert!(manager.delete_database(&db_id).await.unwrap());

        // 4. The settings data dir must be byte-identical: no file created,
        //    none left behind.
        if let Some(settings_db_dir) = settings_db_dir.as_ref() {
            let leaked = settings_db_dir.join(format!("{}.sqlite", db_id));
            assert!(
                !leaked.exists(),
                "the lifecycle wrote {} into the real data dir",
                leaked.display()
            );
        }
    }
    #[test]
    fn test_validate_db_id_allowlist() {
        for bad in [
            "",
            "../x",
            "..",
            "a/b",
            "a\\b",
            "a.b",
            "a b",
            "x\0y",
            "caf\u{e9}",
            "/etc/passwd",
            &"a".repeat(65),
        ] {
            assert!(validate_db_id(bad).is_err(), "must reject {bad:?}");
        }
        for ok in ["a", "Test_1-x", &"a".repeat(64)] {
            assert!(validate_db_id(ok).is_ok(), "must accept {ok:?}");
        }
    }

    #[tokio::test]
    async fn test_traversal_ids_rejected_everywhere() {
        let dir = tempfile::tempdir().unwrap();
        let m = MultiDbManager::open(dir.path().join("db")).unwrap();
        for bad in ["../evil", "a/b", "..", "x.y"] {
            assert!(m
                .create_database(bad.into(), "x".into(), WorkspaceDbKind::Org)
                .await
                .is_err());
            assert!(m.delete_database(bad).await.is_err());
        }
        assert!(!dir.path().join("evil.sqlite").exists());
    }

    #[tokio::test]
    async fn test_registry_persists_across_reopen_and_delete_works() {
        let dir = tempfile::tempdir().unwrap();
        let db_dir = dir.path().join("db");
        let path;
        {
            let m = MultiDbManager::open(&db_dir).unwrap();
            let db = m
                .create_database(
                    "persist_1".into(),
                    "Persist".into(),
                    WorkspaceDbKind::Family,
                )
                .await
                .unwrap();
            path = PathBuf::from(db.db_path);
            assert!(path.starts_with(&db_dir));
        }

        let m2 = MultiDbManager::open(&db_dir).unwrap();
        let list = m2.list_entries().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].db_id, "persist_1");
        assert_eq!(list[0].display_name, "Persist");
        assert_eq!(list[0].kind, Some(WorkspaceDbKind::Family));
        assert!(list[0].created_at.is_some());
        assert!(!list[0].orphaned && !list[0].missing);
        assert!(m2.get_store("persist_1").await.is_ok());

        assert!(m2.delete_database("persist_1").await.unwrap());
        assert!(!path.exists());
        assert!(m2.list_entries().await.is_empty());

        // Deletion is persisted too.
        let m3 = MultiDbManager::open(&db_dir).unwrap();
        assert!(m3.list_entries().await.is_empty());
        assert!(!m3.delete_database("persist_1").await.unwrap());
    }

    #[tokio::test]
    async fn test_orphan_and_missing_are_reported_honestly() {
        let dir = tempfile::tempdir().unwrap();
        let db_dir = dir.path().join("db");
        let m = MultiDbManager::open(&db_dir).unwrap();
        let db = m
            .create_database("known".into(), "Known".into(), WorkspaceDbKind::Org)
            .await
            .unwrap();
        drop(m);

        std::fs::write(db_dir.join("stray.sqlite"), b"").unwrap();
        std::fs::write(db_dir.join("bad name.sqlite"), b"").unwrap();
        std::fs::remove_file(&db.db_path).unwrap();

        let m = MultiDbManager::open(&db_dir).unwrap();
        let list = m.list_entries().await;
        assert_eq!(list.len(), 2, "invalid-named file must not be listed");
        let known = list.iter().find(|d| d.db_id == "known").unwrap();
        assert!(known.missing && !known.orphaned);
        let stray = list.iter().find(|d| d.db_id == "stray").unwrap();
        assert!(stray.orphaned && !stray.missing);
        assert_eq!(stray.kind, None);

        // Never auto-deleted.
        assert!(db_dir.join("stray.sqlite").exists());
        // Explicit delete of the orphan works.
        assert!(m.delete_database("stray").await.unwrap());
        assert!(!db_dir.join("stray.sqlite").exists());
    }

    #[tokio::test]
    async fn test_config_roundtrips_verbatim_and_corrupt_registry_is_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let db_dir = dir.path().join("db");
        let m = MultiDbManager::open(&db_dir).unwrap();
        m.create_database("cfg".into(), "Cfg".into(), WorkspaceDbKind::Personal)
            .await
            .unwrap();
        {
            let mut dbs = m.databases.write().await;
            dbs.get_mut("cfg").unwrap().config = Some(serde_json::json!({"x": [1, 2]}));
            m.persist(&dbs).unwrap();
        }
        let m = MultiDbManager::open(&db_dir).unwrap();
        assert_eq!(
            m.list_entries().await[0].config,
            Some(serde_json::json!({"x": [1, 2]}))
        );
        drop(m);

        std::fs::write(db_dir.join(REGISTRY_FILE), b"{not json").unwrap();
        let m = MultiDbManager::open(&db_dir).unwrap();
        let list = m.list_entries().await;
        assert_eq!(list.len(), 1);
        assert!(list[0].orphaned);
        assert!(db_dir.join("registry.json.corrupt").exists());
    }
}
