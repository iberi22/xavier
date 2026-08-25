//! AppData Encrypted Persistence Storage (`src/nodes/byo/app_data.rs`)
//!
//! Provides encrypted application data persistence over PostgreSQL (`app_data_enc` table),
//! using AES-256-GCM record cipher encryption/decryption with multi-tenant isolation.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, PgPool};
use std::sync::Arc;

use crate::crypto::encryption::{encrypt_data, decrypt_data, EncryptedBlob, NonceBytes};
use crate::crypto::hex_decode;

/// Record cipher trait for app data encryption and decryption.
pub trait RecordCipher: Send + Sync {
    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>>;
    fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>>;
}

/// AES-256-GCM record cipher implementation.
#[derive(Clone)]
pub struct AesGcmRecordCipher {
    key: [u8; 32],
}

impl AesGcmRecordCipher {
    pub fn new(key: [u8; 32]) -> Self {
        Self { key }
    }
}

impl RecordCipher for AesGcmRecordCipher {
    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let nonce = NonceBytes::generate();
        let blob = encrypt_data(plaintext, &self.key, &nonce)
            .map_err(|e| anyhow::anyhow!("Encryption error: {:?}", e))?;
        Ok(blob.to_bytes())
    }

    fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        let blob = EncryptedBlob::from_bytes(ciphertext)
            .map_err(|e| anyhow::anyhow!("Failed to parse encrypted blob: {:?}", e))?;
        let nonce_arr: [u8; 12] = blob
            .nonce
            .clone()
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid nonce length in blob"))?;
        let plaintext = decrypt_data(&blob.ciphertext, &self.key, &nonce_arr)
            .map_err(|e| anyhow::anyhow!("Decryption error: {:?}", e))?;
        Ok(plaintext)
    }
}

/// App data record structure stored in `app_data_enc`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppDataRecord {
    pub tenant_id: String,
    pub app_id: String,
    pub kind: String,
    pub id: String,
    pub payload: Vec<u8>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Decodes bytea or text string from PostgreSQL column.
pub fn decode_bytea_row(row: &sqlx::postgres::PgRow, col: &str) -> Result<Vec<u8>> {
    if let Ok(bytes) = row.try_get::<Vec<u8>, _>(col) {
        return Ok(bytes);
    }
    if let Ok(text) = row.try_get::<String, _>(col) {
        return Ok(decode_bytea_str(&text));
    }
    anyhow::bail!("Failed to extract bytea/text field '{}'", col)
}

/// Helper to parse text representation of bytea (hex formatted with optional \\x or 0x prefix).
pub fn decode_bytea_str(s: &str) -> Vec<u8> {
    let trimmed = s.trim();
    if let Some(hex_str) = trimmed.strip_prefix("\\x").or_else(|| trimmed.strip_prefix("0x")) {
        if let Ok(decoded) = hex_decode(hex_str) {
            return decoded;
        }
    }
    if let Ok(decoded) = hex_decode(trimmed) {
        return decoded;
    }
    trimmed.as_bytes().to_vec()
}

/// Manager for encrypted app data CRUD operations over Postgres pool.
pub struct AppDataManager {
    pool: PgPool,
    cipher: Arc<dyn RecordCipher>,
}

impl AppDataManager {
    pub fn new(pool: PgPool, cipher: Arc<dyn RecordCipher>) -> Self {
        Self { pool, cipher }
    }

    pub fn with_aes_key(pool: PgPool, key: [u8; 32]) -> Self {
        Self::new(pool, Arc::new(AesGcmRecordCipher::new(key)))
    }

    /// Initializes table schema if not already existing.
    pub async fn init_schema(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS app_data_enc (
                tenant_id TEXT NOT NULL,
                app_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                id TEXT NOT NULL,
                payload BYTEA NOT NULL,
                metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY (tenant_id, app_id, kind, id)
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to initialize app_data_enc schema")?;

        Ok(())
    }

    /// Store (upsert) encrypted record into `app_data_enc`.
    pub async fn put(&self, record: &AppDataRecord) -> Result<()> {
        let encrypted_payload = self.cipher.encrypt(&record.payload)?;

        sqlx::query(
            r#"
            INSERT INTO app_data_enc (tenant_id, app_id, kind, id, payload, metadata, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (tenant_id, app_id, kind, id) DO UPDATE SET
                payload = EXCLUDED.payload,
                metadata = EXCLUDED.metadata,
                updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(&record.tenant_id)
        .bind(&record.app_id)
        .bind(&record.kind)
        .bind(&record.id)
        .bind(&encrypted_payload)
        .bind(&record.metadata)
        .bind(record.created_at)
        .bind(record.updated_at)
        .execute(&self.pool)
        .await
        .context("Failed to put app_data record")?;

        Ok(())
    }

    /// Retrieve and decrypt single record by PK (tenant_id, app_id, kind, id).
    pub async fn get(
        &self,
        tenant_id: &str,
        app_id: &str,
        kind: &str,
        id: &str,
    ) -> Result<Option<AppDataRecord>> {
        let row = sqlx::query(
            r#"
            SELECT tenant_id, app_id, kind, id, payload, metadata, created_at, updated_at
            FROM app_data_enc
            WHERE tenant_id = $1 AND app_id = $2 AND kind = $3 AND id = $4
            "#,
        )
        .bind(tenant_id)
        .bind(app_id)
        .bind(kind)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch app_data record")?;

        match row {
            Some(row) => {
                let raw_payload = decode_bytea_row(&row, "payload")?;
                let decrypted_payload = self.cipher.decrypt(&raw_payload)?;

                Ok(Some(AppDataRecord {
                    tenant_id: row.try_get("tenant_id")?,
                    app_id: row.try_get("app_id")?,
                    kind: row.try_get("kind")?,
                    id: row.try_get("id")?,
                    payload: decrypted_payload,
                    metadata: row.try_get("metadata")?,
                    created_at: row.try_get("created_at")?,
                    updated_at: row.try_get("updated_at")?,
                }))
            }
            None => Ok(None),
        }
    }

    /// List and decrypt all records matching (tenant_id, app_id, kind).
    pub async fn list_by_kind(
        &self,
        tenant_id: &str,
        app_id: &str,
        kind: &str,
    ) -> Result<Vec<AppDataRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT tenant_id, app_id, kind, id, payload, metadata, created_at, updated_at
            FROM app_data_enc
            WHERE tenant_id = $1 AND app_id = $2 AND kind = $3
            ORDER BY id ASC
            "#,
        )
        .bind(tenant_id)
        .bind(app_id)
        .bind(kind)
        .fetch_all(&self.pool)
        .await
        .context("Failed to list app_data records by kind")?;

        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            let raw_payload = decode_bytea_row(&row, "payload")?;
            let decrypted_payload = self.cipher.decrypt(&raw_payload)?;

            results.push(AppDataRecord {
                tenant_id: row.try_get("tenant_id")?,
                app_id: row.try_get("app_id")?,
                kind: row.try_get("kind")?,
                id: row.try_get("id")?,
                payload: decrypted_payload,
                metadata: row.try_get("metadata")?,
                created_at: row.try_get("created_at")?,
                updated_at: row.try_get("updated_at")?,
            });
        }

        Ok(results)
    }

    /// Delete record by PK (tenant_id, app_id, kind, id). Returns true if deleted.
    pub async fn delete(
        &self,
        tenant_id: &str,
        app_id: &str,
        kind: &str,
        id: &str,
    ) -> Result<bool> {
        let result = sqlx::query(
            r#"
            DELETE FROM app_data_enc
            WHERE tenant_id = $1 AND app_id = $2 AND kind = $3 AND id = $4
            "#,
        )
        .bind(tenant_id)
        .bind(app_id)
        .bind(kind)
        .bind(id)
        .execute(&self.pool)
        .await
        .context("Failed to delete app_data record")?;

        Ok(result.rows_affected() > 0)
    }
}
