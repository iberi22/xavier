// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Time Metrics storage for Xavier.
//!
//! Stores TimeMetric records to SQLite at path: metrics/time/{YYYY-MM-DD}/{metric_type}/{agent_id}

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::params;

use crate::adapters::inbound::http::dto::TimeMetricDto;
use crate::codebase::connection_manager::ConnectionManager;
use crate::ports::outbound::schema_init::SchemaInitializer;

/// Table name for time metrics
const TABLE_TIME_METRICS: &str = "time_metrics";

/// Time metrics storage adapter
pub struct TimeMetricsStore {
    pub project_id: String,
}

impl Default for TimeMetricsStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeMetricsStore {
    /// Create a new TimeMetricsStore
    pub fn new() -> Self {
        let project_id = "metrics";
        if let Err(e) = ConnectionManager::global().connect(project_id, ".") {
            tracing::warn!(
                "TimeMetricsStore failed to connect to metrics database: {}",
                e
            );
        }
        Self {
            project_id: project_id.to_string(),
        }
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

        let workspace_id = workspace_id.to_string();
        let metric_type = metric.metric_type.to_string();
        let agent_id = metric.agent_id.to_string();
        let task_id = metric.task_id.clone();
        let started_at = metric.started_at.to_string();
        let completed_at = metric.completed_at.to_string();
        let duration_ms = metric.duration_ms;
        let status = metric.status.to_string();
        let error_message = metric.error_message.clone();
        let provider = metric.provider.clone();
        let model = metric.model.clone();
        let tokens_used = metric.tokens_used.map(|t| t as i64);
        let task_category = metric.task_category.clone();

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute(
                &format!(
                    "INSERT INTO {} (id, workspace_id, path, metric_type, agent_id, task_id, \
                     started_at, completed_at, duration_ms, status, error_message, provider, \
                     model, tokens_used, task_category, metadata) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                    TABLE_TIME_METRICS
                ),
                params![
                    id,
                    workspace_id,
                    path,
                    metric_type,
                    agent_id,
                    task_id,
                    started_at,
                    completed_at,
                    duration_ms,
                    status,
                    error_message,
                    provider,
                    model,
                    tokens_used,
                    task_category,
                    metadata_json,
                ],
            ).context("failed to insert time metric")?;
            Ok(())
        }).await.map_err(|e: anyhow::Error| e.to_string())
    }

    pub async fn init_schema_async(&self) -> Result<()> {
        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
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
                ))
                .context("failed to init time metrics schema")?;
                Ok(())
            })
            .await
    }
}

impl SchemaInitializer for TimeMetricsStore {
    /// Initialize the time_metrics table schema
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
