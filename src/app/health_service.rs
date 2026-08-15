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
    /// New.
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::session_sync_task::{SyncCheckResult, LAST_CHECK_RESULT};

    #[test]
    fn test_health_service_new_and_default() {
        let service1 = HealthService::new();
        let service2 = HealthService::default();

        assert!(service1.start_time.elapsed().as_secs() < 5);
        assert!(service2.start_time.elapsed().as_secs() < 5);
    }

    #[tokio::test]
    async fn test_get_health_status_ok() {
        let service = HealthService::new();

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

    #[tokio::test]
    async fn test_get_health_status_degraded() {
        let service = HealthService::default();

        {
            let mut result = LAST_CHECK_RESULT.write().unwrap();
            *result = SyncCheckResult {
                status: "degraded".to_string(),
                lag_ms: 5000,
                save_ok_rate: 0.50,
                match_score: 0.40,
                active_agents: 1,
                timestamp_ms: 2000,
                alerts: vec!["high_lag".to_string(), "low_match_score".to_string()],
            };
        }

        let status = service.get_health_status().await;

        assert_eq!(status.status, "degraded");
        assert_eq!(status.lag_ms, 5000);
        assert_eq!(status.save_ok_rate, 0.50);
        assert_eq!(status.match_score, 0.40);
        assert_eq!(status.active_agents, 1);
        assert_eq!(status.timestamp_ms, 2000);
        assert_eq!(status.alerts.len(), 2);
        assert_eq!(status.alerts[0], "high_lag");
        assert_eq!(status.alerts[1], "low_match_score");
    }

    #[tokio::test]
    async fn test_get_health_status_empty_alerts() {
        let service = HealthService::new();

        {
            let mut result = LAST_CHECK_RESULT.write().unwrap();
            *result = SyncCheckResult {
                status: "healthy".to_string(),
                lag_ms: 10,
                save_ok_rate: 1.0,
                match_score: 1.0,
                active_agents: 10,
                timestamp_ms: 3000,
                alerts: vec![],
            };
        }

        let status = service.get_health_status().await;

        assert_eq!(status.status, "healthy");
        assert_eq!(status.lag_ms, 10);
        assert_eq!(status.save_ok_rate, 1.0);
        assert_eq!(status.match_score, 1.0);
        assert_eq!(status.active_agents, 10);
        assert_eq!(status.timestamp_ms, 3000);
        assert!(status.alerts.is_empty());
    }
}
