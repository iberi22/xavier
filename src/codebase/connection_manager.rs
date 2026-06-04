use dashmap::DashMap;
use once_cell::sync::OnceCell;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use anyhow::{Context, Result};

pub static CONNECTION_MANAGER: OnceCell<ConnectionManager> = OnceCell::new();

pub struct ConnectionManager {
    pools: DashMap<String, ProjectPool>,
    active: Arc<tokio::sync::RwLock<Option<String>>>,
    idle_timeout: Duration,
}

struct ProjectPool {
    pool: Arc<Pool<SqliteConnectionManager>>,
    activated_at: Instant,
}

impl ConnectionManager {
    pub fn global() -> &'static Self {
        CONNECTION_MANAGER.get_or_init(|| Self {
            pools: DashMap::new(),
            active: Arc::new(tokio::sync::RwLock::new(None)),
            idle_timeout: Duration::from_secs(1800), // 30 minutes
        })
    }

    pub fn connect(&self, project_id: &str, project_root: PathBuf) -> Result<()> {
        if self.pools.contains_key(project_id) {
            return Ok(());
        }

        if self.pools.len() >= 10 {
            self.cleanup_lru();
        }

        let db_path = project_root.join(".xavier").join("codebase.db");
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let manager = SqliteConnectionManager::file(&db_path)
            .with_init(|conn| {
                conn.execute_batch(
                    "PRAGMA journal_mode=WAL; \
                     PRAGMA busy_timeout=5000; \
                     PRAGMA synchronous=NORMAL; \
                     PRAGMA foreign_keys=ON;",
                )?;
                Ok(())
            });

        let pool = Pool::builder()
            .max_size(10)
            .build(manager)
            .with_context(|| format!("failed to create pool for project {}", project_id))?;

        self.pools.insert(project_id.to_string(), ProjectPool {
            pool: Arc::new(pool),
            activated_at: Instant::now(),
        });

        Ok(())
    }

    /// Internal version of connect that takes a direct DB path (used by generic stores)
    pub fn connect_path(&self, db_key: &str, db_path: PathBuf) -> Result<()> {
        if self.pools.contains_key(db_key) {
            return Ok(());
        }

        if self.pools.len() >= 10 {
            self.cleanup_lru();
        }

        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let manager = SqliteConnectionManager::file(&db_path)
            .with_init(|conn| {
                conn.execute_batch(
                    "PRAGMA journal_mode=WAL; \
                     PRAGMA busy_timeout=5000; \
                     PRAGMA synchronous=NORMAL; \
                     PRAGMA foreign_keys=ON;",
                )?;
                Ok(())
            });

        let pool = Pool::builder()
            .max_size(10)
            .build(manager)
            .with_context(|| format!("failed to create pool for path {}", db_path.display()))?;

        self.pools.insert(db_key.to_string(), ProjectPool {
            pool: Arc::new(pool),
            activated_at: Instant::now(),
        });

        Ok(())
    }

    pub fn disconnect(&self, project_id: &str) {
        self.pools.remove(project_id);
    }

    pub async fn set_active(&self, project_id: &str, project_root: PathBuf) -> Result<()> {
        self.connect(project_id, project_root)?;
        let mut active = self.active.write().await;
        *active = Some(project_id.to_string());
        Ok(())
    }

    pub async fn with_active<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let active_id = self.active.read().await;
        let id = active_id.as_deref().context("no active project")?;

        let project_pool = self.pools.get(id).context("active project pool not found")?;
        let pool = project_pool.pool.clone();
        drop(project_pool);

        let conn = pool.get()?;
        f(&conn)
    }

    pub fn get_pool(&self, id: &str) -> Result<Arc<Pool<SqliteConnectionManager>>> {
        let project_pool = self.pools.get(id).context(format!("pool not found: {}", id))?;
        Ok(project_pool.pool.clone())
    }

    fn cleanup_lru(&self) {
        let mut items: Vec<_> = self.pools.iter().map(|r| (r.key().clone(), r.value().activated_at)).collect();
        items.sort_by_key(|&(_, time)| time);

        let now = Instant::now();
        for (key, activated_at) in items {
            if self.pools.len() <= 5 || now.duration_since(activated_at) > self.idle_timeout {
                self.pools.remove(&key);
                if self.pools.len() <= 5 {
                    break;
                }
            }
        }
    }
}
