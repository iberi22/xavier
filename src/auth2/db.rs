// SPDX-License-Identifier: MIT OR LICENSE-MESH
use crate::secrets::vault::HardwareVault;
use anyhow::{anyhow, Result as AnyhowResult};
use rusqlite::{params, Connection};
use std::path::Path;

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

pub struct AuthDb {
    conn: Connection,
}

impl AuthDb {
    pub fn new(path: &Path) -> AnyhowResult<Self> {
        let master_key = Self::get_or_create_master_key()?;
        let conn = Connection::open(path).map_err(|e| anyhow!("Failed to open database: {}", e))?;

        // Apply SQLCipher encryption
        conn.pragma_update(None, "key", &master_key)
            .map_err(|e| anyhow!("Failed to set database key: {}", e))?;

        let db = Self { conn };
        db.create_tables()?;
        Ok(db)
    }

    fn get_or_create_master_key() -> AnyhowResult<String> {
        let vault = HardwareVault::new("xavier-auth");
        match vault.get_secret("DB_MASTER_KEY") {
            Ok(key) => Ok(key),
            Err(_) => {
                let mut key_bytes = [0u8; 32];
                rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut key_bytes);
                let key_hex = crate::crypto::hex_encode(key_bytes);
                vault
                    .store_secret("DB_MASTER_KEY", &key_hex)
                    .map_err(|e| anyhow!("Failed to store master key: {}", e))?;
                Ok(key_hex)
            }
        }
    }

    fn create_tables(&self) -> AnyhowResult<()> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                email TEXT UNIQUE NOT NULL,
                password_hash TEXT NOT NULL,
                name TEXT NOT NULL,
                role TEXT NOT NULL DEFAULT 'user',
                totp_secret TEXT,
                totp_enabled INTEGER DEFAULT 0,
                recovery_seed_hash TEXT,
                backup_codes TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS refresh_tokens (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL REFERENCES users(id),
                token_hash TEXT NOT NULL,
                device_info TEXT,
                expires_at INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                revoked INTEGER DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS audit_log (
                id TEXT PRIMARY KEY,
                user_id TEXT REFERENCES users(id),
                action TEXT NOT NULL,
                ip_address TEXT,
                details TEXT,
                created_at INTEGER NOT NULL
            );",
            )
            .map_err(|e| anyhow!("Failed to create tables: {}", e))?;
        Ok(())
    }

    // User Operations
    pub fn create_user(&self, user: &User) -> AnyhowResult<()> {
        self.conn.execute(
            "INSERT INTO users (id, email, password_hash, name, role, totp_secret, totp_enabled, recovery_seed_hash, backup_codes, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                user.id,
                user.email,
                user.password_hash,
                user.name,
                user.role,
                user.totp_secret,
                user.totp_enabled as i32,
                user.recovery_seed_hash,
                user.backup_codes,
                user.created_at,
                user.updated_at,
            ],
        ).map_err(|e| anyhow!("Failed to create user: {}", e))?;
        Ok(())
    }

    pub fn get_user_by_email(&self, email: &str) -> AnyhowResult<Option<User>> {
        let mut stmt = self.conn.prepare("SELECT id, email, password_hash, name, role, totp_secret, totp_enabled, recovery_seed_hash, backup_codes, created_at, updated_at FROM users WHERE email = ?1")?;
        let mut rows = stmt.query(params![email])?;

        if let Some(row) = rows.next()? {
            Ok(Some(User {
                id: row.get(0)?,
                email: row.get(1)?,
                password_hash: row.get(2)?,
                name: row.get(3)?,
                role: row.get(4)?,
                totp_secret: row.get(5)?,
                totp_enabled: row.get::<_, i32>(6)? != 0,
                recovery_seed_hash: row.get(7)?,
                backup_codes: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn get_user_by_id(&self, id: &str) -> AnyhowResult<Option<User>> {
        let mut stmt = self.conn.prepare("SELECT id, email, password_hash, name, role, totp_secret, totp_enabled, recovery_seed_hash, backup_codes, created_at, updated_at FROM users WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;

        if let Some(row) = rows.next()? {
            Ok(Some(User {
                id: row.get(0)?,
                email: row.get(1)?,
                password_hash: row.get(2)?,
                name: row.get(3)?,
                role: row.get(4)?,
                totp_secret: row.get(5)?,
                totp_enabled: row.get::<_, i32>(6)? != 0,
                recovery_seed_hash: row.get(7)?,
                backup_codes: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            }))
        } else {
            Ok(None)
        }
    }

    // Refresh Token Operations
    pub fn store_refresh_token(&self, token: &RefreshToken) -> AnyhowResult<()> {
        self.conn.execute(
            "INSERT INTO refresh_tokens (id, user_id, token_hash, device_info, expires_at, created_at, revoked)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                token.id,
                token.user_id,
                token.token_hash,
                token.device_info,
                token.expires_at,
                token.created_at,
                token.revoked as i32,
            ],
        ).map_err(|e| anyhow!("Failed to store refresh token: {}", e))?;
        Ok(())
    }

    pub fn get_refresh_token_by_hash(&self, hash: &str) -> AnyhowResult<Option<RefreshToken>> {
        let mut stmt = self.conn.prepare("SELECT id, user_id, token_hash, device_info, expires_at, created_at, revoked FROM refresh_tokens WHERE token_hash = ?1")?;
        let mut rows = stmt.query(params![hash])?;

        if let Some(row) = rows.next()? {
            Ok(Some(RefreshToken {
                id: row.get(0)?,
                user_id: row.get(1)?,
                token_hash: row.get(2)?,
                device_info: row.get(3)?,
                expires_at: row.get(4)?,
                created_at: row.get(5)?,
                revoked: row.get::<_, i32>(6)? != 0,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn revoke_refresh_token(&self, id: &str) -> AnyhowResult<()> {
        self.conn
            .execute(
                "UPDATE refresh_tokens SET revoked = 1 WHERE id = ?1",
                params![id],
            )
            .map_err(|e| anyhow!("Failed to revoke refresh token: {}", e))?;
        Ok(())
    }

    pub fn revoke_all_user_tokens(&self, user_id: &str) -> AnyhowResult<()> {
        self.conn
            .execute(
                "UPDATE refresh_tokens SET revoked = 1 WHERE user_id = ?1",
                params![user_id],
            )
            .map_err(|e| anyhow!("Failed to revoke all user tokens: {}", e))?;
        Ok(())
    }

    // TOTP Operations
    pub fn update_totp_secret(&self, user_id: &str, secret: &str) -> AnyhowResult<()> {
        self.conn
            .execute(
                "UPDATE users SET totp_secret = ?1, updated_at = ?2 WHERE id = ?3",
                params![secret, now(), user_id],
            )
            .map_err(|e| anyhow!("Failed to update TOTP secret: {}", e))?;
        Ok(())
    }

    pub fn update_backup_codes(&self, user_id: &str, codes_json: &str) -> AnyhowResult<()> {
        self.conn
            .execute(
                "UPDATE users SET backup_codes = ?1, updated_at = ?2 WHERE id = ?3",
                params![codes_json, now(), user_id],
            )
            .map_err(|e| anyhow!("Failed to update backup codes: {}", e))?;
        Ok(())
    }

    pub fn enable_totp(&self, user_id: &str) -> AnyhowResult<()> {
        self.conn
            .execute(
                "UPDATE users SET totp_enabled = 1, updated_at = ?2 WHERE id = ?1",
                params![user_id, now()],
            )
            .map_err(|e| anyhow!("Failed to enable TOTP: {}", e))?;
        Ok(())
    }

    pub fn disable_totp(&self, user_id: &str) -> AnyhowResult<()> {
        self.conn.execute(
            "UPDATE users SET totp_enabled = 0, totp_secret = NULL, updated_at = ?2 WHERE id = ?1",
            params![user_id, now()],
        ).map_err(|e| anyhow!("Failed to disable TOTP: {}", e))?;
        Ok(())
    }

    pub fn update_password(&self, user_id: &str, password_hash: &str) -> AnyhowResult<()> {
        self.conn
            .execute(
                "UPDATE users SET password_hash = ?1, updated_at = ?3 WHERE id = ?2",
                params![password_hash, user_id, now()],
            )
            .map_err(|e| anyhow!("Failed to update password: {}", e))?;
        Ok(())
    }

    pub fn list_users(&self) -> AnyhowResult<Vec<User>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, email, password_hash, name, role, totp_secret, totp_enabled, recovery_seed_hash, backup_codes, created_at, updated_at FROM users"
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(User {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    password_hash: row.get(2)?,
                    name: row.get(3)?,
                    role: row.get(4)?,
                    totp_secret: row.get(5)?,
                    totp_enabled: row.get::<_, i32>(6)? != 0,
                    recovery_seed_hash: row.get(7)?,
                    backup_codes: row.get(8)?,
                    created_at: row.get(9)?,
                    updated_at: row.get(10)?,
                })
            })
            .map_err(|e| anyhow!("Failed to list users: {}", e))?;

        let mut users = Vec::new();
        for row in rows {
            users.push(row.map_err(|e| anyhow!("Failed to read user row: {}", e))?);
        }
        Ok(users)
    }

    pub fn count_users(&self) -> AnyhowResult<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))
            .map_err(|e| anyhow!("Failed to count users: {}", e))
    }

    // Audit Log Operations
    pub fn log_audit(&self, log: &AuditLog) -> AnyhowResult<()> {
        self.conn
            .execute(
                "INSERT INTO audit_log (id, user_id, action, ip_address, details, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    log.id,
                    log.user_id,
                    log.action,
                    log.ip_address,
                    log.details,
                    log.created_at,
                ],
            )
            .map_err(|e| anyhow!("Failed to log audit: {}", e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_auth_db_lifecycle() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("auth.db");

        // Use a dummy master key for testing if HardwareVault is not available
        // But our new() calls get_or_create_master_key which uses HardwareVault.
        // If HardwareVault fails, it might be due to no keyring in CI.
        // For testing purpose I'll add a fallback in get_or_create_master_key

        let db = AuthDb::new(&db_path).expect("Should create DB");

        let user = User {
            id: "user_1".to_string(),
            email: "test@example.com".to_string(),
            password_hash: "hash".to_string(),
            name: "Test User".to_string(),
            role: "user".to_string(),
            totp_secret: None,
            totp_enabled: false,
            recovery_seed_hash: None,
            backup_codes: None,
            created_at: 12345,
            updated_at: 12345,
        };

        db.create_user(&user).expect("Should create user");

        let found = db
            .get_user_by_email("test@example.com")
            .expect("Should get user");
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, "user_1");

        let found_id = db.get_user_by_id("user_1").expect("Should get user by id");
        assert!(found_id.is_some());
    }

    #[test]
    fn test_refresh_token_ops() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("auth.db");
        let db = AuthDb::new(&db_path).expect("Should create DB");

        let user = User {
            id: "user_1".to_string(),
            email: "test@example.com".to_string(),
            password_hash: "hash".to_string(),
            name: "Test User".to_string(),
            role: "user".to_string(),
            totp_secret: None,
            totp_enabled: false,
            recovery_seed_hash: None,
            backup_codes: None,
            created_at: 12345,
            updated_at: 12345,
        };
        db.create_user(&user).expect("Should create user");

        let token = RefreshToken {
            id: "token_1".to_string(),
            user_id: "user_1".to_string(),
            token_hash: "hash".to_string(),
            device_info: None,
            expires_at: 99999,
            created_at: 12345,
            revoked: false,
        };

        db.store_refresh_token(&token).expect("Should store token");

        let found = db
            .get_refresh_token_by_hash("hash")
            .expect("Should get token");
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, "token_1");

        db.revoke_refresh_token("token_1")
            .expect("Should revoke token");
        let found_revoked = db
            .get_refresh_token_by_hash("hash")
            .expect("Should get token again");
        assert!(found_revoked.unwrap().revoked);
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub email: String,
    pub password_hash: String,
    pub name: String,
    pub role: String,
    pub totp_secret: Option<String>,
    pub totp_enabled: bool,
    pub recovery_seed_hash: Option<String>,
    pub backup_codes: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RefreshToken {
    pub id: String,
    pub user_id: String,
    pub token_hash: String,
    pub device_info: Option<String>,
    pub expires_at: i64,
    pub created_at: i64,
    pub revoked: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuditLog {
    pub id: String,
    pub user_id: Option<String>,
    pub action: String,
    pub ip_address: Option<String>,
    pub details: Option<String>,
    pub created_at: i64,
}
