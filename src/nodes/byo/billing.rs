//! Billing Records Manager for BYO persistence nodes.
//!
//! Provides subscription billing lifecycle (create, get_active, cancel, list_history)
//! backed by PostgreSQL with NUMERIC(12,2) amount storage and opaque payment reference hashing.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use std::fmt;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::utils::crypto::sha256_hex;

/// Supported billing plans with corresponding prices in USDC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BillingPlan {
    Free,
    Micro,
    Socio,
    Nodo,
}

impl BillingPlan {
    /// Monthly price in USDC.
    pub fn amount_usdc(&self) -> f64 {
        match self {
            Self::Free => 0.0,
            Self::Micro => 3.0,
            Self::Socio => 8.0,
            Self::Nodo => 15.0,
        }
    }

    /// Formatted monthly price string for NUMERIC(12,2).
    pub fn amount_usdc_str(&self) -> String {
        format!("{:.2}", self.amount_usdc())
    }
}

impl fmt::Display for BillingPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Free => write!(f, "free"),
            Self::Micro => write!(f, "micro"),
            Self::Socio => write!(f, "socio"),
            Self::Nodo => write!(f, "nodo"),
        }
    }
}

impl FromStr for BillingPlan {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "free" => Ok(Self::Free),
            "micro" => Ok(Self::Micro),
            "socio" => Ok(Self::Socio),
            "nodo" => Ok(Self::Nodo),
            _ => Err(anyhow!("Invalid billing plan: '{}'", s)),
        }
    }
}

/// Billing record stored in PostgreSQL.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BillingRecord {
    pub billing_id: String,
    pub tenant_id: String,
    pub plan: BillingPlan,
    pub amount_usdc: String,
    pub period: String,
    pub payment_ref: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Manager handling subscription billing lifecycle in PostgreSQL.
#[derive(Clone)]
pub struct BillingManager {
    pool: PgPool,
}

impl BillingManager {
    /// Create a new BillingManager instance.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Ensure table and indexes exist in PostgreSQL.
    pub async fn init_schema(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS billing_records (
                billing_id VARCHAR(64) PRIMARY KEY,
                tenant_id VARCHAR(64) NOT NULL,
                plan VARCHAR(32) NOT NULL,
                amount_usdc NUMERIC(12,2) NOT NULL,
                period VARCHAR(32) NOT NULL,
                payment_ref VARCHAR(128) NOT NULL,
                status VARCHAR(32) NOT NULL,
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_billing_records_tenant ON billing_records(tenant_id);
            CREATE INDEX IF NOT EXISTS idx_billing_records_active ON billing_records(tenant_id, status);
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Create a new billing record for a tenant.
    ///
    /// `payment_ref` is converted into an opaque SHA-256 hash to ensure no plaintext PII is stored.
    pub async fn create_record(
        &self,
        tenant: &str,
        plan: BillingPlan,
        period: &str,
        payment_ref: &str,
    ) -> Result<BillingRecord> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let billing_id = format!("bill_{}_{}", tenant, now);
        let opaque_payment_ref = sha256_hex(payment_ref.as_bytes());
        let amount_str = plan.amount_usdc_str();
        let plan_str = plan.to_string();
        let status = "active";

        sqlx::query(
            r#"
            INSERT INTO billing_records
                (billing_id, tenant_id, plan, amount_usdc, period, payment_ref, status, created_at, updated_at)
            VALUES
                ($1, $2, $3, $4::numeric, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(&billing_id)
        .bind(tenant)
        .bind(&plan_str)
        .bind(&amount_str)
        .bind(period)
        .bind(&opaque_payment_ref)
        .bind(status)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(BillingRecord {
            billing_id,
            tenant_id: tenant.to_string(),
            plan,
            amount_usdc: amount_str,
            period: period.to_string(),
            payment_ref: opaque_payment_ref,
            status: status.to_string(),
            created_at: now,
            updated_at: now,
        })
    }

    /// Fetch active billing subscription for a tenant.
    pub async fn get_active(&self, tenant: &str) -> Result<Option<BillingRecord>> {
        let row = sqlx::query(
            r#"
            SELECT billing_id, tenant_id, plan, amount_usdc::text, period, payment_ref, status, created_at, updated_at
            FROM billing_records
            WHERE tenant_id = $1 AND status = 'active'
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(tenant)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let plan_str: String = row.get("plan");
            let plan = BillingPlan::from_str(&plan_str)?;

            Ok(Some(BillingRecord {
                billing_id: row.get("billing_id"),
                tenant_id: row.get("tenant_id"),
                plan,
                amount_usdc: row.get("amount_usdc"),
                period: row.get("period"),
                payment_ref: row.get("payment_ref"),
                status: row.get("status"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            }))
        } else {
            Ok(None)
        }
    }

    /// Cancel a billing record by billing_id.
    pub async fn cancel(&self, billing_id: &str) -> Result<BillingRecord> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let row = sqlx::query(
            r#"
            UPDATE billing_records
            SET status = 'cancelled', updated_at = $1
            WHERE billing_id = $2
            RETURNING billing_id, tenant_id, plan, amount_usdc::text, period, payment_ref, status, created_at, updated_at
            "#,
        )
        .bind(now)
        .bind(billing_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| anyhow!("Billing record '{}' not found", billing_id))?;

        let plan_str: String = row.get("plan");
        let plan = BillingPlan::from_str(&plan_str)?;

        Ok(BillingRecord {
            billing_id: row.get("billing_id"),
            tenant_id: row.get("tenant_id"),
            plan,
            amount_usdc: row.get("amount_usdc"),
            period: row.get("period"),
            payment_ref: row.get("payment_ref"),
            status: row.get("status"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }

    /// List full billing history for a tenant.
    pub async fn list_history(&self, tenant: &str) -> Result<Vec<BillingRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT billing_id, tenant_id, plan, amount_usdc::text, period, payment_ref, status, created_at, updated_at
            FROM billing_records
            WHERE tenant_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(tenant)
        .fetch_all(&self.pool)
        .await?;

        let mut history = Vec::new();
        for row in rows {
            let plan_str: String = row.get("plan");
            let plan = BillingPlan::from_str(&plan_str)?;

            history.push(BillingRecord {
                billing_id: row.get("billing_id"),
                tenant_id: row.get("tenant_id"),
                plan,
                amount_usdc: row.get("amount_usdc"),
                period: row.get("period"),
                payment_ref: row.get("payment_ref"),
                status: row.get("status"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            });
        }

        Ok(history)
    }
}
