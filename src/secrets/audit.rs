//! Audit logging for secret management
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use super::lending::AuditLogger;
use crate::codebase::connection_manager::ConnectionManager;
use crate::ports::outbound::schema_init::SchemaInitializer;
use chrono::Utc;
use rusqlite::params;

pub struct QmdAuditLogger {
    project_id: String,
}

impl Default for QmdAuditLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl QmdAuditLogger {
    /// New.
    pub fn new() -> Self {
        let project_id = "metrics";
        if let Err(e) = ConnectionManager::global().connect(project_id, ".") {
            tracing::warn!(
                "QmdAuditLogger failed to connect to metrics database: {}",
                e
            );
        }
        Self {
            project_id: project_id.to_string(),
        }
    }

    /// Init schema async.
    pub async fn init_schema_async(&self) -> anyhow::Result<()> {
        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
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
                )?;
                Ok(())
            })
            .await
    }
}

impl SchemaInitializer for QmdAuditLogger {
    fn init_schema(&self) -> anyhow::Result<()> {
        match tokio::runtime::Handle::try_current() {
            Ok(_) => tokio::task::block_in_place(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .build()
                    .map_err(|e| anyhow::anyhow!("failed to create temporary runtime: {}", e))?;
                rt.block_on(self.init_schema_async())
            }),
            Err(_) => {
                let runtime = tokio::runtime::Runtime::new()
                    .map_err(|e| anyhow::anyhow!("failed to create tokio runtime: {}", e))?;
                runtime.block_on(self.init_schema_async())
            }
        }
    }
}

impl AuditLogger for QmdAuditLogger {
    fn log_lend(&self, agent_id: &str, secret_id: &str, session_token: &str, ttl_secs: u64) {
        let project_id = self.project_id.clone();
        let agent_id = agent_id.to_string();
        let secret_id = secret_id.to_string();
        let session_token = session_token.to_string();
        let now = Utc::now().to_rfc3339();
        tokio::spawn(async move {
            let _ = ConnectionManager::global().with_conn(&project_id, move |conn| {
                conn.execute(
                    "INSERT INTO secret_audit_logs (timestamp, event_type, agent_id, session_token, secret_id, reason)
                     VALUES (?, ?, ?, ?, ?, ?)",
                    params![now, "LEND", agent_id, session_token, secret_id, format!("TTL: {}s", ttl_secs)],
                )?;
                Ok(())
            }).await;
        });
    }

    fn log_revoke(&self, agent_id: &str, session_token: &str, reason: &str) {
        let project_id = self.project_id.clone();
        let agent_id = agent_id.to_string();
        let session_token = session_token.to_string();
        let reason = reason.to_string();
        let now = Utc::now().to_rfc3339();
        tokio::spawn(async move {
            let _ = ConnectionManager::global().with_conn(&project_id, move |conn| {
                conn.execute(
                    "INSERT INTO secret_audit_logs (timestamp, event_type, agent_id, session_token, reason)
                     VALUES (?, ?, ?, ?, ?)",
                    params![now, "REVOKE", agent_id, session_token, reason],
                )?;
                Ok(())
            }).await;
        });
    }

    fn log_proxy_use(&self, agent_id: &str, lease_token: &str, endpoint: &str) {
        let project_id = self.project_id.clone();
        let agent_id = agent_id.to_string();
        let session_token = lease_token.to_string();
        let reason = endpoint.to_string();
        let now = Utc::now().to_rfc3339();
        tokio::spawn(async move {
            let _ = ConnectionManager::global().with_conn(&project_id, move |conn| {
                conn.execute(
                    "INSERT INTO secret_audit_logs (timestamp, event_type, agent_id, session_token, reason)
                     VALUES (?, ?, ?, ?, ?)",
                    params![now, "PROXY_USE", agent_id, session_token, reason],
                )?;
                Ok(())
            }).await;
        });
    }
}
