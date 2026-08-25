use chrono::Utc;
use sqlx::{PgPool, Row};

/// AI Usage Tracker for BYO nodes.
#[derive(Clone, Debug)]
pub struct UsageTracker {
    pool: PgPool,
}

impl UsageTracker {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Records AI credit usage units for a tenant and usage kind.
    /// Uses upsert semantics (ON CONFLICT on PK tenant_id, month, kind) to increment units.
    pub async fn record(&self, tenant: &str, kind: &str, units: i64) -> Result<(), sqlx::Error> {
        let month = Utc::now().format("%Y-%m").to_string();

        sqlx::query(
            r#"
            INSERT INTO usage_metrics (tenant_id, month, kind, units, updated_at)
            VALUES ($1, $2, $3, $4, NOW())
            ON CONFLICT (tenant_id, month, kind)
            DO UPDATE SET units = usage_metrics.units + EXCLUDED.units, updated_at = NOW()
            "#,
        )
        .bind(tenant)
        .bind(month)
        .bind(kind)
        .bind(units)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Returns total usage units for a tenant in a given month ('YYYY-MM') across all kinds.
    pub async fn monthly_total(&self, tenant: &str, month: &str) -> Result<i64, sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT COALESCE(SUM(units), 0)::BIGINT as total
            FROM usage_metrics
            WHERE tenant_id = $1 AND month = $2
            "#,
        )
        .bind(tenant)
        .bind(month)
        .fetch_one(&self.pool)
        .await?;

        let total: i64 = row.try_get("total")?;
        Ok(total)
    }

    /// Checks if a tenant is within their plan's monthly AI credits quota for the current month.
    /// Plan limits: free=50, micro=400, socio=1500, nodo=unlimited.
    pub async fn check_quota(&self, tenant: &str, plan: &str) -> Result<bool, sqlx::Error> {
        let limit: i64 = match plan.to_ascii_lowercase().as_str() {
            "free" => 50,
            "micro" => 400,
            "socio" => 1500,
            "nodo" => i64::MAX,
            _ => 50, // default fallback to free tier
        };

        if limit == i64::MAX {
            return Ok(true);
        }

        let month = Utc::now().format("%Y-%m").to_string();
        let current_total = self.monthly_total(tenant, &month).await?;

        Ok(current_total < limit)
    }
}
