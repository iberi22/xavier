//! HTTP adapter for timing and performance metrics
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use async_trait::async_trait;
use std::sync::Arc;

use crate::domain::cycle_breaks::w30_02::TimeMetricSink;
use crate::domain::memory::TimeMetric;
use crate::ports::inbound::TimeMetricsPort;

/// Inbound adapter that wraps a time-metric sink and implements TimeMetricsPort
///
/// The dependency is the `TimeMetricSink` contract, not the concrete store, so this
/// module no longer imports the store module (ADR-033 Wave 0).
pub struct TimeMetricsAdapter {
    store: Arc<dyn TimeMetricSink>,
}

impl TimeMetricsAdapter {
    /// New.
    pub fn new(store: Arc<dyn TimeMetricSink>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl TimeMetricsPort for TimeMetricsAdapter {
    async fn save_time_metric(
        &self,
        metric: &TimeMetric,
        workspace_id: &str,
    ) -> Result<(), String> {
        self.store.persist_time_metric(metric, workspace_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct RecordingSink {
        saved: Mutex<Vec<(String, String)>>,
    }

    impl RecordingSink {
        fn new() -> Self {
            Self {
                saved: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl TimeMetricSink for RecordingSink {
        async fn persist_time_metric(
            &self,
            metric: &TimeMetric,
            workspace_id: &str,
        ) -> Result<(), String> {
            self.saved
                .lock()
                .expect("test lock")
                .push((metric.metric_type.clone(), workspace_id.to_string()));
            Ok(())
        }
    }

    fn sample_metric() -> TimeMetric {
        TimeMetric {
            metric_type: "task".to_string(),
            agent_id: "agent-1".to_string(),
            task_id: None,
            started_at: "2026-09-29T00:00:00Z".to_string(),
            completed_at: "2026-09-29T00:00:01Z".to_string(),
            duration_ms: 1_000,
            status: "ok".to_string(),
            error_message: None,
            provider: None,
            model: None,
            tokens_used: None,
            task_category: None,
            metadata: serde_json::json!({}),
        }
    }

    /// The port must reach the sink with the metric and the workspace untouched:
    /// the inversion is a wiring change, the forwarded payload is the same one the
    /// concrete store used to receive.
    #[tokio::test]
    async fn port_forwards_metric_and_workspace_to_the_sink() {
        let sink = Arc::new(RecordingSink::new());
        let adapter = TimeMetricsAdapter::new(sink.clone());

        let result = adapter
            .save_time_metric(&sample_metric(), "workspace-7")
            .await;

        assert_eq!(result, Ok(()));
        let saved = sink.saved.lock().expect("test lock").clone();
        assert_eq!(saved, vec![("task".to_string(), "workspace-7".to_string())]);
    }
}
