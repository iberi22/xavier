use anyhow::{Context, Result};
use libsql::Connection;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use moka::future::Cache;

pub static INSTANCE: once_cell::sync::OnceCell<ConnectionManager> = once_cell::sync::OnceCell::new();

pub struct ConnectionManager {
    cache: Cache<String, Connection>,
    active: Arc<tokio::sync::RwLock<Option<String>>>,
}

impl ConnectionManager {
    pub fn global() -> &'static Self {
        INSTANCE.get_or_init(|| Self {
            cache: Cache::builder()
                .max_capacity(10)
                .time_to_idle(Duration::from_secs(1800))
                .build(),
            active: Arc::new(tokio::sync::RwLock::new(None)),
        })
    }

    pub async fn connect(&self, project_id: &str, project_root: &str) -> Result<()> {
        if !self.cache.contains_key(project_id) {
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

            let path_str = db_path.to_str()
                .ok_or_else(|| anyhow::anyhow!("invalid db path: {:?}", db_path))?;

            let db = libsql::Builder::new_local(path_str)
                .build()
                .await
                .context("failed to build libSQL database")?;

            let conn = db.connect().context("failed to connect to libSQL database")?;

            conn.execute_batch(
                "PRAGMA journal_mode=WAL; \
                 PRAGMA synchronous=NORMAL;",
            ).await.context("failed to set pragmas")?;

            self.cache.insert(project_id.to_string(), conn).await;
        }

        Ok(())
    }

    pub async fn disconnect(&self, project_id: &str) {
        self.cache.invalidate(project_id).await;
    }

    pub async fn set_active(&self, project_id: &str, project_root: &str) -> Result<()> {
        self.connect(project_id, project_root).await?;
        let mut active = self.active.write().await;
        *active = Some(project_id.to_string());
        Ok(())
    }

    pub async fn with_active<F, Fut, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(Connection) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        let active_id = self.active.read().await;
        let project_id = active_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no active project set"))?
            .clone();
        drop(active_id);

        let conn = self.cache.get(&project_id).await
            .ok_or_else(|| anyhow::anyhow!("connection for {} not found in cache", project_id))?;

        f(conn).await
    }

    pub async fn with_conn<F, Fut, T>(&self, project_id: &str, f: F) -> Result<T>
    where
        F: FnOnce(Connection) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        let conn = self.cache.get(project_id).await
            .ok_or_else(|| anyhow::anyhow!("connection for {} not found in cache", project_id))?;

        f(conn).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_connection_manager() {
        let dir = tempdir().unwrap();
        let project_root = dir.path().to_str().unwrap();
        let cm = ConnectionManager {
            cache: Cache::builder().max_capacity(2).build(),
            active: Arc::new(tokio::sync::RwLock::new(None)),
        };

        cm.connect("p1", project_root).await.unwrap();
        cm.connect("p2", project_root).await.unwrap();

        cm.cache.run_pending_tasks().await;
        assert_eq!(cm.cache.entry_count(), 2);

        cm.connect("p3", project_root).await.unwrap();
        // Moka eviction is eventual, but it should happen
        tokio::time::sleep(Duration::from_millis(100)).await;
        cm.cache.run_pending_tasks().await;
        assert!(cm.cache.entry_count() <= 2);
    }

    #[tokio::test]
    async fn test_active_project() {
        let dir = tempdir().unwrap();
        let project_root = dir.path().to_str().unwrap();
        let cm = ConnectionManager {
            cache: Cache::builder().max_capacity(10).build(),
            active: Arc::new(tokio::sync::RwLock::new(None)),
        };

        cm.set_active("p1", project_root).await.unwrap();

        let res = cm.with_active(|conn| async move {
            conn.execute("CREATE TABLE t(id INT)", ()).await.unwrap();
            Ok(())
        }).await;

        assert!(res.is_ok());
    }
}
