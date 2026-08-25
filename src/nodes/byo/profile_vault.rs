//! Profile Vault Encrypted Storage API (Ola 1 BYO persistence nodes)
//!
//! Handles single-tenant encrypted user profiles (email, name, preferences) in Postgres/Neon
//! using AES-256-GCM authenticated encryption with DEKs derived via HKDF-SHA256
//! (`swal-profile-vault-v1`).

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Postgres, Row};

use crate::crypto::encryption::{aes_decrypt, aes_encrypt, NonceBytes};
use crate::security::encryption_keys::MasterKeyManager;

/// Info tag used for deriving the Profile Vault DEK from MasterKeyManager.
pub const HKDF_PROFILE_VAULT_INFO: &[u8] = b"swal-profile-vault-v1";

/// User Profile model stored in the profile vault.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserProfile {
    pub tenant_id: String,
    pub email: String,
    pub name: String,
    #[serde(default)]
    pub preferences: serde_json::Value,
}

/// Persistent encrypted storage engine for user profiles.
#[derive(Clone)]
pub struct ProfileVault {
    pool: Pool<Postgres>,
    dek: [u8; 32],
}

impl ProfileVault {
    /// Create a new `ProfileVault` with a Postgres connection pool and DEK derived from `MasterKeyManager`.
    pub fn new(pool: Pool<Postgres>, master_key_mgr: &MasterKeyManager) -> Result<Self> {
        let mut dek = [0u8; 32];
        master_key_mgr
            .derive_key(HKDF_PROFILE_VAULT_INFO, &mut dek)
            .context("Failed to derive DEK for ProfileVault")?;
        Ok(Self { pool, dek })
    }

    /// Create a `ProfileVault` directly with an explicit 32-byte DEK key.
    pub fn from_key(pool: Pool<Postgres>, dek: [u8; 32]) -> Self {
        Self { pool, dek }
    }

    /// Ensure the `profile_vault_enc` table exists in the database.
    pub async fn init_table(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS profile_vault_enc (
                tenant_id TEXT PRIMARY KEY,
                ciphertext BYTEA NOT NULL,
                updated_at TIMESTAMPTZ DEFAULT NOW()
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to initialize profile_vault_enc table")?;

        Ok(())
    }

    /// Save or update a user profile (upsert semantics, single profile per tenant_id).
    pub async fn save_profile(&self, profile: &UserProfile) -> Result<()> {
        let json_bytes = serde_json::to_vec(profile)
            .context("Failed to serialize UserProfile to JSON")?;

        let nonce = NonceBytes::generate();
        let encrypted_blob = aes_encrypt(&json_bytes, &self.dek, &nonce)
            .map_err(|e| anyhow!("Failed to encrypt UserProfile: {}", e))?;

        sqlx::query(
            r#"
            INSERT INTO profile_vault_enc (tenant_id, ciphertext, updated_at)
            VALUES ($1, $2, NOW())
            ON CONFLICT (tenant_id)
            DO UPDATE SET ciphertext = EXCLUDED.ciphertext, updated_at = NOW()
            "#,
        )
        .bind(&profile.tenant_id)
        .bind(&encrypted_blob)
        .execute(&self.pool)
        .await
        .context("Failed to execute upsert into profile_vault_enc")?;

        Ok(())
    }

    /// Retrieve and decrypt a user profile by tenant_id.
    pub async fn get_profile(&self, tenant_id: &str) -> Result<Option<UserProfile>> {
        let row = sqlx::query("SELECT ciphertext FROM profile_vault_enc WHERE tenant_id = $1")
            .bind(tenant_id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to query profile_vault_enc table")?;

        let row = match row {
            Some(r) => r,
            None => return Ok(None),
        };

        let encrypted_blob: Vec<u8> = row.try_get("ciphertext")?;
        let decrypted_bytes = aes_decrypt(&encrypted_blob, &self.dek)
            .map_err(|e| anyhow!("Failed to decrypt UserProfile ciphertext: {}", e))?;

        let profile: UserProfile = serde_json::from_slice(&decrypted_bytes)
            .context("Failed to deserialize UserProfile from decrypted JSON")?;

        Ok(Some(profile))
    }

    /// Fetch raw encrypted ciphertext directly from DB without decrypting.
    pub async fn get_raw_ciphertext(&self, tenant_id: &str) -> Result<Option<Vec<u8>>> {
        let row = sqlx::query("SELECT ciphertext FROM profile_vault_enc WHERE tenant_id = $1")
            .bind(tenant_id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch raw ciphertext from profile_vault_enc")?;

        match row {
            Some(r) => {
                let ciphertext: Vec<u8> = r.try_get("ciphertext")?;
                Ok(Some(ciphertext))
            }
            None => Ok(None),
        }
    }

    /// Delete a user profile by tenant_id.
    pub async fn delete_profile(&self, tenant_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM profile_vault_enc WHERE tenant_id = $1")
            .bind(tenant_id)
            .execute(&self.pool)
            .await
            .context("Failed to delete record from profile_vault_enc")?;

        Ok(())
    }
}
