//! Persistent Auth Storage for Xavier
//! Manages users, sessions, and audit logs in a dedicated auth.db

use std::path::Path;
use anyhow::{Result, Context, anyhow};
use std::sync::Mutex;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use rand::{RngCore, thread_rng};

use crate::security::auth::{UserRole, User};

/// Persistent storage for authentication data
pub struct AuthStore {
    conn: Mutex<Connection>,
    encryption_key: [u8; 32],
}

impl AuthStore {
    pub fn get_user_by_id(&self, user_id: &str) -> Result<Option<User>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, email, name, role, created_at, updated_at FROM users WHERE id = ?"
        )?;

        let mut rows = stmt.query(params![user_id])?;
        if let Some(row) = rows.next()? {
            let role_str: String = row.get(3)?;
            Ok(Some(User {
                id: row.get(0)?,
                email: row.get(1)?,
                name: row.get(2)?,
                role: serde_json::from_str(&role_str)?,
                api_key: "".to_string(),
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn update_password(&self, user_id: &str, new_hash: &str) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "UPDATE users SET password_hash = ?, updated_at = ? WHERE id = ?",
            params![new_hash, Utc::now().timestamp(), user_id]
        )?;
        Ok(())
    }

    pub fn open<P: AsRef<Path>>(path: P, encryption_key: [u8; 32]) -> Result<Self> {
        let conn = Connection::open(path)?;
        let mut store = Self { conn: Mutex::new(conn), encryption_key };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&mut self) -> Result<()> {
        self.conn.lock().unwrap().execute_batch(
            "CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                email TEXT UNIQUE NOT NULL,
                name TEXT NOT NULL,
                role TEXT NOT NULL,
                password_hash TEXT NOT NULL,
                totp_secret BLOB,
                recovery_phrase BLOB,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS refresh_tokens (
                token TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                expires_at INTEGER NOT NULL,
                revoked INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY(user_id) REFERENCES users(id)
            );
            CREATE TABLE IF NOT EXISTS audit_logs (
                id TEXT PRIMARY KEY,
                timestamp INTEGER NOT NULL,
                user_id TEXT,
                event_type TEXT NOT NULL,
                ip_address TEXT,
                user_agent TEXT,
                metadata TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_logs(timestamp);
            CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);"
        )?;
        Ok(())
    }

    pub fn create_user(&self, user: &User, password_hash: &str) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "INSERT INTO users (id, email, name, role, password_hash, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                user.id,
                user.email,
                user.name,
                serde_json::to_string(&user.role)?,
                password_hash,
                user.created_at,
                user.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_user_by_email(&self, email: &str) -> Result<Option<(User, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, email, name, role, password_hash, created_at, updated_at FROM users WHERE email = ?"
        )?;

        let mut rows = stmt.query(params![email])?;

        if let Some(row) = rows.next()? {
            let role_str: String = row.get(3)?;
            let user = User {
                id: row.get(0)?,
                email: row.get(1)?,
                name: row.get(2)?,
                role: serde_json::from_str(&role_str)?,
                api_key: "".to_string(), // Not used in new auth
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            };
            let password_hash: String = row.get(4)?;
            Ok(Some((user, password_hash)))
        } else {
            Ok(None)
        }
    }

    pub fn set_totp_secret(&self, user_id: &str, secret: &str) -> Result<()> {
        let encrypted = self.encrypt(secret.as_bytes())?;
        self.conn.lock().unwrap().execute(
            "UPDATE users SET totp_secret = ? WHERE id = ?",
            params![encrypted, user_id],
        )?;
        Ok(())
    }

    pub fn get_totp_secret(&self, user_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT totp_secret FROM users WHERE id = ?")?;
        let secret_bytes: Option<Vec<u8>> = stmt.query_row(params![user_id], |r| r.get(0))?;

        match secret_bytes {
            Some(bytes) => {
                let decrypted = self.decrypt(&bytes)?;
                Ok(Some(String::from_utf8(decrypted)?))
            }
            None => Ok(None)
        }
    }

    pub fn set_recovery_phrase(&self, user_id: &str, phrase: &str) -> Result<()> {
        let encrypted = self.encrypt(phrase.as_bytes())?;
        self.conn.lock().unwrap().execute(
            "UPDATE users SET recovery_phrase = ? WHERE id = ?",
            params![encrypted, user_id],
        )?;
        Ok(())
    }

    pub fn get_recovery_phrase(&self, user_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT recovery_phrase FROM users WHERE id = ?")?;
        let phrase_bytes: Option<Vec<u8>> = stmt.query_row(params![user_id], |r| r.get(0))?;

        match phrase_bytes {
            Some(bytes) => {
                let decrypted = self.decrypt(&bytes)?;
                Ok(Some(String::from_utf8(decrypted)?))
            }
            None => Ok(None)
        }
    }

    pub fn save_refresh_token(&self, token: &str, user_id: &str, expires_at: i64) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "INSERT INTO refresh_tokens (token, user_id, expires_at) VALUES (?, ?, ?)",
            params![token, user_id, expires_at],
        )?;
        Ok(())
    }

    pub fn verify_refresh_token(&self, token: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT user_id FROM refresh_tokens WHERE token = ? AND revoked = 0 AND expires_at > ?"
        )?;
        let now = Utc::now().timestamp();
        let mut rows = stmt.query(params![token, now])?;

        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn revoke_refresh_token(&self, token: &str) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "UPDATE refresh_tokens SET revoked = 1 WHERE token = ?",
            params![token],
        )?;
        Ok(())
    }

    pub fn get_active_sessions(&self) -> Result<Vec<ActiveSession>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT token, user_id, expires_at, revoked FROM refresh_tokens WHERE revoked = 0 AND expires_at > ?"
        )?;
        let now = Utc::now().timestamp();
        let mut rows = stmt.query(params![now])?;
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

    pub fn count_failed_logins(&self, ip_address: &str, since_timestamp: i64) -> Result<i32> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT COUNT(1) FROM audit_logs WHERE ip_address = ? AND event_type = 'login_failed' AND timestamp > ?"
        )?;
        let count: i32 = stmt.query_row(params![ip_address, since_timestamp], |r| r.get(0))?;
        Ok(count)
    }

    pub fn log_event(&self, user_id: Option<&str>, event_type: &str, ip: Option<&str>, ua: Option<&str>, metadata: Option<&str>) -> Result<()> {
        let id = ulid::Ulid::new().to_string();
        let timestamp = Utc::now().timestamp();
        self.conn.lock().unwrap().execute(
            "INSERT INTO audit_logs (id, timestamp, user_id, event_type, ip_address, user_agent, metadata)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![id, timestamp, user_id, event_type, ip, ua, metadata],
        )?;
        Ok(())
    }

    fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        let cipher = Aes256Gcm::new_from_slice(&self.encryption_key)
            .map_err(|_| anyhow!("invalid encryption key"))?;

        let mut nonce = [0u8; 12];
        thread_rng().fill_bytes(&mut nonce);
        let nonce = Nonce::from_slice(&nonce);

        let ciphertext = cipher.encrypt(nonce, data)
            .map_err(|e| anyhow!("encryption failed: {}", e))?;

        let mut result = nonce.to_vec();
        result.extend_from_slice(&ciphertext);
        Ok(result)
    }

    fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        if data.len() < 12 {
            return Err(anyhow!("invalid encrypted data"));
        }

        let cipher = Aes256Gcm::new_from_slice(&self.encryption_key)
            .map_err(|_| anyhow!("invalid encryption key"))?;

        let (nonce_bytes, ciphertext) = data.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        let plaintext = cipher.decrypt(nonce, ciphertext)
            .map_err(|e| anyhow!("decryption failed: {}", e))?;

        Ok(plaintext)
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
