use std::sync::Arc;
use anyhow::{Context, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use chrono::Utc;

use crate::codebase::connection_manager::ConnectionManager;

pub struct ThreatStore {
    pool: Arc<Pool<SqliteConnectionManager>>,
}

impl ThreatStore {
    pub fn new(pool: Arc<Pool<SqliteConnectionManager>>) -> Self {
        Self { pool }
    }

    pub fn from_env() -> Result<Self> {
        let manager = ConnectionManager::global();
        let settings = crate::settings::XavierSettings::current();
        manager.connect("security", &settings.memory.data_dir)?;
        let pool = manager.get_pool("security")?;
        Ok(Self { pool })
    }

    pub async fn init_schema(&self) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS threats (
                id TEXT PRIMARY KEY,
                category TEXT NOT NULL,
                severity TEXT NOT NULL,
                description TEXT NOT NULL,
                detected_at TEXT NOT NULL,
                metadata TEXT NOT NULL DEFAULT '{}'
            );
            CREATE INDEX IF NOT EXISTS idx_threats_category ON threats(category);",
        )?;
        Ok(())
    }

    pub async fn record_threat(
        &self,
        category: &str,
        severity: &str,
        description: &str,
        metadata: serde_json::Value,
    ) -> Result<()> {
        let id = ulid::Ulid::new().to_string();
        let now = Utc::now().to_rfc3339();
        let metadata_str = serde_json::to_string(&metadata).unwrap_or_else(|_| "{}".to_string());

        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO threats (id, category, severity, description, detected_at, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, category, severity, description, now, metadata_str],
        )?;
        Ok(())
    }
}
