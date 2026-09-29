//! Audit logging for secret management
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use super::lending::AuditLogger;
use crate::codebase::connection_manager::ConnectionManager;
use crate::ports::outbound::schema_init::SchemaInitializer;
use chrono::{DateTime, Utc};
use rusqlite::params;

/// Schema for the lease persistence table -- metadata only, no secret value column.
const CREATE_LEASES_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS secret_leases (
    token TEXT PRIMARY KEY,
    secret_name TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    revoked_at TEXT
)";

/// A `secret_leases` row, restored on process start by `KeyLendingEngine`.
pub(crate) struct LeaseRow {
    pub token: String,
    pub secret_name: String,
    pub agent_id: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

fn parse_rfc3339(raw: &str) -> rusqlite::Result<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })
}

/// Persists a newly granted lease. Never receives or stores the secret value.
pub(crate) async fn insert_lease(
    project_id: &str,
    token: &str,
    secret_name: &str,
    agent_id: &str,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
) -> anyhow::Result<()> {
    let project_id = project_id.to_string();
    let token = token.to_string();
    let secret_name = secret_name.to_string();
    let agent_id = agent_id.to_string();
    let created_at = created_at.to_rfc3339();
    let expires_at = expires_at.to_rfc3339();
    ConnectionManager::global()
        .with_conn(&project_id, move |conn| {
            conn.execute(CREATE_LEASES_TABLE_SQL, ())?;
            conn.execute(
                "INSERT OR REPLACE INTO secret_leases
                 (token, secret_name, agent_id, created_at, expires_at, revoked_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
                params![token, secret_name, agent_id, created_at, expires_at],
            )?;
            Ok(())
        })
        .await
}

/// Marks a lease as revoked (used by `revoke`, `revoke_for_agent`,
/// `cleanup_expired`, and by restore of a lease that lapsed while down).
pub(crate) async fn mark_lease_revoked(project_id: &str, token: &str) -> anyhow::Result<()> {
    let project_id = project_id.to_string();
    let token = token.to_string();
    let now = Utc::now().to_rfc3339();
    ConnectionManager::global()
        .with_conn(&project_id, move |conn| {
            conn.execute(CREATE_LEASES_TABLE_SQL, ())?;
            conn.execute(
                "UPDATE secret_leases SET revoked_at = ?1 WHERE token = ?2",
                params![now, token],
            )?;
            Ok(())
        })
        .await
}

/// Updates the persisted expiry for a lease (used by `renew` and `backoff`).
pub(crate) async fn update_lease_expiry(
    project_id: &str,
    token: &str,
    expires_at: DateTime<Utc>,
) -> anyhow::Result<()> {
    let project_id = project_id.to_string();
    let token = token.to_string();
    let expires_at = expires_at.to_rfc3339();
    ConnectionManager::global()
        .with_conn(&project_id, move |conn| {
            conn.execute(CREATE_LEASES_TABLE_SQL, ())?;
            conn.execute(
                "UPDATE secret_leases SET expires_at = ?1 WHERE token = ?2",
                params![expires_at, token],
            )?;
            Ok(())
        })
        .await
}

/// Rows that are neither revoked nor known-expired -- the caller still
/// checks `expires_at` against its own clock to decide what's lapsed.
pub(crate) async fn active_leases(project_id: &str) -> anyhow::Result<Vec<LeaseRow>> {
    let project_id = project_id.to_string();
    ConnectionManager::global()
        .with_conn(&project_id, move |conn| {
            conn.execute(CREATE_LEASES_TABLE_SQL, ())?;
            let mut stmt = conn.prepare(
                "SELECT token, secret_name, agent_id, created_at, expires_at
                 FROM secret_leases WHERE revoked_at IS NULL",
            )?;
            let rows = stmt
                .query_map((), |row| {
                    let created_raw: String = row.get(3)?;
                    let expires_raw: String = row.get(4)?;
                    Ok(LeaseRow {
                        token: row.get(0)?,
                        secret_name: row.get(1)?,
                        agent_id: row.get(2)?,
                        created_at: parse_rfc3339(&created_raw)?,
                        expires_at: parse_rfc3339(&expires_raw)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
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

    /// Init schema async. Also creates `secret_leases` so it coexists with
    /// `secret_audit_logs` in the same metrics database.
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
                conn.execute(CREATE_LEASES_TABLE_SQL, ())?;
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

#[cfg(test)]
impl QmdAuditLogger {
    /// Test-only constructor pointed at an already-connected, isolated
    /// project id (e.g. a tempdir-scoped database), so tests never share the
    /// real `metrics.db` used by production and other tests.
    pub(crate) fn for_project(project_id: &str) -> Self {
        Self {
            project_id: project_id.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Connects a fresh project id to a tempdir-scoped database file so
    /// these tests never touch the real `metrics.db` (shared by production
    /// and other test modules). The returned `TempDir` must be kept alive
    /// for as long as the connection is used.
    fn isolated_project(name: &str) -> (String, tempfile::TempDir) {
        let project_id = format!("test_leases_{}_{}", name, uuid::Uuid::new_v4());
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("metrics.db");
        ConnectionManager::global()
            .connect_with_path(&project_id, db_path)
            .expect("connect isolated metrics db");
        (project_id, dir)
    }

    #[tokio::test]
    async fn test_secret_leases_coexists_with_secret_audit_logs() {
        let (project_id, _dir) = isolated_project("coexist");
        let logger = QmdAuditLogger::for_project(&project_id);
        logger.init_schema_async().await.expect("schema init");

        let tables: Vec<String> = ConnectionManager::global()
            .with_conn(&project_id, |conn| {
                let mut stmt = conn.prepare(
                    "SELECT name FROM sqlite_master WHERE type = 'table'
                     AND name IN ('secret_audit_logs', 'secret_leases')",
                )?;
                let rows = stmt
                    .query_map((), |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
            .expect("query tables");

        assert!(tables.iter().any(|t| t == "secret_audit_logs"));
        assert!(tables.iter().any(|t| t == "secret_leases"));
    }

    #[tokio::test]
    async fn test_secret_leases_schema_holds_no_secret_value() {
        let (project_id, _dir) = isolated_project("schema");
        let logger = QmdAuditLogger::for_project(&project_id);
        logger.init_schema_async().await.expect("schema init");

        let columns: Vec<String> = ConnectionManager::global()
            .with_conn(&project_id, |conn| {
                let mut stmt = conn.prepare("PRAGMA table_info(secret_leases)")?;
                let rows = stmt
                    .query_map((), |row| row.get::<_, String>(1))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
            .expect("query schema");

        assert_eq!(
            columns,
            vec![
                "token",
                "secret_name",
                "agent_id",
                "created_at",
                "expires_at",
                "revoked_at",
            ]
        );
        for column in &columns {
            assert!(!column.to_lowercase().contains("value"));
        }
    }

    #[tokio::test]
    async fn test_insert_and_revoke_lease_round_trip() {
        let (project_id, _dir) = isolated_project("roundtrip");
        let now = Utc::now();
        let expires = now + chrono::Duration::seconds(60);

        insert_lease(&project_id, "tok-1", "svc", "agent-1", now, expires)
            .await
            .expect("insert lease");

        let active = active_leases(&project_id).await.expect("active leases");
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].token, "tok-1");
        assert_eq!(active[0].agent_id, "agent-1");

        mark_lease_revoked(&project_id, "tok-1")
            .await
            .expect("revoke lease");

        let active_after = active_leases(&project_id)
            .await
            .expect("active leases after revoke");
        assert!(active_after.is_empty());
    }
}
