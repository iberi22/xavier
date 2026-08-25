//! Tenant management for BYO nodes supporting Supabase (PostgREST) and Neon (sqlx Postgres).

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

/// Record representing a tenant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TenantRecord {
    pub tenant_id: String,
    pub tier: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Dual backend configuration for BYO TenantManager.
#[derive(Clone)]
pub enum TenantBackend {
    Supabase {
        client: Client,
        url: String,
        key: String,
    },
    Neon {
        pool: PgPool,
    },
}

/// TenantManager handles CRUD operations for tenants across Supabase (PostgREST) and Neon (sqlx).
#[derive(Clone)]
pub struct TenantManager {
    backend: TenantBackend,
}

impl TenantManager {
    /// Create a new TenantManager targeting Supabase via PostgREST API.
    pub fn new_supabase(url: &str, key: &str) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Self {
            backend: TenantBackend::Supabase {
                client,
                url: url.trim_end_matches('/').to_string(),
                key: key.to_string(),
            },
        })
    }

    /// Create a new TenantManager targeting Neon via sqlx PgPool.
    pub fn new_neon(pool: PgPool) -> Self {
        Self {
            backend: TenantBackend::Neon { pool },
        }
    }

    /// Create a new TenantManager targeting Neon from a Postgres connection URL.
    pub async fn new_neon_from_url(url: &str) -> Result<Self> {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(url)
            .await?;
        Ok(Self::new_neon(pool))
    }

    /// Generates a deterministic HMAC-SHA256 tenant_id from a secret key and identifier.
    pub fn generate_tenant_id(secret: &[u8], identifier: &[u8]) -> String {
        let hmac_bytes = crate::crypto::hmac::compute_hmac_sha256(secret, identifier);
        crate::crypto::base64_encode(hmac_bytes)
    }

    /// Returns a reference to the inner TenantBackend enum.
    pub fn backend(&self) -> &TenantBackend {
        &self.backend
    }

    /// Initialize tables in Neon Postgres if needed.
    pub async fn init_schema(&self) -> Result<()> {
        if let TenantBackend::Neon { pool } = &self.backend {
            sqlx::query(
                r#"
                CREATE TABLE IF NOT EXISTS tenants (
                    tenant_id TEXT PRIMARY KEY,
                    tier TEXT NOT NULL,
                    created_at TIMESTAMPTZ DEFAULT NOW(),
                    updated_at TIMESTAMPTZ DEFAULT NOW()
                );
                "#,
            )
            .execute(pool)
            .await?;

            sqlx::query(
                r#"
                CREATE TABLE IF NOT EXISTS app_data_enc (
                    id TEXT PRIMARY KEY,
                    tenant_id TEXT NOT NULL REFERENCES tenants(tenant_id) ON DELETE CASCADE,
                    payload TEXT NOT NULL
                );
                "#,
            )
            .execute(pool)
            .await?;
        }
        Ok(())
    }

    /// Create a new tenant record.
    pub async fn create(&self, tenant_id: &str, tier: &str) -> Result<TenantRecord> {
        match &self.backend {
            TenantBackend::Neon { pool } => {
                let row = sqlx::query(
                    r#"
                    INSERT INTO tenants (tenant_id, tier)
                    VALUES ($1, $2)
                    ON CONFLICT (tenant_id) DO UPDATE SET tier = EXCLUDED.tier, updated_at = NOW()
                    RETURNING tenant_id, tier, created_at::text, updated_at::text
                    "#,
                )
                .bind(tenant_id)
                .bind(tier)
                .fetch_one(pool)
                .await?;

                use sqlx::Row;
                Ok(TenantRecord {
                    tenant_id: row.try_get("tenant_id")?,
                    tier: row.try_get("tier")?,
                    created_at: row.try_get("created_at").ok(),
                    updated_at: row.try_get("updated_at").ok(),
                })
            }
            TenantBackend::Supabase { client, url, key } => {
                let endpoint = format!("{}/rest/v1/tenants", url);
                let payload = serde_json::json!({
                    "tenant_id": tenant_id,
                    "tier": tier
                });

                let resp = client
                    .post(&endpoint)
                    .header("apikey", key)
                    .header("Authorization", format!("Bearer {}", key))
                    .header("Content-Type", "application/json")
                    .header("Prefer", "return=representation,resolution=merge-duplicates")
                    .json(&payload)
                    .send()
                    .await?;

                if !resp.status().is_success() && resp.status() != reqwest::StatusCode::CREATED {
                    anyhow::bail!("Supabase create tenant failed: {}", resp.status());
                }

                let records: Vec<TenantRecord> = resp.json().await?;
                records.into_iter().next().ok_or_else(|| {
                    anyhow::anyhow!("Supabase create tenant returned no record representation")
                })
            }
        }
    }

    /// Fetch a tenant by tenant_id.
    pub async fn get(&self, tenant_id: &str) -> Result<Option<TenantRecord>> {
        match &self.backend {
            TenantBackend::Neon { pool } => {
                let row = sqlx::query(
                    r#"
                    SELECT tenant_id, tier, created_at::text, updated_at::text
                    FROM tenants
                    WHERE tenant_id = $1
                    "#,
                )
                .bind(tenant_id)
                .fetch_optional(pool)
                .await?;

                if let Some(row) = row {
                    use sqlx::Row;
                    Ok(Some(TenantRecord {
                        tenant_id: row.try_get("tenant_id")?,
                        tier: row.try_get("tier")?,
                        created_at: row.try_get("created_at").ok(),
                        updated_at: row.try_get("updated_at").ok(),
                    }))
                } else {
                    Ok(None)
                }
            }
            TenantBackend::Supabase { client, url, key } => {
                let endpoint = format!("{}/rest/v1/tenants?tenant_id=eq.{}", url, tenant_id);
                let resp = client
                    .get(&endpoint)
                    .header("apikey", key)
                    .header("Authorization", format!("Bearer {}", key))
                    .send()
                    .await?;

                if !resp.status().is_success() {
                    anyhow::bail!("Supabase get tenant failed: {}", resp.status());
                }

                let records: Vec<TenantRecord> = resp.json().await?;
                Ok(records.into_iter().next())
            }
        }
    }

    /// Update the tier of an existing tenant.
    pub async fn update_tier(&self, tenant_id: &str, tier: &str) -> Result<TenantRecord> {
        match &self.backend {
            TenantBackend::Neon { pool } => {
                let row = sqlx::query(
                    r#"
                    UPDATE tenants
                    SET tier = $2, updated_at = NOW()
                    WHERE tenant_id = $1
                    RETURNING tenant_id, tier, created_at::text, updated_at::text
                    "#,
                )
                .bind(tenant_id)
                .bind(tier)
                .fetch_optional(pool)
                .await?;

                if let Some(row) = row {
                    use sqlx::Row;
                    Ok(TenantRecord {
                        tenant_id: row.try_get("tenant_id")?,
                        tier: row.try_get("tier")?,
                        created_at: row.try_get("created_at").ok(),
                        updated_at: row.try_get("updated_at").ok(),
                    })
                } else {
                    anyhow::bail!("Tenant not found for tier update: {}", tenant_id)
                }
            }
            TenantBackend::Supabase { client, url, key } => {
                let endpoint = format!("{}/rest/v1/tenants?tenant_id=eq.{}", url, tenant_id);
                let payload = serde_json::json!({
                    "tier": tier
                });

                let resp = client
                    .patch(&endpoint)
                    .header("apikey", key)
                    .header("Authorization", format!("Bearer {}", key))
                    .header("Content-Type", "application/json")
                    .header("Prefer", "return=representation")
                    .json(&payload)
                    .send()
                    .await?;

                if !resp.status().is_success() {
                    anyhow::bail!("Supabase update_tier failed: {}", resp.status());
                }

                let records: Vec<TenantRecord> = resp.json().await?;
                records.into_iter().next().ok_or_else(|| {
                    anyhow::anyhow!("Tenant not found for tier update: {}", tenant_id)
                })
            }
        }
    }

    /// Delete a tenant and cascade delete associated app_data_enc records.
    pub async fn delete(&self, tenant_id: &str) -> Result<()> {
        match &self.backend {
            TenantBackend::Neon { pool } => {
                // Delete app_data_enc first to ensure cascade delete regardless of FK constraints
                sqlx::query("DELETE FROM app_data_enc WHERE tenant_id = $1")
                    .bind(tenant_id)
                    .execute(pool)
                    .await?;

                sqlx::query("DELETE FROM tenants WHERE tenant_id = $1")
                    .bind(tenant_id)
                    .execute(pool)
                    .await?;

                Ok(())
            }
            TenantBackend::Supabase { client, url, key } => {
                let app_data_endpoint =
                    format!("{}/rest/v1/app_data_enc?tenant_id=eq.{}", url, tenant_id);
                let _ = client
                    .delete(&app_data_endpoint)
                    .header("apikey", key)
                    .header("Authorization", format!("Bearer {}", key))
                    .send()
                    .await?;

                let tenants_endpoint =
                    format!("{}/rest/v1/tenants?tenant_id=eq.{}", url, tenant_id);
                let resp = client
                    .delete(&tenants_endpoint)
                    .header("apikey", key)
                    .header("Authorization", format!("Bearer {}", key))
                    .send()
                    .await?;

                if !resp.status().is_success() {
                    anyhow::bail!("Supabase delete tenant failed: {}", resp.status());
                }

                Ok(())
            }
        }
    }

    /// Insert an encrypted app data record linked to a tenant (useful for testing cascade delete).
    pub async fn insert_app_data_enc(
        &self,
        id: &str,
        tenant_id: &str,
        payload: &str,
    ) -> Result<()> {
        match &self.backend {
            TenantBackend::Neon { pool } => {
                sqlx::query(
                    r#"
                    INSERT INTO app_data_enc (id, tenant_id, payload)
                    VALUES ($1, $2, $3)
                    ON CONFLICT (id) DO UPDATE SET payload = EXCLUDED.payload
                    "#,
                )
                .bind(id)
                .bind(tenant_id)
                .bind(payload)
                .execute(pool)
                .await?;
                Ok(())
            }
            TenantBackend::Supabase { client, url, key } => {
                let endpoint = format!("{}/rest/v1/app_data_enc", url);
                let body = serde_json::json!({
                    "id": id,
                    "tenant_id": tenant_id,
                    "payload": payload
                });
                let resp = client
                    .post(&endpoint)
                    .header("apikey", key)
                    .header("Authorization", format!("Bearer {}", key))
                    .header("Content-Type", "application/json")
                    .header("Prefer", "resolution=merge-duplicates")
                    .json(&body)
                    .send()
                    .await?;
                if !resp.status().is_success() && resp.status() != reqwest::StatusCode::CREATED {
                    anyhow::bail!("Supabase insert_app_data_enc failed: {}", resp.status());
                }
                Ok(())
            }
        }
    }

    /// Count app_data_enc records linked to a given tenant_id.
    pub async fn count_app_data_enc(&self, tenant_id: &str) -> Result<i64> {
        match &self.backend {
            TenantBackend::Neon { pool } => {
                let row = sqlx::query(
                    "SELECT COUNT(*) as count FROM app_data_enc WHERE tenant_id = $1",
                )
                .bind(tenant_id)
                .fetch_one(pool)
                .await?;
                use sqlx::Row;
                let count: i64 = row.try_get("count")?;
                Ok(count)
            }
            TenantBackend::Supabase { client, url, key } => {
                let endpoint = format!(
                    "{}/rest/v1/app_data_enc?tenant_id=eq.{}&select=id",
                    url, tenant_id
                );
                let resp = client
                    .get(&endpoint)
                    .header("apikey", key)
                    .header("Authorization", format!("Bearer {}", key))
                    .send()
                    .await?;
                if !resp.status().is_success() {
                    anyhow::bail!("Supabase count_app_data_enc failed: {}", resp.status());
                }
                let items: Vec<serde_json::Value> = resp.json().await?;
                Ok(items.len() as i64)
            }
        }
    }
}
