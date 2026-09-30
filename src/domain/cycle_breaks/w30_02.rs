//! Owned by issue [WAVE-30.02] (ADR-033 Wave 0).
//!
//! Material living here to cut the module cycles that ran from the inbound HTTP
//! adapters back into the rest of the crate: `adapters -> time` (the
//! `TimeMetricSink` capability below) and `adapters -> tasks` (the moved sync-check
//! result and its cache). The third cut, `adapters -> middleware`, needed no domain
//! material: the RBAC and rate-limit layers moved next to the router that mounts
//! them, in `adapters::inbound::http::middleware`.
//!
//! Every moved item keeps a `pub use` re-export at its original path, so no caller
//! and no behaviour changes.

use std::sync::{LazyLock, RwLock as StdRwLock};

use async_trait::async_trait;

use crate::domain::memory::TimeMetric;

// ─── Cycle cut `adapters -> time` ───────────────────────────────────────────

/// Persistence capability the inbound time-metrics adapter depends on.
///
/// The contract lives in the domain and is implemented by the store that owns the
/// persistence, so the adapter no longer has to import that store.
#[async_trait]
pub trait TimeMetricSink: Send + Sync {
    /// Persist one metric of `workspace_id`.
    async fn persist_time_metric(
        &self,
        metric: &TimeMetric,
        workspace_id: &str,
    ) -> Result<(), String>;
}

// ─── Cycle cut `adapters -> tasks` ──────────────────────────────────────────

/// Sync check result (moved here from the session-sync task module).
#[derive(Debug, Clone)]
pub struct SyncCheckResult {
    pub status: String,
    pub lag_ms: u64,
    pub save_ok_rate: f64,
    pub match_score: f64,
    pub active_agents: u64,
    pub timestamp_ms: u64,
    pub alerts: Vec<String>,
}

impl Default for SyncCheckResult {
    fn default() -> Self {
        Self {
            status: "unknown".to_string(),
            lag_ms: 0,
            save_ok_rate: 1.0,
            match_score: 1.0,
            active_agents: 0,
            timestamp_ms: 0,
            alerts: Vec::new(),
        }
    }
}

/// Last sync check result stored in memory (static).
/// Unified last sync check result (single lock — no data race).
pub(crate) static LAST_CHECK_RESULT: LazyLock<StdRwLock<SyncCheckResult>> =
    LazyLock::new(|| StdRwLock::new(SyncCheckResult::default()));

/// Get last sync check result (for REST endpoint) — consistent snapshot via unified lock.
pub fn get_last_sync_result() -> SyncCheckResult {
    LAST_CHECK_RESULT
        .read()
        .map(|r| r.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unwritten cache still reports the documented default.
    #[test]
    fn default_result_is_the_documented_unknown() {
        let default = SyncCheckResult::default();
        assert_eq!(default.status, "unknown");
        assert_eq!(default.save_ok_rate, 1.0);
        assert_eq!(default.match_score, 1.0);
        assert!(default.alerts.is_empty());
    }
}
