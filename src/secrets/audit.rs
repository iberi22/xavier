//! Audit logging for secret management
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use super::lending::AuditLogger;
use crate::codebase::connection_manager::ConnectionManager;
use crate::ports::outbound::schema_init::SchemaInitializer;
use chrono::Utc;
use rusqlite::params;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct AuditLogEntry {
    pub id: i64,
    pub timestamp: String,
    pub event_type: String,
    pub agent_id: String,
    pub session_token: String,
    pub secret_id: Option<String>,
    pub reason: Option<String>,
}

pub struct QmdAuditLogger {
    project_id: String,
}

impl Default for QmdAuditLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl QmdAuditLogger {
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

    pub async fn get_recent_logs(&self, limit: usize) -> anyhow::Result<Vec<AuditLogEntry>> {
        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, timestamp, event_type, agent_id, session_token, secret_id, reason
                     FROM secret_audit_logs ORDER BY timestamp DESC LIMIT ?",
                )?;
                let rows = stmt.query_map(params![limit], |row| {
                    Ok(AuditLogEntry {
                        id: row.get(0)?,
                        timestamp: row.get(1)?,
                        event_type: row.get(2)?,
                        agent_id: row.get(3)?,
                        session_token: row.get(4)?,
                        secret_id: row.get(5)?,
                        reason: row.get(6)?,
                    })
                })?;

                let mut logs = Vec::new();
                for log in rows {
                    logs.push(log?);
                }
                Ok(logs)
            })
            .await
    }

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
}
