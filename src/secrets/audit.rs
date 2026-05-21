use super::lending::AuditLogger;
use crate::ports::outbound::schema_init::SchemaInitializer;
use chrono::Utc;

use crate::utils::connection_pool::LibsqlConnectionPool;

pub struct QmdAuditLogger {
    pool: LibsqlConnectionPool,
}

impl QmdAuditLogger {
    pub fn new(pool: LibsqlConnectionPool) -> Self {
        Self { pool }
    }

    pub async fn init_schema_async(&self) -> anyhow::Result<()> {
        let conn = self.pool.get().await?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS secret_audit_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                event_type TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                session_token TEXT NOT NULL,
                secret_id TEXT,
                reason TEXT
            )",
            (),
        ).await?;
        Ok(())
    }
}

impl SchemaInitializer for QmdAuditLogger {
    fn init_schema(&self) -> anyhow::Result<()> {
        std::thread::scope(|s| {
            s.spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| anyhow::anyhow!("failed to build runtime for audit schema: {}", e))?;
                rt.block_on(self.init_schema_async())
            })
            .join()
            .map_err(|_| anyhow::anyhow!("audit schema thread panicked"))?
        })
    }
}

impl AuditLogger for QmdAuditLogger {
    fn log_lend(&self, agent_id: &str, secret_id: &str, session_token: &str, ttl_secs: u64) {
        let pool = self.pool.clone();
        let agent_id = agent_id.to_string();
        let secret_id = secret_id.to_string();
        let session_token = session_token.to_string();
        let now = Utc::now().to_rfc3339();
        tokio::spawn(async move {
            if let Ok(conn) = pool.get().await {
                let _ = conn.execute(
                    "INSERT INTO secret_audit_logs (timestamp, event_type, agent_id, session_token, secret_id, reason)
                     VALUES (?, ?, ?, ?, ?, ?)",
                    (now, "LEND", agent_id, session_token, secret_id, format!("TTL: {}s", ttl_secs)),
                ).await;
            }
        });
    }

    fn log_revoke(&self, agent_id: &str, session_token: &str, reason: &str) {
        let pool = self.pool.clone();
        let agent_id = agent_id.to_string();
        let session_token = session_token.to_string();
        let reason = reason.to_string();
        let now = Utc::now().to_rfc3339();
        tokio::spawn(async move {
            if let Ok(conn) = pool.get().await {
                let _ = conn.execute(
                    "INSERT INTO secret_audit_logs (timestamp, event_type, agent_id, session_token, reason)
                     VALUES (?, ?, ?, ?, ?)",
                    (now, "REVOKE", agent_id, session_token, reason),
                ).await;
            }
        });
    }
}
