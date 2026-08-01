use crate::codebase::connection_manager::ConnectionManager;
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// An audit log entry representing a single permission check event.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuditLogEntry {
    pub id: String,
    pub user_id: String,
    pub role: String,
    pub permission: String,
    pub result: String,
    pub timestamp: DateTime<Utc>,
}

/// A structured logger for logging security events and permission checks.
pub struct AuditLogger {
    project_id: String,
}

impl Default for AuditLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditLogger {
    /// Creates a new AuditLogger with default "security" project_id.
    pub fn new() -> Self {
        Self {
            project_id: "security".to_string(),
        }
    }

    /// Creates a new AuditLogger with a custom project_id (useful for test isolation).
    pub fn with_project_id(project_id: impl Into<String>) -> Self {
        Self {
            project_id: project_id.into(),
        }
    }

    /// Initializes the `audit_log` table in the SQLite database.
    pub async fn init_schema(&self) -> Result<()> {
        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                conn.execute_batch(
                    r#"
                    CREATE TABLE IF NOT EXISTS audit_log (
                        id TEXT PRIMARY KEY,
                        user_id TEXT NOT NULL,
                        role TEXT NOT NULL,
                        permission TEXT NOT NULL,
                        result TEXT NOT NULL,
                        timestamp TEXT NOT NULL
                    );
                    CREATE INDEX IF NOT EXISTS idx_audit_log_timestamp ON audit_log(timestamp);
                    CREATE INDEX IF NOT EXISTS idx_audit_log_user ON audit_log(user_id);
                    "#,
                )?;
                Ok(())
            })
            .await
    }

    /// Logs a single permission check.
    pub async fn log_check(
        &self,
        user_id: &str,
        role: &str,
        permission: &str,
        result: bool,
    ) -> Result<AuditLogEntry> {
        let entry = AuditLogEntry {
            id: Uuid::new_v4().to_string(),
            user_id: user_id.to_string(),
            role: role.to_string(),
            permission: permission.to_string(),
            result: if result { "ALLOW".to_string() } else { "DENY".to_string() },
            timestamp: Utc::now(),
        };

        let id = entry.id.clone();
        let uid = entry.user_id.clone();
        let r = entry.role.clone();
        let p = entry.permission.clone();
        let res = entry.result.clone();
        let ts_str = entry.timestamp.to_rfc3339();

        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                conn.execute(
                    "INSERT INTO audit_log (id, user_id, role, permission, result, timestamp)
                     VALUES (?, ?, ?, ?, ?, ?)",
                    params![id, uid, r, p, res, ts_str],
                )?;
                Ok(())
            })
            .await?;

        Ok(entry)
    }

    /// Retrieves all logged permission check events.
    pub async fn list_logs(&self) -> Result<Vec<AuditLogEntry>> {
        ConnectionManager::global()
            .with_conn(&self.project_id, |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, user_id, role, permission, result, timestamp FROM audit_log ORDER BY timestamp DESC"
                )?;
                let rows = stmt.query_map([], |row| {
                    let ts_str: String = row.get(5)?;
                    let timestamp = ts_str
                        .parse::<DateTime<Utc>>()
                        .unwrap_or_else(|_| Utc::now());

                    Ok(AuditLogEntry {
                        id: row.get(0)?,
                        user_id: row.get(1)?,
                        role: row.get(2)?,
                        permission: row.get(3)?,
                        result: row.get(4)?,
                        timestamp,
                    })
                })?;

                let mut entries = Vec::new();
                for row in rows {
                    entries.push(row?);
                }
                Ok(entries)
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_audit_logger_lifecycle() {
        let temp = tempfile::tempdir().unwrap();
        let project_id = format!("test_security_audit_{}", Uuid::new_v4().simple());
        let root = temp.path().to_string_lossy().to_string();

        ConnectionManager::global()
            .connect(&project_id, &root)
            .unwrap();

        let logger = AuditLogger::with_project_id(project_id.clone());
        logger.init_schema().await.unwrap();

        // Log an allowed check
        let entry1 = logger
            .log_check("user-123", "admin", "delete", true)
            .await
            .unwrap();

        assert_eq!(entry1.user_id, "user-123");
        assert_eq!(entry1.role, "admin");
        assert_eq!(entry1.permission, "delete");
        assert_eq!(entry1.result, "ALLOW");

        // Log a denied check
        let entry2 = logger
            .log_check("user-456", "viewer", "write", false)
            .await
            .unwrap();

        assert_eq!(entry2.user_id, "user-456");
        assert_eq!(entry2.role, "viewer");
        assert_eq!(entry2.permission, "write");
        assert_eq!(entry2.result, "DENY");

        // List and assert logs
        let logs = logger.list_logs().await.unwrap();
        assert_eq!(logs.len(), 2);

        // Ordered by timestamp desc, so entry2 (more recent) is first
        assert_eq!(logs[0].user_id, "user-456");
        assert_eq!(logs[1].user_id, "user-123");

        ConnectionManager::global().disconnect(&project_id);
    }
}
