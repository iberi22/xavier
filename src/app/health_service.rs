//! Health check service for system monitoring
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::ports::inbound::health_port::{HealthPort, HealthStatus};
use crate::tasks::session_sync_task::get_last_sync_result;
use async_trait::async_trait;
use std::time::Instant;

pub struct HealthService {
    pub start_time: Instant,
}

impl HealthService {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::session_sync_task::{SyncCheckResult, LAST_CHECK_RESULT};

    #[tokio::test]
    async fn test_get_health_status() {
        let service = HealthService::new();

        // Setup mock data in the static LAST_CHECK_RESULT
        {
            let mut result = LAST_CHECK_RESULT.write().unwrap();
            *result = SyncCheckResult {
                status: "ok".to_string(),
                lag_ms: 123,
                save_ok_rate: 0.99,
                match_score: 0.95,
                active_agents: 5,
                timestamp_ms: 1000,
                alerts: vec!["alert1".to_string()],
            };
        }

        let status = service.get_health_status().await;

        assert_eq!(status.status, "ok");
        assert_eq!(status.lag_ms, 123);
        assert_eq!(status.save_ok_rate, 0.99);
        assert_eq!(status.match_score, 0.95);
        assert_eq!(status.active_agents, 5);
        assert_eq!(status.timestamp_ms, 1000);
        assert_eq!(status.alerts, vec!["alert1".to_string()]);
    }
}

impl Default for HealthService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl HealthPort for HealthService {
    async fn get_health_status(&self) -> HealthStatus {
        let result = get_last_sync_result();

        HealthStatus {
            status: result.status,
            lag_ms: result.lag_ms,
            save_ok_rate: result.save_ok_rate,
            match_score: result.match_score,
            active_agents: result.active_agents,
            timestamp_ms: result.timestamp_ms,
            alerts: result.alerts,
        }
    }
}
