//! Codebase connection manager
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use anyhow::{Context, Result};
use dashmap::DashMap;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

pub static INSTANCE: once_cell::sync::OnceCell<ConnectionManager> = once_cell::sync::OnceCell::new();

/// Unified SQLite connection manager for Xavier.
/// Manages connection pools by project_id with LRU eviction and PRAGMA optimizations.
pub struct ConnectionManager {
    pools: DashMap<String, ProjectPool>,
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
    fn on_acquire(&self, conn: &mut Connection) -> std::result::Result<(), rusqlite::Error> {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; \
             PRAGMA busy_timeout=5000; \
             PRAGMA synchronous=NORMAL;",
        )
        .map_err(|e| {
            eprintln!("PragmaCustomizer: PRAGMA error: {}", e);
            e
        })
    }
}

impl ConnectionManager {
    /// Create a new connection manager instance.
    pub fn new() -> Self {
        Self {
            pools: DashMap::new(),
            active: Arc::new(tokio::sync::RwLock::new(None)),
            idle_timeout_secs: 1800, // 30 minutes
        }
    }

    /// Get the global singleton instance.
    pub fn global() -> &'static Self {
        INSTANCE.get_or_init(Self::new)
    }

    /// Connect to a database by project_id.
    /// If the pool doesn't exist, it is created lazily.
    pub fn connect(&self, project_id: &str, project_root: &str) -> Result<()> {
        if !self.pools.contains_key(project_id) {
            let db_path = if project_id == "memory" {
                PathBuf::from(project_root).join("xavier_memory.db")
            } else if project_id == "vec_store" {
                PathBuf::from(project_root).join("vec-store.sqlite3")
            } else if project_id == "metrics" {
                PathBuf::from(project_root).join("metrics.db")
            } else if project_id.starts_with("conv_") {
                let pid = project_id.strip_prefix("conv_")
                    .ok_or_else(|| anyhow::anyhow!("invalid conversation prefix"))?;
                dirs::home_dir()
                    .ok_or_else(|| anyhow::anyhow!("could not find home directory"))?
                    .join(".xavier")
                    .join("conversations")
                    .join(format!("{}.db", pid))
            } else {
                PathBuf::from(project_root).join(".xavier").join("codebase.db")
            };

            if let Some(parent) = db_path.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("failed to create parent dir for {:?}", db_path))?;
                }
            }

            let manager = SqliteConnectionManager::file(db_path);
            let pool = Pool::builder()
                .max_size(10)
                .connection_customizer(Box::new(PragmaCustomizer))
                .build(manager)
                .context("failed to build r2d2 SQLite pool")?;

            self.evict_if_needed();

            self.pools.insert(
                project_id.to_string(),
                ProjectPool {
                    pool: Arc::new(pool),
                    activated_at: Instant::now(),
                },
            );
        } else {
            // Update last accessed time
            if let Some(mut entry) = self.pools.get_mut(project_id) {
                entry.activated_at = Instant::now();
            }
        }
        Ok(())
    }

    /// Manually disconnect and drop a pool.
    pub fn disconnect(&self, project_id: &str) {
        self.pools.remove(project_id);
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
            active.clone().ok_or_else(|| anyhow::anyhow!("no active project set"))?
        };

        self.with_conn(&active_id, f).await
    }

    /// Execute a query closure using a connection from a specific project's pool.
    pub async fn with_conn<F, T>(&self, project_id: &str, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let pool = {
            let mut entry = self.pools.get_mut(project_id)
                .ok_or_else(|| anyhow::anyhow!("pool for {} not found", project_id))?;
            entry.activated_at = Instant::now();
            entry.pool.clone()
        };

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
        let pool = {
            let mut entry = self.pools.get_mut(project_id)
                .ok_or_else(|| anyhow::anyhow!("pool for {} not found", project_id))?;
            entry.activated_at = Instant::now();
            entry.pool.clone()
        };

        tokio::task::spawn_blocking(move || f(&pool))
            .await
            .context("blocking task panicked")?
    }

    /// Evict pools that are idle for too long or if we exceed the max capacity (10).
    fn evict_if_needed(&self) {
        let now = Instant::now();

        // 1. Remove expired pools
        self.pools.retain(|_, pool| {
            now.duration_since(pool.activated_at).as_secs() < self.idle_timeout_secs
        });

        // 2. If still too many, remove least recently used
        while self.pools.len() >= 10 {
            let oldest = self.pools.iter()
                .map(|e| (e.key().clone(), e.activated_at))
                .min_by_key(|e| e.1);

            if let Some((key, _)) = oldest {
                self.pools.remove(&key);
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
        assert_eq!(cm.pools.len(), 2);
        assert!(cm.pools.contains_key("p1"));
        assert!(cm.pools.contains_key("p2"));
    }

    #[tokio::test]
    async fn test_active_project() {
        let cm = ConnectionManager::new();
        let dir = tempdir().unwrap();
        let project_root = dir.path().to_str().unwrap();

        cm.set_active("p1", project_root).await.unwrap();

        let res = cm.with_active(|conn| {
            conn.execute("CREATE TABLE IF NOT EXISTS t(id INT)", [])?;
            Ok(())
        }).await;

        assert!(res.is_ok());
    }
}
