//! Standalone ConnectionManager for xavier-core using r2d2 and r2d2_sqlite.
use anyhow::{Context, Result};
use parking_lot::RwLock;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

pub static INSTANCE: std::sync::OnceLock<ConnectionManager> = std::sync::OnceLock::new();

pub struct ConnectionManager {
    pools: RwLock<HashMap<String, Arc<Pool<SqliteConnectionManager>>>>,
}

#[derive(Debug)]
struct PragmaCustomizer;

impl r2d2::CustomizeConnection<Connection, rusqlite::Error> for PragmaCustomizer {
    fn on_acquire(&self, conn: &mut Connection) -> std::result::Result<(), rusqlite::Error> {
        conn.execute_batch(
            "PRAGMA busy_timeout=5000; \
             PRAGMA synchronous=NORMAL; \
             PRAGMA mmap_size=268435456; \
             PRAGMA foreign_keys=ON;",
        )
    }
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            pools: RwLock::new(HashMap::new()),
        }
    }

    pub fn global() -> &'static Self {
        INSTANCE.get_or_init(Self::new)
    }

    pub fn connect(&self, project_id: &str, project_root: &str) -> Result<()> {
        if !self.pools.read().contains_key(project_id) {
            let db_path = if project_id == "memory" {
                PathBuf::from(project_root).join("xavier_memory.db")
            } else {
                PathBuf::from(project_root).join(".xavier").join("codebase.db")
            };
            self.connect_with_path(project_id, db_path)?;
        }
        Ok(())
    }

    pub fn connect_with_path(&self, project_id: &str, db_path: PathBuf) -> Result<()> {
        if !self.pools.read().contains_key(project_id) {
            if let Some(parent) = db_path.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent).with_context(|| {
                        format!("failed to create parent dir for {:?}", db_path)
                    })?;
                }
            }

            // Test opening and initializing the database file
            let init_conn = Connection::open(&db_path).with_context(|| {
                format!("failed to initialize SQLite database at {:?}", db_path)
            })?;
            // Enable WAL mode
            let _ = init_conn.execute_batch("PRAGMA journal_mode=WAL;");
            drop(init_conn);

            let manager = SqliteConnectionManager::file(db_path);
            let pool = Pool::builder()
                .max_size(10)
                .connection_customizer(Box::new(PragmaCustomizer))
                .build(manager)
                .context("failed to build r2d2 SQLite pool")?;

            self.pools.write().insert(project_id.to_string(), Arc::new(pool));
        }
        Ok(())
    }

    pub async fn with_conn<F, T>(&self, project_id: &str, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let pool = {
            self.pools
                .read()
                .get(project_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("pool for {} not found", project_id))?
        };

        tokio::task::spawn_blocking(move || {
            let conn = pool.get().context("failed to get connection from pool")?;
            f(&conn)
        })
        .await
        .context("blocking task panicked")?
    }
}
