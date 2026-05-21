use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::ports::outbound::schema_init::SchemaInitializer;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaStatus {
    pub provider: String,
    pub used_hourly: usize,
    pub used_today: usize,
    pub used_weekly: usize,
    pub used_monthly: usize,
    pub weekly_quota: usize,
    pub cache_hits: usize,
    pub rate_limited_until: Option<DateTime<Utc>>,
    pub last_update: DateTime<Utc>,
}

use crate::utils::connection_pool::LibsqlConnectionPool;

pub struct RateLimitManager {
    db: LibsqlConnectionPool,
}

impl RateLimitManager {
    pub fn new(db: LibsqlConnectionPool) -> Self {
        Self { db }
    }

    pub fn db(&self) -> &LibsqlConnectionPool {
        &self.db
    }

    pub async fn track_request(
        &self,
        provider: &str,
        tokens: usize,
        status: u16,
        cost_usd: f64,
        is_cache_hit: bool,
    ) -> Result<()> {
        let conn = self.db.get().await?;
        conn.execute(
            "INSERT INTO rate_limit_usage (provider, tokens_used, cost_usd, status_code, is_error)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            (provider.to_string(), tokens as i64, cost_usd, status as i64, (status >= 400) as i64),
        ).await?;

        if is_cache_hit {
            conn.execute(
                "UPDATE rate_limit_usage SET cache_hits = cache_hits + 1 WHERE id = last_insert_rowid()",
                (),
            ).await?;
        }

        if status == 429 {
            // Default cooldown of 5 minutes if not specified
            let until = Utc::now() + Duration::minutes(5);
            conn.execute(
                "INSERT INTO provider_quotas (provider, rate_limited_until)
                 VALUES (?1, ?2)
                 ON CONFLICT(provider) DO UPDATE SET rate_limited_until = ?2",
                (provider.to_string(), until.to_rfc3339()),
            ).await?;
        }

        Ok(())
    }

    pub async fn get_status(&self, provider: &str) -> Result<QuotaStatus> {
        let now = Utc::now();
        let hour_ago = now - Duration::hours(1);
        let day_ago = now - Duration::days(1);
        let week_ago = now - Duration::days(7);
        let month_ago = now - Duration::days(30);

        let conn = self.db.get().await?;
        let mut stmt = conn.prepare(
            "SELECT
                COALESCE(SUM(CASE WHEN timestamp > ?1 THEN tokens_used ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN timestamp > ?2 THEN tokens_used ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN timestamp > ?3 THEN tokens_used ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN timestamp > ?4 THEN tokens_used ELSE 0 END), 0),
                COALESCE(SUM(cache_hits), 0)
             FROM rate_limit_usage
             WHERE provider = ?5",
        ).await?;

        let mut rows = stmt.query((
            hour_ago.format("%Y-%m-%d %H:%M:%f").to_string(),
            day_ago.format("%Y-%m-%d %H:%M:%f").to_string(),
            week_ago.format("%Y-%m-%d %H:%M:%f").to_string(),
            month_ago.format("%Y-%m-%d %H:%M:%f").to_string(),
            provider.to_string(),
        )).await?;

        let (used_hourly, used_today, used_weekly, used_monthly, cache_hits) = if let Some(row) = rows.next().await? {
            (
                row.get::<i64>(0).unwrap_or(0),
                row.get::<i64>(1).unwrap_or(0),
                row.get::<i64>(2).unwrap_or(0),
                row.get::<i64>(3).unwrap_or(0),
                row.get::<i64>(4).unwrap_or(0),
            )
        } else {
            (0, 0, 0, 0, 0)
        };

        let mut quota_stmt = conn.prepare(
            "SELECT rate_limited_until, COALESCE(weekly_quota, 1000000) FROM provider_quotas WHERE provider = ?1"
        ).await?;
        let mut quota_rows = quota_stmt.query([provider.to_string()]).await?;

        let (rate_limited_until, weekly_quota) = if let Some(row) = quota_rows.next().await? {
            let until_str: Option<String> = row.get(0).ok();
            let until = until_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .ok()
            });
            let quota = row.get::<i64>(1).unwrap_or(1000000) as usize;
            (until, quota)
        } else {
            (None, 1000000)
        };

        Ok(QuotaStatus {
            provider: provider.to_string(),
            used_hourly: used_hourly as usize,
            used_today: used_today as usize,
            used_weekly: used_weekly as usize,
            used_monthly: used_monthly as usize,
            weekly_quota,
            cache_hits: cache_hits as usize,
            rate_limited_until,
            last_update: now,
        })
    }

    pub async fn get_daily_summary(&self, provider: &str) -> Result<serde_json::Value> {
        let now = Utc::now();
        let day_ago = now - Duration::days(1);

        let conn = self.db.get().await?;
        let mut stmt = conn.prepare(
            "SELECT timestamp, tokens_used, status_code FROM rate_limit_usage
             WHERE provider = ?1 AND timestamp > ?2
             ORDER BY timestamp ASC",
        ).await?;

        let mut rows = stmt.query((provider.to_string(), day_ago.to_rfc3339())).await?;
        let mut requests = Vec::new();

        while let Some(row) = rows.next().await? {
            let ts_str: String = row.get(0)?;
            let ts = DateTime::parse_from_rfc3339(&ts_str)
                .map(|dt| dt.with_timezone(&Utc))
                .or_else(|_| {
                    chrono::NaiveDateTime::parse_from_str(&ts_str, "%Y-%m-%d %H:%M:%f")
                        .map(|ndt| DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc))
                })
                .unwrap_or_else(|_| Utc::now());

            let tokens: i64 = row.get(1)?;
            let status: i64 = row.get(2)?;

            requests.push(serde_json::json!({
                "ts": ts,
                "tokens": tokens,
                "status": status as u16,
            }));
        }

        let total = requests.len();
        let tokens: i64 = requests
            .iter()
            .map(|r| r["tokens"].as_i64().unwrap_or(0))
            .sum();

        let status = self.get_status(provider).await?;

        Ok(serde_json::json!({
            "requests": requests,
            "daily_total": total,
            "daily_tokens": tokens,
            "rate_limited": status.rate_limited_until.is_some_and(|until| until > now),
            "cooldown_until": status.rate_limited_until,
        }))
    }

    pub async fn is_quota_low(&self, provider: &str) -> Result<bool> {
        let status = self.get_status(provider).await?;
        if status.weekly_quota == 0 {
            return Ok(false);
        }

        let used_ratio = status.used_weekly as f32 / status.weekly_quota as f32;
        Ok(used_ratio > 0.9)
    }

    pub async fn report_429(&self, provider: &str, cooldown_minutes: i64) -> Result<()> {
        let until = Utc::now() + Duration::minutes(cooldown_minutes);
        let conn = self.db.get().await?;
        conn.execute(
            "INSERT INTO provider_quotas (provider, rate_limited_until)
             VALUES (?1, ?2)
             ON CONFLICT(provider) DO UPDATE SET rate_limited_until = ?2",
            (provider.to_string(), until.to_rfc3339()),
        ).await?;
        Ok(())
    }

    pub async fn get_all_providers(&self) -> Result<Vec<String>> {
        let conn = self.db.get().await?;
        let mut stmt = conn.prepare("SELECT DISTINCT provider FROM rate_limit_usage").await?;
        let mut rows = stmt.query(()).await?;
        let mut providers = Vec::new();
        while let Some(row) = rows.next().await? {
            if let Ok(p) = row.get::<String>(0) {
                providers.push(p);
            }
        }
        Ok(providers)
    }

    pub async fn update_manual_limit(&self, provider: &str, percentage: f32) -> Result<()> {
        let conn = self.db.get().await?;
        conn.execute(
            "INSERT INTO provider_quotas (provider, manual_limit_percentage, last_manual_update)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(provider) DO UPDATE SET manual_limit_percentage = ?2, last_manual_update = ?3",
            (provider.to_string(), percentage as f64, Utc::now().to_rfc3339()),
        ).await?;
        Ok(())
    }

    pub async fn init_schema_async(&self) -> Result<()> {
        let conn = self.db.get().await?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS rate_limit_usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider TEXT NOT NULL,
                timestamp DATETIME DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now')),
                tokens_used INTEGER DEFAULT 0,
                cost_usd REAL DEFAULT 0.0,
                status_code INTEGER,
                is_error BOOLEAN DEFAULT 0,
                cache_hits INTEGER DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS provider_quotas (
                provider TEXT PRIMARY KEY,
                rate_limited_until DATETIME,
                manual_limit_percentage REAL DEFAULT 0.0,
                last_manual_update DATETIME,
                weekly_quota INTEGER DEFAULT 1000000
            );"
        ).await?;

        // Defensive column migrations
        let _ = conn.execute("ALTER TABLE rate_limit_usage ADD COLUMN cache_hits INTEGER DEFAULT 0", ()).await;
        let _ = conn.execute("ALTER TABLE rate_limit_usage ADD COLUMN cost_usd REAL DEFAULT 0.0", ()).await;
        let _ = conn.execute("ALTER TABLE provider_quotas ADD COLUMN weekly_quota INTEGER DEFAULT 1000000", ()).await;

        Ok(())
    }
}

impl SchemaInitializer for RateLimitManager {
    fn init_schema(&self) -> Result<()> {
        std::thread::scope(|s| {
            s.spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| anyhow::anyhow!("failed to build runtime for rate limit schema: {}", e))?;
                rt.block_on(self.init_schema_async())
            })
            .join()
            .map_err(|_| anyhow::anyhow!("rate limit schema thread panicked"))?
        })
    }
}


