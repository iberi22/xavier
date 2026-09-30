//! Owned by issue [WAVE-30.02] (ADR-033 Wave 0).

use async_trait::async_trait;

#[async_trait]
pub trait TimeMetricSaver: Send + Sync {
    async fn save_time_metric(
        &self,
        metric: &crate::adapters::inbound::http::dto::TimeMetricDto,
        workspace_id: &str,
    ) -> Result<(), String>;
}
