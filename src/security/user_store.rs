use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::codebase::connection_manager::ConnectionManager;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct User {
    pub id: String,
    pub email: String,
    pub password_hash: String,
    pub recovery_seed_hash: String,
    pub two_factor_enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BackupCode {
    pub id: String,
    pub user_id: String,
    pub code_hash: String,
    pub used: bool,
}

pub struct UserStore {
    project_id: String,
}

impl Default for UserStore {
    fn default() -> Self {
        Self::new()
    }
}

impl UserStore {
    /// New.
    pub fn new() -> Self {
        Self {
            project_id: "default".to_string(),
        }
    }

    /// With project id.
    pub fn with_project_id(project_id: impl Into<String>) -> Self {
        Self {
            project_id: project_id.into(),
        }
    }

    /// Add user.
    pub async fn add_user(&self, user: User) -> Result<()> {
        let created_at = user.created_at.to_rfc3339();
        let updated_at = user.updated_at.to_rfc3339();
        let two_factor_enabled = if user.two_factor_enabled { 1 } else { 0 };

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute(
                "INSERT INTO users (id, email, password_hash, recovery_seed_hash, two_factor_enabled, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
                params![
                    user.id,
                    user.email,
                    user.password_hash,
                    user.recovery_seed_hash,
                    two_factor_enabled,
                    created_at,
                    updated_at,
                ],
            )?;
            Ok(())
        }).await
    }

    /// Get user by email.
    pub async fn get_user_by_email(&self, email: &str) -> Result<Option<User>> {
        let email = email.to_string();
        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, email, password_hash, recovery_seed_hash, two_factor_enabled, created_at, updated_at FROM users WHERE email = ?"
            )?;
            let mut rows = stmt.query(params![email])?;
            if let Some(row) = rows.next()? {
                let created_at_str: String = row.get(5)?;
                let updated_at_str: String = row.get(6)?;
                Ok(Some(User {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    password_hash: row.get(2)?,
                    recovery_seed_hash: row.get(3)?,
                    two_factor_enabled: row.get::<_, i32>(4)? != 0,
                    created_at: created_at_str.parse().unwrap_or_else(|_| Utc::now()),
                    updated_at: updated_at_str.parse().unwrap_or_else(|_| Utc::now()),
                }))
            } else {
                Ok(None)
            }
        }).await
    }

    /// Update password and recovery.
    pub async fn update_password_and_recovery(
        &self,
        user_id: &str,
        password_hash: &str,
        recovery_seed_hash: &str,
    ) -> Result<()> {
        let user_id = user_id.to_string();
        let password_hash = password_hash.to_string();
        let recovery_seed_hash = recovery_seed_hash.to_string();
        let now = Utc::now().to_rfc3339();

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute(
                "UPDATE users SET password_hash = ?, recovery_seed_hash = ?, two_factor_enabled = 0, updated_at = ? WHERE id = ?",
                params![password_hash, recovery_seed_hash, now, user_id],
            )?;
            Ok(())
        }).await
    }

    /// Save backup codes.
    pub async fn save_backup_codes(&self, codes: Vec<BackupCode>) -> Result<()> {
        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                let tx = conn.unchecked_transaction()?;
                {
                    let mut stmt = tx.prepare(
                    "INSERT INTO backup_codes (id, user_id, code_hash, used) VALUES (?, ?, ?, ?)"
                )?;
                    for code in codes {
                        stmt.execute(params![
                            code.id,
                            code.user_id,
                            code.code_hash,
                            if code.used { 1 } else { 0 }
                        ])?;
                    }
                }
                tx.commit()?;
                Ok(())
            })
            .await
    }

    /// Delete backup codes for user.
    pub async fn delete_backup_codes_for_user(&self, user_id: &str) -> Result<()> {
        let user_id = user_id.to_string();
        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                conn.execute(
                    "DELETE FROM backup_codes WHERE user_id = ?",
                    params![user_id],
                )?;
                Ok(())
            })
            .await
    }

    /// Count remaining backup codes.
    pub async fn count_remaining_backup_codes(&self, user_id: &str) -> Result<usize> {
        let user_id = user_id.to_string();
        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                let count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM backup_codes WHERE user_id = ? AND used = 0",
                    params![user_id],
                    |row| row.get(0),
                )?;
                Ok(count as usize)
            })
            .await
    }

    /// Verify and consume backup code.
    pub async fn verify_and_consume_backup_code(
        &self,
        user_id: &str,
        code_hash: &str,
    ) -> Result<bool> {
        let user_id = user_id.to_string();
        let code_hash = code_hash.to_string();
        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id FROM backup_codes WHERE user_id = ? AND code_hash = ? AND used = 0 LIMIT 1"
            )?;
            let mut rows = stmt.query(params![user_id, code_hash])?;
            if let Some(row) = rows.next()? {
                let id: String = row.get(0)?;
                conn.execute("UPDATE backup_codes SET used = 1 WHERE id = ?", params![id])?;
                Ok(true)
            } else {
                Ok(false)
            }
        }).await
    }
}
