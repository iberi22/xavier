use anyhow::{anyhow, Result};
use dashmap::DashMap;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

pub static INSTANCE: once_cell::sync::OnceCell<ConnectionManager> = once_cell::sync::OnceCell::new();

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
    fn on_acquire(&self, conn: &mut Connection) -> Result<(), rusqlite::Error> {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; \
             PRAGMA busy_timeout=5000; \
             PRAGMA synchronous=NORMAL;",
        )
    }
}

impl ConnectionManager {
    pub fn global() -> &'static Self {
        INSTANCE.get_or_init(|| Self {
            pools: DashMap::new(),
            active: Arc::new(tokio::sync::RwLock::new(None)),
            idle_timeout_secs: 1800, // 30 minutes
        })
    }

    pub fn connect(&self, project_id: &str, project_root: &str) -> Result<()> {
        if !self.pools.contains_key(project_id) {
            let db_path = if project_id == "memory" {
                PathBuf::from(project_root).join("xavier_memory.db")
            } else if project_id == "vec_store" {
                PathBuf::from(project_root).join("vec-store.sqlite3")
            } else if project_id == "metrics" {
                PathBuf::from(project_root).join("metrics.db")
            } else if project_id.starts_with("conv_") {
                let pid = project_id.strip_prefix("conv_").unwrap();
                dirs::home_dir()
                    .ok_or_else(|| anyhow!("could not find home directory"))?
                    .join(".xavier")
                    .join("conversations")
                    .join(format!("{}.db", pid))
            } else {
                PathBuf::from(project_root).join(".xavier").join("codebase.db")
            };

            if let Some(parent) = db_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            let manager = SqliteConnectionManager::file(db_path);
            let pool = Pool::builder()
                .connection_customizer(Box::new(PragmaCustomizer))
                .build(manager)?;

            self.evict_if_needed();

            self.pools.insert(
                project_id.to_string(),
                ProjectPool {
                    pool: Arc::new(pool),
                    activated_at: Instant::now(),
                },
            );
        } else {
            // Update last access time for LRU
            if let Some(mut pool) = self.pools.get_mut(project_id) {
                pool.activated_at = Instant::now();
            }
        }
        Ok(())
    }

    pub fn disconnect(&self, project_id: &str) {
        self.pools.remove(project_id);
    }

    pub async fn set_active(&self, project_id: &str, project_root: &str) -> Result<()> {
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
        let project_id = active_id
            .as_ref()
            .ok_or_else(|| anyhow!("no active project set"))?;

        let entry = self
            .pools
            .get(project_id)
            .ok_or_else(|| anyhow!("pool for {} not found", project_id))?;

        let pool = entry.pool.clone();
        drop(entry); // release dashmap lock

        let conn = pool.get()?;
        f(&conn)
    }

    pub fn get_pool(&self, project_id: &str) -> Result<Arc<Pool<SqliteConnectionManager>>> {
        let entry = self
            .pools
            .get(project_id)
            .ok_or_else(|| anyhow!("pool for {} not found", project_id))?;
        Ok(entry.pool.clone())
    }

    fn evict_if_needed(&self) {
        let now = Instant::now();

        // 1. Remove TTL expired
        self.pools.retain(|_, pool| {
            now.duration_since(pool.activated_at).as_secs() < self.idle_timeout_secs
        });

        // 2. Max 10 connections LRU
        if self.pools.len() >= 10 {
            let mut entries: Vec<_> = self.pools.iter().map(|e| (e.key().clone(), e.activated_at)).collect();
            entries.sort_by_key(|e| e.1);
            if let Some((key, _)) = entries.first() {
                self.pools.remove(key);
            }
        }
    }
}
