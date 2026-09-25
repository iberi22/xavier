//! Codebase connection manager
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use anyhow::{Context, Result};
use parking_lot::RwLock;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, ErrorCode};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub static INSTANCE: std::sync::OnceLock<ConnectionManager> = std::sync::OnceLock::new();

const MAX_POOLS: usize = 16;
const WAL_INIT_ATTEMPTS: usize = 10;
const WAL_INIT_RETRY_DELAY: Duration = Duration::from_millis(50);

static WAL_INIT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Unified SQLite connection manager for Xavier.
/// Manages connection pools by project_id with LRU eviction and PRAGMA optimizations.
pub struct ConnectionManager {
    pools: RwLock<std::collections::HashMap<String, ProjectPool>>,
    active: Arc<tokio::sync::RwLock<Option<String>>>,
    idle_timeout_secs: u64,
}

struct ProjectPool {
    pool: Arc<Pool<SqliteConnectionManager>>,
    activated_at: Instant,
}

#[derive(Debug)]
struct PragmaCustomizer;

impl r2d2::CustomizeConnection<Connection, rusqlite::Error> for PragmaCustomizer {
    /// Only applies the cheap, connection-local PRAGMA layer because this
    /// callback can run on every acquire.
    ///
    /// The heavy settings are applied once per connection by the
    /// [`SqliteConnectionManager::with_init`] callback installed in
    /// [`ConnectionManager::connect_with_path`], and no layer checkpoints the
    /// WAL. Acquiring a pooled connection therefore never blocks behind another
    /// connection's readers (previously a `wal_checkpoint(TRUNCATE)` could stall
    /// every acquire for up to `busy_timeout`).
    fn on_acquire(&self, conn: &mut Connection) -> std::result::Result<(), rusqlite::Error> {
        crate::storage::apply_acquire_pragmas(conn).map_err(|e| {
            eprintln!("PragmaCustomizer: PRAGMA error: {}", e);
            e
        })
    }
}

/// One-time SQLite/WAL setup for a database file, run when its pool is built.
///
/// Applies the full (heavy) PRAGMA layer and switches the file to WAL mode,
/// retrying briefly when another process holds a conflicting lock. Does not
/// checkpoint the WAL: that is maintenance work, not connection setup (see
/// `crate::storage::pragma`).
fn initialize_wal_mode(conn: &Connection, db_path: &PathBuf) -> Result<()> {
    let _guard = WAL_INIT_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("SQLite WAL initialization lock was poisoned"))?;

    crate::storage::apply_pragmas(conn)
        .with_context(|| format!("failed to configure SQLite pragmas at {:?}", db_path))?;

    for attempt in 1..=WAL_INIT_ATTEMPTS {
        match conn.execute_batch("PRAGMA journal_mode=WAL;") {
            Ok(()) => break,
            Err(err) if is_sqlite_lock_error(&err) && attempt < WAL_INIT_ATTEMPTS => {
                std::thread::sleep(WAL_INIT_RETRY_DELAY);
            }
            Err(err) if is_sqlite_lock_error(&err) => {
                eprintln!(
                    "ConnectionManager: SQLite WAL initialization skipped for {:?}: {}",
                    db_path, err
                );
                break;
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("failed to initialize SQLite WAL mode at {:?}", db_path)
                });
            }
        }
    }

    // Opportunistic WAL truncation, moved out of the pool *acquire* path: it runs
    // once per pool construction and only when the WAL exceeded the threshold
    // (a `wal_checkpoint(TRUNCATE)` waits for every reader of the file, so running
    // it per acquire could stall every checkout behind a long-running read).
    let _ =
        crate::storage::maybe_wal_checkpoint(conn, crate::storage::WAL_CHECKPOINT_THRESHOLD_BYTES);

    Ok(())
}

fn is_sqlite_lock_error(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(error, _)
            if matches!(error.code, ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionManager {
    /// Create a new connection manager instance.
    pub fn new() -> Self {
        Self {
            pools: RwLock::new(std::collections::HashMap::new()),
            active: Arc::new(tokio::sync::RwLock::new(None)),
            idle_timeout_secs: 300, // 5 minutes
        }
    }

    /// Get the global singleton instance.
    pub fn global() -> &'static Self {
        INSTANCE.get_or_init(Self::new)
    }

    /// Connect to a database by project_id.
    /// If the pool doesn't exist, it is created lazily.
    pub fn connect(&self, project_id: &str, project_root: &str) -> Result<()> {
        if !self.pools.read().contains_key(project_id) {
            let db_path = if project_id == "memory" {
                PathBuf::from(project_root).join("xavier_memory.db")
            } else if project_id == "vec_store" {
                PathBuf::from(project_root).join("vec-store.sqlite3")
            } else if project_id == "metrics" {
                PathBuf::from(project_root).join("metrics.db")
            } else if project_id == "security" {
                PathBuf::from(project_root)
                    .join(".xavier")
                    .join("security.db")
            } else if project_id.starts_with("conv_test_") {
                PathBuf::from(project_root)
                    .join(".xavier")
                    .join("tests")
                    .join(format!("{}.db", project_id))
            } else if project_id.starts_with("conv_") {
                let pid = project_id
                    .strip_prefix("conv_")
                    .ok_or_else(|| anyhow::anyhow!("invalid conversation prefix"))?;
                dirs::home_dir()
                    .ok_or_else(|| anyhow::anyhow!("could not find home directory"))?
                    .join(".xavier")
                    .join("conversations")
                    .join(format!("{}.db", pid))
            } else if project_id.starts_with("test_") {
                PathBuf::from(project_root)
                    .join(".xavier")
                    .join("tests")
                    .join(format!("{}.db", project_id))
            } else {
                PathBuf::from(project_root)
                    .join(".xavier")
                    .join("codebase.db")
            };

            self.connect_with_path(project_id, db_path)
        } else {
            // Update last accessed time
            if let Some(entry) = self.pools.write().get_mut(project_id) {
                entry.activated_at = Instant::now();
            }
            Ok(())
        }
    }

    /// Explicitly connect to a database file with a given project_id.
    pub fn connect_with_path(&self, project_id: &str, db_path: PathBuf) -> Result<()> {
        if !self.pools.read().contains_key(project_id) {
            if let Some(parent) = db_path.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent).with_context(|| {
                        format!("failed to create parent dir for {:?}", db_path)
                    })?;
                }
            }

            let init_conn = Connection::open(&db_path).with_context(|| {
                format!("failed to initialize SQLite database at {:?}", db_path)
            })?;
            initialize_wal_mode(&init_conn, &db_path)?;
            drop(init_conn);

            // Heavy PRAGMAs are applied once per connection, when the pool opens
            // it; `PragmaCustomizer` only re-applies the cheap layer on acquire.
            let manager = SqliteConnectionManager::file(db_path).with_init(|conn| {
                crate::storage::apply_connection_pragmas(conn)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
            });
            let pool = Pool::builder()
                .max_size(10)
                .connection_customizer(Box::new(PragmaCustomizer))
                .build(manager)
                .context("failed to build r2d2 SQLite pool")?;

            self.evict_if_needed();

            self.pools.write().insert(
                project_id.to_string(),
                ProjectPool {
                    pool: Arc::new(pool),
                    activated_at: Instant::now(),
                },
            );
        } else if let Some(entry) = self.pools.write().get_mut(project_id) {
            entry.activated_at = Instant::now();
        }
        Ok(())
    }

    /// Get or open a cached `CodeGraphDB` instance for a given database path.
    pub fn get_code_graph_db(
        &self,
        db_path: &std::path::Path,
    ) -> Result<code_graph::db::CodeGraphDB> {
        code_graph::db::CodeGraphDB::new(db_path).map_err(|e| anyhow::anyhow!(e))
    }

    /// Unload and checkpoint a specific `CodeGraphDB` from memory.
    pub fn unload_code_graph_db(&self, db_path: &std::path::Path) -> bool {
        code_graph::db::unload_db(db_path)
    }

    /// Shutdown the connection manager and flush all SQLite WAL checkpoints cleanly.
    pub fn shutdown(&self) {
        self.pools.write().clear();
        code_graph::db::flush_and_close_cache();
    }

    /// Manually disconnect and drop a pool.
    pub fn disconnect(&self, project_id: &str) {
        self.pools.write().remove(project_id);
    }

    /// Set a project as active.
    pub async fn set_active(&self, project_id: &str, project_root: &str) -> Result<()> {
        self.connect(project_id, project_root)?;
        let mut active = self.active.write().await;
        *active = Some(project_id.to_string());
        Ok(())
    }

    /// Execute a query closure using a connection from the active pool.
    pub async fn with_active<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let active_id = {
            let active = self.active.read().await;
            active
                .clone()
                .ok_or_else(|| anyhow::anyhow!("no active project set"))?
        };

        self.with_conn(&active_id, f).await
    }

    /// Get an existing pool or lazily resurrect / reconnect it if evicted.
    pub fn get_or_reconnect_pool(
        &self,
        project_id: &str,
    ) -> Result<Arc<Pool<SqliteConnectionManager>>> {
        {
            let mut pools = self.pools.write();
            if let Some(entry) = pools.get_mut(project_id) {
                entry.activated_at = Instant::now();
                return Ok(entry.pool.clone());
            }
        }

        // Pool was evicted or not yet connected; attempt on-demand resurrection.
        self.connect(project_id, ".")?;

        let mut pools = self.pools.write();
        let entry = pools.get_mut(project_id).ok_or_else(|| {
            anyhow::anyhow!("pool for {} not found after reconnect attempt", project_id)
        })?;
        entry.activated_at = Instant::now();
        Ok(entry.pool.clone())
    }

    /// Execute a query closure using a connection from a specific project's pool.
    pub async fn with_conn<F, T>(&self, project_id: &str, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let pool = self.get_or_reconnect_pool(project_id)?;

        tokio::task::spawn_blocking(move || {
            let conn = pool.get().context("failed to get connection from pool")?;
            f(&conn)
        })
        .await
        .context("blocking task panicked")?
    }

    /// Access the underlying pool for a specific project.
    pub async fn with_pool<F, T>(&self, project_id: &str, f: F) -> Result<T>
    where
        F: FnOnce(&Pool<SqliteConnectionManager>) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let pool = self.get_or_reconnect_pool(project_id)?;

        tokio::task::spawn_blocking(move || f(&pool))
            .await
            .context("blocking task panicked")?
    }

    /// Evict pools that are idle for too long or if we exceed the max capacity.
    fn evict_if_needed(&self) {
        let now = Instant::now();
        let mut pools = self.pools.write();

        // 1. Remove expired pools (checkpoint WAL before removal)
        let expired: Vec<String> = pools
            .iter()
            .filter(|(_, pool)| {
                now.duration_since(pool.activated_at).as_secs() >= self.idle_timeout_secs
            })
            .map(|(k, _)| k.clone())
            .collect();
        for key in expired {
            if let Some(entry) = pools.get(&key) {
                if let Ok(conn) = entry.pool.get() {
                    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
                }
            }
            pools.remove(&key);
        }

        // 2. If still too many, remove least recently used (checkpoint WAL before removal)
        while pools.len() >= MAX_POOLS {
            let oldest = pools
                .iter()
                .map(|(k, v)| (k.clone(), v.activated_at))
                .min_by_key(|e| e.1);

            if let Some((key, _)) = oldest {
                if let Some(entry) = pools.get(&key) {
                    if let Ok(conn) = entry.pool.get() {
                        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
                    }
                }
                pools.remove(&key);
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_connection_manager() {
        let cm = ConnectionManager::new();
        let dir = tempdir().unwrap();
        let project_root = dir.path().to_str().unwrap();

        cm.connect("p1", project_root).unwrap();
        cm.connect("p2", project_root).unwrap();
        assert_eq!(cm.pools.read().len(), 2);
        assert!(cm.pools.read().contains_key("p1"));
        assert!(cm.pools.read().contains_key("p2"));
    }

    /// Pool acquires must still hand out fully configured connections after the
    /// pragma split: the one-time layer is applied when the pool is built and the
    /// cheap acquire layer on every checkout.
    #[tokio::test]
    async fn test_pooled_connections_carry_both_pragma_layers() {
        let cm = ConnectionManager::new();
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("pragma_layers.db");

        cm.connect_with_path("pragma_layers", db_path).unwrap();

        let pragmas = cm
            .with_conn("pragma_layers", |conn| {
                Ok((
                    conn.query_row("PRAGMA busy_timeout;", [], |r| r.get::<_, i64>(0))?,
                    conn.query_row("PRAGMA foreign_keys;", [], |r| r.get::<_, i64>(0))?,
                    conn.query_row("PRAGMA cache_size;", [], |r| r.get::<_, i64>(0))?,
                    conn.query_row("PRAGMA temp_store;", [], |r| r.get::<_, i64>(0))?,
                ))
            })
            .await
            .unwrap();

        // Acquire layer.
        assert_eq!(pragmas.0, 5000, "busy_timeout must be applied on acquire");
        assert_eq!(pragmas.1, 1, "foreign_keys must be applied on acquire");
        // One-time layer.
        assert_eq!(pragmas.2, -8000, "cache_size must be set at pool build");
        assert_eq!(pragmas.3, 2, "temp_store=MEMORY must be set at pool build");
    }

    /// Regression for issue #2544: a *new* write connection opened through
    /// `ConnectionManager` (the same mechanism `VecSqliteMemoryStore` and the
    /// MCP `create_memory` write path use) must have the sqlite-vec extension
    /// (`vec_f32`, `vec_distance_cosine`, ...) available — not just the
    /// connection that happened to be open at boot.
    ///
    /// Mirrors production ordering: register the extension via
    /// `sqlite3_auto_extension` (idempotent, safe to call from every test in
    /// this binary), *then* open a brand-new project pool + connection and
    /// exercise a real `vec_f32(...)` call, matching the embedding INSERT in
    /// `sqlite_vec_store`.
    #[tokio::test]
    async fn test_fresh_write_connection_has_vec_f32() {
        crate::memory::sqlite_vec_store::vector::register_sqlite_vec_extension()
            .expect("sqlite-vec extension registration must succeed");

        let cm = ConnectionManager::new();
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("vec_f32_write_conn.sqlite3");

        cm.connect_with_path("vec_f32_write_conn", db_path).unwrap();

        let version: String = cm
            .with_conn("vec_f32_write_conn", |conn| {
                Ok(conn.query_row("SELECT vec_version()", [], |row| row.get(0))?)
            })
            .await
            .expect("vec_version() must resolve on a fresh connection from a fresh pool");
        assert!(
            version.starts_with('v'),
            "unexpected vec_version() output: {version}"
        );

        // The exact call shape used on the memory-write path
        // (`INSERT ... VALUES (?1, ?2, vec_f32(?3))`): a real vec_f32() call
        // against a freshly opened write connection must not error with
        // "no such function: vec_f32".
        let blob: Vec<u8> = cm
            .with_conn("vec_f32_write_conn", |conn| {
                let embedding_json = serde_json::to_string(&[0.1_f32, 0.2, 0.3]).unwrap();
                conn.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, v BLOB)", [])?;
                conn.execute(
                    "INSERT INTO t (v) VALUES (vec_f32(?1))",
                    rusqlite::params![embedding_json],
                )?;
                Ok(conn.query_row("SELECT v FROM t WHERE id = 1", [], |row| row.get(0))?)
            })
            .await
            .expect("vec_f32() insert must succeed on a fresh write connection");
        assert_eq!(blob.len(), 3 * std::mem::size_of::<f32>());
    }

    #[tokio::test]
    async fn test_get_code_graph_db_and_shutdown() {
        let cm = ConnectionManager::new();
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("code_graph_cm.db");

        let cg1 = cm.get_code_graph_db(&db_path).unwrap();
        let cg2 = cm.get_code_graph_db(&db_path).unwrap();

        let sym = code_graph::types::Symbol {
            id: None,
            stable_id: None,
            name: "cm_test_symbol".to_string(),
            kind: code_graph::types::SymbolKind::Function,
            lang: code_graph::types::Language::Rust,
            file_path: "src/cm_test.rs".to_string(),
            start_line: 1,
            end_line: 2,
            start_col: 0,
            end_col: 0,
            signature: None,
            parent: None,
            complexity: None,
        };
        cg1.insert_symbol(&sym).unwrap();
        let found = cg2.find_by_name("cm_test_symbol", 1).unwrap();
        assert_eq!(found.len(), 1);

        cm.shutdown();
    }

    #[tokio::test]
    async fn test_active_project() {
        let cm = ConnectionManager::new();
        let dir = tempdir().unwrap();
        let project_root = dir.path().to_str().unwrap();

        cm.set_active("p1", project_root).await.unwrap();

        let res = cm
            .with_active(|conn| {
                conn.execute("CREATE TABLE IF NOT EXISTS t(id INT)", [])?;
                Ok(())
            })
            .await;

        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_eviction_and_resurrection() {
        let cm = ConnectionManager::new();
        let dir = tempdir().unwrap();
        let project_root = dir.path().to_str().unwrap();

        cm.connect("conv_test_resurrect", project_root).unwrap();
        assert!(cm.pools.read().contains_key("conv_test_resurrect"));

        // Force eviction
        cm.disconnect("conv_test_resurrect");
        assert!(!cm.pools.read().contains_key("conv_test_resurrect"));

        // with_conn should automatically reconnect / resurrect
        let res = cm
            .with_conn("conv_test_resurrect", |conn| {
                conn.execute("CREATE TABLE IF NOT EXISTS t_res(id INT)", [])?;
                Ok(())
            })
            .await;

        assert!(res.is_ok());
        assert!(cm.pools.read().contains_key("conv_test_resurrect"));
    }
}
