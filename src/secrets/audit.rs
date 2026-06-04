use std::sync::Arc;
use anyhow::{Context, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use chrono::Utc;

use crate::codebase::connection_manager::ConnectionManager;

pub struct AuditLogger {
    pool: Arc<Pool<SqliteConnectionManager>>,
}

impl AuditLogger {
    pub fn new(pool: Arc<Pool<SqliteConnectionManager>>) -> Self {
        Self { pool }
    }

    pub fn from_env() -> Result<Self> {
        let manager = ConnectionManager::global();
        let settings = crate::settings::XavierSettings::current();
        manager.connect("audit", &settings.memory.data_dir)?;
        let pool = manager.get_pool("audit")?;
        Ok(Self { pool })
    }

    pub async fn init_schema(&self) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS audit_logs (
                id TEXT PRIMARY KEY,
                action TEXT NOT NULL,
                actor TEXT NOT NULL,
                target TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                details TEXT NOT NULL DEFAULT '{}'
            );",
        )?;
        Ok(())
    }

    pub async fn log(
        &self,
        action: &str,
        actor: &str,
        target: &str,
        details: serde_json::Value,
    ) -> Result<()> {
        let id = ulid::Ulid::new().to_string();
        let now = Utc::now().to_rfc3339();
        let details_str = serde_json::to_string(&details).unwrap_or_else(|_| "{}".to_string());

        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO audit_logs (id, action, actor, target, timestamp, details)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, action, actor, target, now, details_str],
        )?;
        Ok(())
    }
}
