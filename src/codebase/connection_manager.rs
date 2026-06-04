use anyhow::{Context, Result};
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
    pub fn global() -> &'static Self {
        INSTANCE.get_or_init(|| Self {
            pools: DashMap::new(),
            active: Arc::new(tokio::sync::RwLock::new(None)),
            idle_timeout_secs: 1800,
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
                    .ok_or_else(|| anyhow::anyhow!("could not find home directory"))?
                    .join(".xavier")
                    .join("conversations")
                    .join(format!("{}.db", pid))
            } else {
                PathBuf::from(project_root).join(".xavier").join("codebase.db")
            };

            if let Some(parent) = db_path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create parent dir for {:?}", db_path))?;
            }

            let manager = SqliteConnectionManager::file(db_path);
            let pool = Pool::builder()
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
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let active_id = self.active.read().await;
        let project_id = active_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no active project set"))?
            .clone();
        drop(active_id);

        let entry = self
            .pools
            .get(&project_id)
            .ok_or_else(|| anyhow::anyhow!("pool for {} not found", project_id))?;

        let pool = entry.pool.clone();
        drop(entry);

        // r2d2 is sync — use spawn_blocking for async interop
        let result = tokio::task::spawn_blocking(move || {
            let conn = pool
                .get()
                .map_err(|e| anyhow::anyhow!("failed to get connection: {}", e))?;
            f(&conn)
        })
        .await
        .context("blocking task panicked")??;

        Ok(result)
    }

    pub async fn with_pool<F, T>(&self, project_id: &str, f: F) -> Result<T>
    where
        F: FnOnce(&Pool<SqliteConnectionManager>) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let entry = self
            .pools
            .get(project_id)
            .ok_or_else(|| anyhow::anyhow!("pool for {} not found", project_id))?;
        let pool = entry.pool.clone();
        drop(entry);

        let result = tokio::task::spawn_blocking(move || f(&pool))
            .await
            .context("blocking task panicked")??;
        Ok(result)
    }

    fn evict_if_needed(&self) {
        let now = Instant::now();

        self.pools.retain(|_, pool| {
            now.duration_since(pool.activated_at).as_secs() < self.idle_timeout_secs
        });

        if self.pools.len() >= 10 {
            let mut entries: Vec<_> = self
                .pools
                .iter()
                .map(|e| (e.key().clone(), e.activated_at))
                .collect();
            entries.sort_by_key(|e| e.1);
            if let Some((key, _)) = entries.first() {
                self.pools.remove(key);
            }
        }
    }
}
