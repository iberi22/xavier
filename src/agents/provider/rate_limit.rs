//! Provider rate limiting and token bucket implementation.
//!
//! Implements per-provider rate limiting with configurable RPM/TPM
//! limits, token bucket throttling, and back-pressure for API calls.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::codebase::connection_manager::ConnectionManager;
use crate::domain::proxy::types::{ApiTier, ProviderKind, ProviderQuota};
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

pub struct RateLimitManager {
    project_id: String,
}

impl Default for RateLimitManager {
    fn default() -> Self {
        Self::new_with_project("metrics")
    }
}

impl RateLimitManager {
    /// Creates a new RateLimitManager, connecting to the metrics database.
    /// Logs a warning if the connection fails (non-fatal for rate limiting).
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new RateLimitManager with a specific project ID for testing or isolation.
    pub fn new_with_project(project_id: &str) -> Self {
        if let Err(e) = ConnectionManager::global().connect(project_id, ".") {
            warn!(
                "RateLimitManager failed to connect to {} DB: {}",
                project_id, e
            );
        }
        Self {
            project_id: project_id.to_string(),
        }
    }

    /// Checks if a provider is currently rate limited.
    pub async fn check(&self, provider: &str) -> bool {
        if let Ok(status) = self.get_status(provider).await {
            let now = Utc::now();
            return status.rate_limited_until.is_some_and(|until| until > now);
        }
        false
    }

    /// Checks if a lease token has exceeded the rate limit (default 100 req/min).
    pub async fn check_lease_rate_limit(
        &self,
        lease_token: &str,
        limit_per_min: usize,
    ) -> Result<bool> {
        self.check_rpm_limit(&format!("lease:{}", lease_token), limit_per_min)
            .await
    }

    /// Checks if a generic identifier has exceeded its requests-per-minute (RPM) limit.
    pub async fn check_rpm_limit(&self, identifier: &str, limit: usize) -> Result<bool> {
        let now = Utc::now();
        let minute_ago = now - Duration::minutes(1);
        let identifier = identifier.to_string();

        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                let count: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM rate_limit_usage
                 WHERE provider = ?1 AND timestamp > ?2",
                        params![
                            identifier,
                            minute_ago.format("%Y-%m-%d %H:%M:%f").to_string()
                        ],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);

                Ok(count < limit as i64)
            })
            .await
    }

    /// Resets the rate limit for a provider (clears cooldown).
    pub async fn reset(&self, provider: &str) -> Result<()> {
        let provider = provider.to_string();
        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                conn.execute(
                    "UPDATE provider_quotas SET rate_limited_until = NULL WHERE provider = ?1",
                    params![provider],
                )?;
                Ok(())
            })
            .await
    }

    pub async fn track_request(
        &self,
        provider: &str,
        tokens: usize,
        status: u16,
        cost_usd: f64,
        is_cache_hit: bool,
    ) -> Result<()> {
        let provider = provider.to_string();

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute(
                "INSERT INTO rate_limit_usage (provider, tokens_used, cost_usd, status_code, is_error)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    provider,
                    tokens as i64,
                    cost_usd,
                    status as i64,
                    (status >= 400) as i64,
                ],
            )?;

            if is_cache_hit {
                conn.execute(
                    "UPDATE rate_limit_usage SET cache_hits = cache_hits + 1 WHERE id = last_insert_rowid()",
                    (),
                )?;
            }

            if status == 429 {
                // Default cooldown of 5 minutes if not specified
                let until = Utc::now() + Duration::minutes(5);
                conn.execute(
                    "INSERT INTO provider_quotas (provider, rate_limited_until)
                     VALUES (?1, ?2)
                     ON CONFLICT(provider) DO UPDATE SET rate_limited_until = ?2",
                    params![provider, until.to_rfc3339()],
                )?;
            }

            Ok(())
        }).await
    }

    pub async fn get_status(&self, provider: &str) -> Result<QuotaStatus> {
        let now = Utc::now();
        let hour_ago = now - Duration::hours(1);
        let day_ago = now - Duration::days(1);
        let week_ago = now - Duration::days(7);
        let month_ago = now - Duration::days(30);
        let provider = provider.to_string();

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            let (used_hourly, used_today, used_weekly, used_monthly, cache_hits) = conn.query_row(
                "SELECT
                    COALESCE(SUM(CASE WHEN timestamp > ?1 THEN tokens_used ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN timestamp > ?2 THEN tokens_used ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN timestamp > ?3 THEN tokens_used ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN timestamp > ?4 THEN tokens_used ELSE 0 END), 0),
                    COALESCE(SUM(cache_hits), 0)
                 FROM rate_limit_usage
                 WHERE provider = ?5",
                params![
                    hour_ago.format("%Y-%m-%d %H:%M:%f").to_string(),
                    day_ago.format("%Y-%m-%d %H:%M:%f").to_string(),
                    week_ago.format("%Y-%m-%d %H:%M:%f").to_string(),
                    month_ago.format("%Y-%m-%d %H:%M:%f").to_string(),
                    provider,
                ],
                |row| Ok((
                    row.get::<_, i64>(0).unwrap_or(0),
                    row.get::<_, i64>(1).unwrap_or(0),
                    row.get::<_, i64>(2).unwrap_or(0),
                    row.get::<_, i64>(3).unwrap_or(0),
                    row.get::<_, i64>(4).unwrap_or(0),
                ))
            ).unwrap_or((0, 0, 0, 0, 0));

            let (rate_limited_until, weekly_quota) = conn.query_row(
                "SELECT rate_limited_until, COALESCE(weekly_quota, 1000000) FROM provider_quotas WHERE provider = ?1",
                params![provider],
                |row| {
                    let until_str: Option<String> = row.get(0).ok();
                    let until = until_str.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .map(|dt| dt.with_timezone(&Utc))
                            .ok()
                    });
                    let quota = row.get::<_, i64>(1).unwrap_or(1000000) as usize;
                    Ok((until, quota))
                }
            ).unwrap_or((None, 1000000));

            Ok(QuotaStatus {
                provider: provider.clone(),
                used_hourly: used_hourly as usize,
                used_today: used_today as usize,
                used_weekly: used_weekly as usize,
                used_monthly: used_monthly as usize,
                weekly_quota,
                cache_hits: cache_hits as usize,
                rate_limited_until,
                last_update: now,
            })
        }).await
    }

    pub async fn get_daily_summary(&self, provider: &str) -> Result<serde_json::Value> {
        let now = Utc::now();
        let day_ago = now - Duration::days(1);
        let provider_id = provider.to_string();

        let requests = ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT timestamp, tokens_used, status_code FROM rate_limit_usage
                 WHERE provider = ?1 AND timestamp > ?2
                 ORDER BY timestamp ASC",
                )?;

                let mut rows = stmt.query(params![provider_id, day_ago.to_rfc3339()])?;
                let mut internal_requests = Vec::new();

                while let Some(row) = rows.next()? {
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

                    internal_requests.push(serde_json::json!({
                        "ts": ts,
                        "tokens": tokens,
                        "status": status as u16,
                    }));
                }
                Ok(internal_requests)
            })
            .await?;

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
        let provider = provider.to_string();
        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                conn.execute(
                    "INSERT INTO provider_quotas (provider, rate_limited_until)
                 VALUES (?1, ?2)
                 ON CONFLICT(provider) DO UPDATE SET rate_limited_until = ?2",
                    params![provider, until.to_rfc3339()],
                )?;
                Ok(())
            })
            .await
    }

    pub async fn get_all_providers(&self) -> Result<Vec<String>> {
        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                let mut stmt = conn.prepare("SELECT DISTINCT provider FROM rate_limit_usage")?;
                let mut rows = stmt.query(())?;
                let mut providers = Vec::new();
                while let Some(row) = rows.next()? {
                    if let Ok(p) = row.get::<_, String>(0) {
                        providers.push(p);
                    }
                }
                Ok(providers)
            })
            .await
    }

    pub async fn update_manual_limit(&self, provider: &str, percentage: f32) -> Result<()> {
        let provider = provider.to_string();
        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute(
                "INSERT INTO provider_quotas (provider, manual_limit_percentage, last_manual_update)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(provider) DO UPDATE SET manual_limit_percentage = ?2, last_manual_update = ?3",
                params![provider, percentage as f64, Utc::now().to_rfc3339()],
            )?;
            Ok(())
        }).await
    }

    pub async fn update_quota(&self, quota: ProviderQuota) -> Result<()> {
        let provider_name = quota.provider.as_str().to_string();
        let api_tier =
            serde_json::to_string(&quota.api_tier).unwrap_or_else(|_| "Unknown".to_string());
        let resets_at = quota.resets_at.map(|dt| dt.to_rfc3339());
        let last_checked = quota.last_checked.to_rfc3339();

        // Warning logic for 80%+ usage
        if let Some(limit) = quota.requests_limit {
            if let Some(rem) = quota.requests_remaining {
                if limit > 0 && (limit - rem) as f32 / limit as f32 >= 0.8 {
                    warn!(
                        "Provider {} is reaching request limit: {}/{} remaining",
                        provider_name, rem, limit
                    );
                }
            }
        }
        if let Some(limit) = quota.tokens_limit {
            if let Some(rem) = quota.tokens_remaining {
                if limit > 0 && (limit - rem) as f32 / limit as f32 >= 0.8 {
                    warn!(
                        "Provider {} is reaching token limit: {}/{} remaining",
                        provider_name, rem, limit
                    );
                }
            }
        }

        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                conn.execute(
                    "INSERT INTO provider_quotas (
                        provider, api_tier, requests_remaining, tokens_remaining,
                        requests_limit, tokens_limit, resets_at, last_checked
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                    ON CONFLICT(provider) DO UPDATE SET
                        api_tier = ?2,
                        requests_remaining = ?3,
                        tokens_remaining = ?4,
                        requests_limit = ?5,
                        tokens_limit = ?6,
                        resets_at = ?7,
                        last_checked = ?8",
                    params![
                        provider_name,
                        api_tier,
                        quota.requests_remaining,
                        quota.tokens_remaining,
                        quota.requests_limit,
                        quota.tokens_limit,
                        resets_at,
                        last_checked,
                    ],
                )?;
                Ok(())
            })
            .await
    }

    pub async fn get_all_quotas(&self) -> Result<Vec<ProviderQuota>> {
        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT provider, api_tier, requests_remaining, tokens_remaining,
                            requests_limit, tokens_limit, resets_at, last_checked
                     FROM provider_quotas",
                )?;
                let mut rows = stmt.query([])?;
                let mut quotas = Vec::new();

                while let Some(row) = rows.next()? {
                    let provider_str: String = row.get(0)?;
                    let tier_str: String = row.get(1).unwrap_or_else(|_| "\"Unknown\"".to_string());
                    let resets_at_str: Option<String> = row.get(6).ok();
                    let last_checked_str: String =
                        row.get(7).unwrap_or_else(|_| Utc::now().to_rfc3339());

                    let provider = ProviderKind::from_str(&provider_str);
                    let api_tier: ApiTier =
                        serde_json::from_str(&tier_str).unwrap_or(ApiTier::Unknown);
                    let resets_at = resets_at_str.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                    });
                    let last_checked = DateTime::parse_from_rfc3339(&last_checked_str)
                        .ok()
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now);

                    quotas.push(ProviderQuota {
                        provider,
                        api_tier,
                        requests_remaining: row.get(2).ok(),
                        tokens_remaining: row.get(3).ok(),
                        requests_limit: row.get(4).ok(),
                        tokens_limit: row.get(5).ok(),
                        resets_at,
                        last_checked,
                    });
                }
                Ok(quotas)
            })
            .await
    }

    pub async fn init_schema_async(&self) -> Result<()> {
        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
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
                    weekly_quota INTEGER DEFAULT 1000000,
                    api_tier TEXT,
                    requests_remaining INTEGER,
                    tokens_remaining INTEGER,
                    requests_limit INTEGER,
                    tokens_limit INTEGER,
                    resets_at DATETIME,
                    last_checked DATETIME
                );",
                )?;

                // Defensive column migrations
                let _ = conn.execute(
                    "ALTER TABLE rate_limit_usage ADD COLUMN cache_hits INTEGER DEFAULT 0",
                    (),
                );
                let _ = conn.execute(
                    "ALTER TABLE rate_limit_usage ADD COLUMN cost_usd REAL DEFAULT 0.0",
                    (),
                );
                let _ = conn.execute(
                    "ALTER TABLE provider_quotas ADD COLUMN weekly_quota INTEGER DEFAULT 1000000",
                    (),
                );
                let _ = conn.execute("ALTER TABLE provider_quotas ADD COLUMN api_tier TEXT", ());
                let _ = conn.execute(
                    "ALTER TABLE provider_quotas ADD COLUMN requests_remaining INTEGER",
                    (),
                );
                let _ = conn.execute(
                    "ALTER TABLE provider_quotas ADD COLUMN tokens_remaining INTEGER",
                    (),
                );
                let _ = conn.execute(
                    "ALTER TABLE provider_quotas ADD COLUMN requests_limit INTEGER",
                    (),
                );
                let _ = conn.execute(
                    "ALTER TABLE provider_quotas ADD COLUMN tokens_limit INTEGER",
                    (),
                );
                let _ = conn.execute(
                    "ALTER TABLE provider_quotas ADD COLUMN resets_at DATETIME",
                    (),
                );
                let _ = conn.execute(
                    "ALTER TABLE provider_quotas ADD COLUMN last_checked DATETIME",
                    (),
                );

                Ok(())
            })
            .await
    }
}

/// Compatibility wrapper for token-based quota tracking.
pub struct TokenQuotaTracker {
    manager: RateLimitManager,
}

impl TokenQuotaTracker {
    pub fn new(manager: RateLimitManager) -> Self {
        Self { manager }
    }

    pub async fn increment(&self, provider: &str, tokens: usize) -> Result<()> {
        self.manager
            .track_request(provider, tokens, 200, 0.0, false)
            .await
    }
}

/// Compatibility wrapper for general quota checking.
pub struct QuotaTracker {
    manager: RateLimitManager,
}

impl QuotaTracker {
    pub fn new(manager: RateLimitManager) -> Self {
        Self { manager }
    }

    pub async fn check(&self, provider: &str) -> bool {
        !self.manager.check(provider).await
    }
}

impl SchemaInitializer for RateLimitManager {
    fn init_schema(&self) -> Result<()> {
        match tokio::runtime::Handle::try_current() {
            Ok(_) => tokio::task::block_in_place(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .build()
                    .map_err(|e| anyhow::anyhow!("failed to create temporary runtime: {}", e))?;
                rt.block_on(self.init_schema_async())
            }),
            Err(_) => {
                let runtime = tokio::runtime::Runtime::new()
                    .map_err(|e| anyhow::anyhow!("failed to create tokio runtime: {}", e))?;
                runtime.block_on(self.init_schema_async())
            }
        }
    }
}
