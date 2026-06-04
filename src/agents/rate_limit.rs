use std::sync::Arc;
use anyhow::{Context, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use chrono::Utc;

use crate::codebase::connection_manager::ConnectionManager;

pub struct RateLimiter {
    pool: Arc<Pool<SqliteConnectionManager>>,
}

impl RateLimiter {
    pub fn new(pool: Arc<Pool<SqliteConnectionManager>>) -> Self {
        Self { pool }
    }

    pub fn from_env() -> Result<Self> {
        let manager = ConnectionManager::global();
        let settings = crate::settings::XavierSettings::current();
        manager.connect("rate_limit", &settings.memory.data_dir)?;
        let pool = manager.get_pool("rate_limit")?;
        Ok(Self { pool })
    }

    pub async fn init_schema(&self) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS rate_limits (
                key TEXT PRIMARY KEY,
                tokens REAL NOT NULL,
                last_updated TEXT NOT NULL
            );",
        )?;
        Ok(())
    }

    pub fn db(&self) -> Arc<Pool<SqliteConnectionManager>> {
        self.pool.clone()
    }
}
