//! API Token Management for Xavier
//!
//! Provides secure storage and validation of API tokens using bcrypt hashing
//! and SQLite for persistence.

use crate::codebase::connection_manager::ConnectionManager;
use anyhow::Result;
use bcrypt::{hash, verify, DEFAULT_COST};
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Metadata for an API token, excluding the hash.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApiTokenMetadata {
    pub id: String,
    pub name: String,
    pub partial_hash: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

/// Store for managing API tokens in a persistent SQLite database.
pub struct TokenStore {
    project_id: String,
}

impl Default for TokenStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenStore {
    pub fn new() -> Self {
        Self {
            project_id: "security".to_string(),
        }
    }

    /// Initializes the database schema for API tokens.
    pub async fn init_schema_async(&self) -> Result<()> {
        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS api_tokens (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    hash TEXT NOT NULL,
                    partial_hash TEXT NOT NULL,
                    scopes TEXT NOT NULL,
                    expires_at TEXT,
                    created_at TEXT NOT NULL,
                    last_used_at TEXT
                );
                CREATE INDEX IF NOT EXISTS idx_api_tokens_partial_hash ON api_tokens(partial_hash);
                "#,
            )?;
            Ok(())
        }).await
    }

    /// Creates a new API token.
    /// Returns the plaintext token (only once) and its metadata.
    pub async fn create_token(
        &self,
        name: String,
        scopes: Vec<String>,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<(String, ApiTokenMetadata)> {
        let plaintext = format!("xav_{}", Uuid::new_v4().to_string().replace("-", ""));
        let hashed = hash(&plaintext, DEFAULT_COST)?;

        let mut hasher = Sha256::new();
        hasher.update(plaintext.as_bytes());
        let full_hash = hex::encode(hasher.finalize());
        let partial_hash = format!("{}...", &full_hash[0..12]);

        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let scopes_json = serde_json::to_string(&scopes)?;
        let expires_at_str = expires_at.map(|dt| dt.to_rfc3339());

        let metadata = ApiTokenMetadata {
            id: id.clone(),
            name,
            partial_hash: partial_hash.clone(),
            scopes,
            expires_at,
            created_at: now,
            last_used_at: None,
        };

        ConnectionManager::global().with_conn(&self.project_id, {
            let id = id.clone();
            let name = metadata.name.clone();
            let partial_hash = partial_hash.clone();
            let now_str = now.to_rfc3339();
            move |conn| {
                conn.execute(
                    "INSERT INTO api_tokens (id, name, hash, partial_hash, scopes, expires_at, created_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?)",
                    params![id, name, hashed, partial_hash, scopes_json, expires_at_str, now_str],
                )?;
                Ok(())
            }
        }).await?;

        Ok((plaintext, metadata))
    }

    /// Lists all tokens (metadata only).
    pub async fn list_tokens(&self) -> Result<Vec<ApiTokenMetadata>> {
        ConnectionManager::global().with_conn(&self.project_id, |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, partial_hash, scopes, expires_at, created_at, last_used_at FROM api_tokens"
            )?;
            let rows = stmt.query_map([], |row| {
                let expires_at_str: Option<String> = row.get(4)?;
                let created_at_str: String = row.get(5)?;
                let last_used_at_str: Option<String> = row.get(6)?;
                let scopes_json: String = row.get(3)?;

                Ok(ApiTokenMetadata {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    partial_hash: row.get(2)?,
                    scopes: serde_json::from_str(&scopes_json).unwrap_or_default(),
                    expires_at: expires_at_str.and_then(|s| s.parse::<DateTime<Utc>>().ok()),
                    created_at: created_at_str.parse::<DateTime<Utc>>().unwrap_or_else(|_| Utc::now()),
                    last_used_at: last_used_at_str.and_then(|s| s.parse::<DateTime<Utc>>().ok()),
                })
            })?;

            let mut tokens = Vec::new();
            for row in rows {
                tokens.push(row?);
            }
            Ok(tokens)
        }).await
    }

    /// Revokes (deletes) a token.
    pub async fn revoke_token(&self, id: &str) -> Result<()> {
        let id = id.to_string();
        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute("DELETE FROM api_tokens WHERE id = ?", params![id])?;
            Ok(())
        }).await
    }

    /// Validates a plaintext token.
    /// If valid, returns its metadata and updates last_used_at.
    pub async fn validate_token(&self, plaintext: &str) -> Result<Option<ApiTokenMetadata>> {
        if !plaintext.starts_with("xav_") {
            return Ok(None);
        }

        let mut hasher = Sha256::new();
        hasher.update(plaintext.as_bytes());
        let full_hash = hex::encode(hasher.finalize());
        let partial_hash = format!("{}...", &full_hash[0..12]);

        let matches = self.get_by_partial_hash(&partial_hash).await?;
        for (meta, hashed) in matches {
            if verify(plaintext, &hashed).unwrap_or(false) {
                // Check expiry
                if let Some(expiry) = meta.expires_at {
                    if Utc::now() > expiry {
                        return Ok(None);
                    }
                }

                // Update last used
                let _ = self.update_last_used(&meta.id).await;

                return Ok(Some(meta));
            }
        }

        Ok(None)
    }

    async fn get_by_partial_hash(&self, partial: &str) -> Result<Vec<(ApiTokenMetadata, String)>> {
        let partial = partial.to_string();
        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, partial_hash, scopes, expires_at, created_at, last_used_at, hash FROM api_tokens WHERE partial_hash = ?"
            )?;
            let rows = stmt.query_map(params![partial], |row| {
                let expires_at_str: Option<String> = row.get(4)?;
                let created_at_str: String = row.get(5)?;
                let last_used_at_str: Option<String> = row.get(6)?;
                let scopes_json: String = row.get(3)?;

                let meta = ApiTokenMetadata {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    partial_hash: row.get(2)?,
                    scopes: serde_json::from_str(&scopes_json).unwrap_or_default(),
                    expires_at: expires_at_str.and_then(|s| s.parse::<DateTime<Utc>>().ok()),
                    created_at: created_at_str.parse::<DateTime<Utc>>().unwrap_or_else(|_| Utc::now()),
                    last_used_at: last_used_at_str.and_then(|s| s.parse::<DateTime<Utc>>().ok()),
                };
                let hashed: String = row.get(7)?;
                Ok((meta, hashed))
            })?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row?);
            }
            Ok(results)
        }).await
    }

    async fn update_last_used(&self, id: &str) -> Result<()> {
        let id = id.to_string();
        let now = Utc::now().to_rfc3339();
        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute("UPDATE api_tokens SET last_used_at = ? WHERE id = ?", params![now, id])?;
            Ok(())
        }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_token_lifecycle() {
        ConnectionManager::global().connect("security", ".").unwrap();
        let store = TokenStore::new();
        store.init_schema_async().await.unwrap();

        // 1. Create
        let (plaintext, meta) = store
            .create_token("test-token".to_string(), vec!["read".to_string()], None)
            .await
            .unwrap();

        assert!(plaintext.starts_with("xav_"));
        assert_eq!(meta.name, "test-token");
        assert_eq!(meta.scopes, vec!["read"]);

        // 2. Validate
        let validated = store.validate_token(&plaintext).await.unwrap();
        assert!(validated.is_some());
        let validated = validated.unwrap();
        assert_eq!(validated.id, meta.id);

        // 3. List
        let tokens = store.list_tokens().await.unwrap();
        assert!(tokens.iter().any(|t| t.id == meta.id));

        // 4. Revoke
        store.revoke_token(&meta.id).await.unwrap();
        let validated_after = store.validate_token(&plaintext).await.unwrap();
        assert!(validated_after.is_none());
    }
}
