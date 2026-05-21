//! Time Metrics storage for Xavier.
//!
//! Stores TimeMetric records to SQLite at path: metrics/time/{YYYY-MM-DD}/{metric_type}/{agent_id}

use anyhow::Result;
use chrono::Utc;

use crate::adapters::inbound::http::dto::TimeMetricDto;
use crate::ports::outbound::schema_init::SchemaInitializer;

use crate::utils::connection_pool::LibsqlConnectionPool;

/// Table name for time metrics
const TABLE_TIME_METRICS: &str = "time_metrics";

/// Time metrics storage adapter
pub struct TimeMetricsStore {
    pub pool: LibsqlConnectionPool,
}

impl TimeMetricsStore {
    /// Create a new TimeMetricsStore with the given libSQL connection pool
    pub fn new(pool: LibsqlConnectionPool) -> Self {
        Self { pool }
    }

    /// Save a TimeMetric to the store
    pub async fn save_time_metric(
        &self,
        metric: &TimeMetricDto,
        workspace_id: &str,
    ) -> Result<(), String> {
        let id = ulid::Ulid::new().to_string();
        let now = Utc::now();
        let date = now.format("%Y-%m-%d").to_string();

        // Build path: metrics/time/{YYYY-MM-DD}/{metric_type}/{agent_id}
        let path = format!(
            "metrics/time/{}/{}/{}",
            date, metric.metric_type, metric.agent_id
        );

        let metadata_json = serde_json::to_string(&metric.metadata).map_err(|e| e.to_string())?;

        let conn = self.pool.get().await.map_err(|e| e.to_string())?;
        conn.execute(
            &format!(
                "INSERT INTO {} (id, workspace_id, path, metric_type, agent_id, task_id, \
                 started_at, completed_at, duration_ms, status, error_message, provider, \
                 model, tokens_used, task_category, metadata) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                TABLE_TIME_METRICS
            ),
            (
                id,
                workspace_id.to_string(),
                path,
                metric.metric_type.to_string(),
                metric.agent_id.to_string(),
                metric.task_id.clone(),
                metric.started_at.to_string(),
                metric.completed_at.to_string(),
                metric.duration_ms,
                metric.status.to_string(),
                metric.error_message.clone(),
                metric.provider.clone(),
                metric.model.clone(),
                metric.tokens_used.map(|t| t as i64),
                metric.task_category.clone(),
                metadata_json,
            ),
        )
        .await
        .map_err(|e| e.to_string())?;

        Ok(())
    }

    pub async fn init_schema_async(&self) -> Result<()> {
        let conn = self.pool.get().await?;
        conn.execute_batch(&format!(
            r#"
            CREATE TABLE IF NOT EXISTS {} (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                path TEXT NOT NULL,
                metric_type TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                task_id TEXT,
                started_at TEXT NOT NULL,
                completed_at TEXT NOT NULL,
                duration_ms INTEGER NOT NULL,
                status TEXT NOT NULL,
                error_message TEXT,
                provider TEXT,
                model TEXT,
                tokens_used INTEGER,
                task_category TEXT,
                metadata TEXT NOT NULL DEFAULT '{{}}',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_time_metrics_workspace ON {}(workspace_id);
            CREATE INDEX IF NOT EXISTS idx_time_metrics_agent ON {}(agent_id);
            CREATE INDEX IF NOT EXISTS idx_time_metrics_type ON {}(metric_type);
            CREATE INDEX IF NOT EXISTS idx_time_metrics_path ON {}(path);
            "#,
            TABLE_TIME_METRICS,
            TABLE_TIME_METRICS,
            TABLE_TIME_METRICS,
            TABLE_TIME_METRICS,
            TABLE_TIME_METRICS
        )).await?;
        Ok(())
    }
}

impl SchemaInitializer for TimeMetricsStore {
    /// Initialize the time_metrics table schema
    fn init_schema(&self) -> Result<()> {
        std::thread::scope(|s| {
            s.spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| anyhow::anyhow!("failed to build runtime for time schema: {}", e))?;
                rt.block_on(self.init_schema_async())
            })
            .join()
            .map_err(|_| anyhow::anyhow!("time schema thread panicked"))?
        })
    }
}
