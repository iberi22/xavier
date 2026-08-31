//! Fallback health evaluation for Xavier.
//!
//! Provides data structures and helper logic to determine component status when
//! primary backends fail but secondary fallback backends succeed. When a fallback
//! succeeds, status should be "degraded" rather than "unhealthy".

use serde::{Deserialize, Serialize};

/// Report describing the health status of a component using fallback execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FallbackHealthReport {
    /// Whether a fallback backend successfully handled requests/probes.
    pub fallback_success: bool,
    /// Whether the primary backend failed.
    pub primary_failed: bool,
    /// The name of the active backend that responded.
    pub active_backend: String,
    /// Aggregated health status ("healthy", "degraded", "unhealthy", "disabled").
    pub status: String,
}

impl FallbackHealthReport {
    /// Evaluate status based on primary/fallback state.
    pub fn new(primary_healthy: bool, fallback_success: bool, active_backend: impl Into<String>) -> Self {
        let active_backend = active_backend.into();
        let primary_failed = !primary_healthy;
        let status = eval_fallback_status(primary_healthy, fallback_success);
        Self {
            fallback_success,
            primary_failed,
            active_backend,
            status,
        }
    }
}

/// Helper function to compute status string based on primary & fallback success.
/// If primary is healthy -> "healthy"
/// Else if fallback succeeded -> "degraded"
/// Else -> "unhealthy"
pub fn eval_fallback_status(primary_healthy: bool, fallback_success: bool) -> String {
    if primary_healthy {
        "healthy".to_string()
    } else if fallback_success {
        "degraded".to_string()
    } else {
        "unhealthy".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eval_fallback_status_healthy_when_primary_ok() {
        assert_eq!(eval_fallback_status(true, false), "healthy");
        assert_eq!(eval_fallback_status(true, true), "healthy");
    }

    #[test]
    fn test_eval_fallback_status_degraded_when_fallback_succeeds() {
        assert_eq!(eval_fallback_status(false, true), "degraded");
    }

    #[test]
    fn test_eval_fallback_status_unhealthy_when_both_fail() {
        assert_eq!(eval_fallback_status(false, false), "unhealthy");
    }

    #[test]
    fn test_fallback_health_report_creation() {
        let report = FallbackHealthReport::new(false, true, "ollama");
        assert!(report.fallback_success);
        assert!(report.primary_failed);
        assert_eq!(report.active_backend, "ollama");
        assert_eq!(report.status, "degraded");
    }
}
