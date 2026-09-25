//! Persistent storage for root-token sessions.
//!
//! Historically this store also held a second, parallel user/login/TOTP system
//! (`create_user`, `get_user_by_email`, TOTP secrets, recovery phrases, refresh-token
//! issuance, failed-login counters, ...) behind `/v1/auth/login` and friends. That system had
//! **zero production callers ever writing a user into it** and was removed as part of
//! <https://github.com/iberi22/xavier/issues/2545> — the real login lives in
//! `xavier::auth2` / `.xavier/auth.db`. What remains here backs only the still-active
//! `/v1/auth/sessions` and `/v1/auth/sessions/{id}` root-token session endpoints
//! (`cli::handlers::auth::list_sessions_handler` / `revoke_session_handler`), which operate
//! purely on the `refresh_tokens` table and never touch `users`.
use anyhow::{anyhow, Result};
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

/// Persistent storage backing root-token sessions.
pub struct AuthStore {
    conn: Mutex<Connection>,
}

impl AuthStore {
    fn get_conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|e| anyhow!("Database lock poisoned: {}", e))
    }

    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<()> {
        self.get_conn()?.execute_batch(
            "CREATE TABLE IF NOT EXISTS refresh_tokens (
                token TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                expires_at INTEGER NOT NULL,
                revoked INTEGER NOT NULL DEFAULT 0
            );",
        )?;
        Ok(())
    }

    /// Revoke user session.
    pub fn revoke_user_session(&self, user_id: &str, token: &str) -> Result<()> {
        self.get_conn()?.execute(
            "UPDATE refresh_tokens SET revoked = 1 WHERE token = ? AND user_id = ?",
            params![token, user_id],
        )?;
        Ok(())
    }

    /// Get active sessions.
    pub fn get_active_sessions(&self, user_id: &str) -> Result<Vec<ActiveSession>> {
        let conn = self.get_conn()?;
        let mut stmt = conn.prepare(
            "SELECT token, user_id, expires_at, revoked FROM refresh_tokens WHERE revoked = 0 AND expires_at > ? AND user_id = ?"
        )?;
        let now = Utc::now().timestamp();
        let mut rows = stmt.query(params![now, user_id])?;
        let mut sessions = Vec::new();
        while let Some(row) = rows.next()? {
            sessions.push(ActiveSession {
                token: row.get(0)?,
                user_id: row.get(1)?,
                expires_at: row.get(2)?,
                revoked: row.get::<_, i32>(3)? == 1,
            });
        }
        Ok(sessions)
    }
}

/// Active session details representing an active refresh token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveSession {
    pub token: String,
    pub user_id: String,
    pub expires_at: i64,
    pub revoked: bool,
}
