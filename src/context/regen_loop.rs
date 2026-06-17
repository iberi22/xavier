//! Context Regeneration Loop
//!
//! Auto-triggers context rebuilds based on session staleness detection:
//! - Time elapsed since last context rebuild
//! - Session growth (new messages / tokens added)
//! - Threshold-based triggers that invoke the orchestrator's precompact
//!
//! # Architecture
//!
//! ```text
//! New message arrives
//!   ↓
//! RegenerationLoop::check(session_id, new_tokens)
//!   ↓
//! Compute elapsed time + growth ratio
//!   ↓
//! Exceeds threshold?
//!   ├── Yes → trigger_precompact() → rebuild context
//!   └── No  → update session stats, return Ok(false)
//! ```

use crate::context::orchestrator::Orchestrator;
use crate::context::ContextDocument;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// Configuration for the regeneration loop thresholds
#[derive(Debug, Clone, Copy)]
pub struct RegenerationConfig {
    /// If this many seconds pass without a rebuild, trigger one
    pub stale_after_secs: u64,
    /// If session tokens grew by this ratio (0.0-1.0) since last rebuild, trigger
    pub growth_ratio_threshold: f64,
    /// Minimum tokens that must have been added to trigger (avoid tiny flurries)
    pub min_growth_tokens: usize,
    /// Cooldown: don't rebuild more frequently than this (seconds)
    pub cooldown_secs: u64,
    /// Maximum rebuilds allowed within cooldown window (rate limiting)
    pub max_rebuilds_per_window: u32,
}

impl Default for RegenerationConfig {
    fn default() -> Self {
        Self {
            stale_after_secs: 600,          // 10 minutes
            growth_ratio_threshold: 0.25,    // 25% growth triggers rebuild
            min_growth_tokens: 100,          // Ignore <100 token additions
            cooldown_secs: 120,              // Don't rebuild more than every 2 min
            max_rebuilds_per_window: 10,     // Max 10 rebuilds per session
        }
    }
}

/// Per-session statistics tracked for regeneration decisions
#[derive(Debug, Clone)]
pub struct SessionRegenStats {
    /// Tokens at last context rebuild
    pub tokens_at_last_rebuild: usize,
    /// When the last rebuild happened
    pub last_rebuild_at: Instant,
    /// Total tokens added to this session
    pub total_tokens_seen: usize,
    /// How many rebuilds this session has had
    pub rebuild_count: u32,
    /// When this session was first seen
    pub created_at: Instant,
}

impl Default for SessionRegenStats {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            tokens_at_last_rebuild: 0,
            last_rebuild_at: now,
            total_tokens_seen: 0,
            rebuild_count: 0,
            created_at: now,
        }
    }
}

/// Result of a regeneration check
#[derive(Debug, Clone, PartialEq)]
pub enum RegenDecision {
    /// No rebuild needed
    Skip,
    /// Rebuild triggered by staleness (time elapsed)
    Stale {
        seconds_since_rebuild: u64,
    },
    /// Rebuild triggered by session growth
    Growth {
        growth_ratio: f64,
        tokens_added: usize,
    },
    /// Blocked by rate limit (too many rebuilds)
    RateLimited {
        reason: String,
    },
}

/// The context regeneration loop — monitors sessions and auto-triggers rebuilds
pub struct RegenerationLoop {
    /// Per-session regeneration statistics
    sessions: Arc<Mutex<HashMap<String, SessionRegenStats>>>,
    /// Configuration
    config: RegenerationConfig,
    /// Reference to orchestrator (used to trigger precompact)
    orchestrator: Option<Arc<Orchestrator>>,
    /// Current rebuild window counter (resets after cooldown)
    last_global_rebuild: Arc<Mutex<Instant>>,
}

impl RegenerationLoop {
    /// Create a new regeneration loop with default config
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            config: RegenerationConfig::default(),
            orchestrator: None,
            last_global_rebuild: Arc::new(Mutex::new(Instant::now())),
        }
    }

    /// Create with custom config
    pub fn with_config(config: RegenerationConfig) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            config,
            orchestrator: None,
            last_global_rebuild: Arc::new(Mutex::new(Instant::now())),
        }
    }

    /// Wire up the orchestrator
    pub fn with_orchestrator(mut self, orchestrator: Arc<Orchestrator>) -> Self {
        self.orchestrator = Some(orchestrator);
        self
    }

    /// Check if a session needs context regeneration
    ///
    /// Returns `Ok(RegenDecision::Skip)` if no rebuild needed,
    /// or the reason for triggering a rebuild.
    pub async fn check(
        &self,
        session_id: &str,
        new_tokens_added: usize,
    ) -> RegenDecision {
        let mut sessions = self.sessions.lock().await;
        let stats = sessions.entry(session_id.to_string())
            .or_insert_with(SessionRegenStats::default);

        // Update token count
        stats.total_tokens_seen += new_tokens_added;

        // Time since last rebuild
        let elapsed = stats.last_rebuild_at.elapsed();

        // Check rate limit
        if stats.rebuild_count >= self.config.max_rebuilds_per_window {
            return RegenDecision::RateLimited {
                reason: format!(
                    "Session '{}' has already been rebuilt {} times (max {})",
                    session_id,
                    stats.rebuild_count,
                    self.config.max_rebuilds_per_window,
                ),
            };
        }

        // Cooldown check
        if elapsed < Duration::from_secs(self.config.cooldown_secs) {
            return RegenDecision::Skip;
        }

        // 1. Staleness trigger
        if elapsed > Duration::from_secs(self.config.stale_after_secs)
            && stats.rebuild_count > 0
        {
            let secs = elapsed.as_secs();
            debug!(
                session_id = %session_id,
                seconds_since_rebuild = secs,
                "Session stale — triggering context regeneration"
            );
            return RegenDecision::Stale {
                seconds_since_rebuild: secs,
            };
        }

        // 2. Growth trigger (only if we have a baseline to compare against)
        if stats.tokens_at_last_rebuild > 0 {
            let tokens_added = stats.total_tokens_seen.saturating_sub(stats.tokens_at_last_rebuild);
            if tokens_added >= self.config.min_growth_tokens {
                let growth_ratio = tokens_added as f64 / stats.tokens_at_last_rebuild as f64;
                if growth_ratio >= self.config.growth_ratio_threshold {
                    debug!(
                        session_id = %session_id,
                        growth_ratio = %growth_ratio,
                        tokens_added = tokens_added,
                        "Session growth exceeded threshold — triggering context regeneration"
                    );
                    return RegenDecision::Growth {
                        growth_ratio,
                        tokens_added,
                    };
                }
            }
        }

        RegenDecision::Skip
    }

    /// Trigger a context rebuild for a session
    ///
    /// Returns the number of documents selected, or an error string.
    pub async fn trigger_rebuild(
        &self,
        session_id: &str,
        current_context: &[ContextDocument],
    ) -> Result<usize, String> {
        let orchestrator = self.orchestrator.as_ref()
            .ok_or_else(|| "RegenerationLoop: orchestrator not configured".to_string())?;

        // Use precompact hook — it has the largest budget
        let plan = orchestrator
            .precompact(session_id, "regenerate context", current_context)
            .await;

        let selected = orchestrator
            .execute(&plan, current_context, session_id)
            .await;

        let count = selected.len();

        // Update session stats
        {
            let mut sessions = self.sessions.lock().await;
            if let Some(stats) = sessions.get_mut(session_id) {
                stats.tokens_at_last_rebuild = stats.total_tokens_seen;
                stats.last_rebuild_at = Instant::now();
                stats.rebuild_count += 1;
            }
        }

        // Update global rebuild timestamp
        {
            let mut global = self.last_global_rebuild.lock().await;
            *global = Instant::now();
        }

        info!(
            session_id = %session_id,
            documents_selected = count,
            "Context regeneration completed"
        );

        Ok(count)
    }

    /// Run a regeneration cycle: check + trigger if needed
    ///
    /// Convenience method combining `check()` + `trigger_rebuild()`.
    /// Returns `(decision, selected_count)`.
    pub async fn cycle(
        &self,
        session_id: &str,
        new_tokens: usize,
        current_context: &[ContextDocument],
    ) -> (RegenDecision, Option<usize>) {
        let decision = self.check(session_id, new_tokens).await;
        match &decision {
            RegenDecision::Stale { .. } | RegenDecision::Growth { .. } => {
                match self.trigger_rebuild(session_id, current_context).await {
                    Ok(count) => (decision, Some(count)),
                    Err(e) => {
                        warn!("Context regeneration failed: {}", e);
                        (RegenDecision::RateLimited { reason: e }, None)
                    }
                }
            }
            _ => (decision, None),
        }
    }

    /// Get stats for a specific session
    pub async fn get_stats(&self, session_id: &str) -> Option<SessionRegenStats> {
        let sessions = self.sessions.lock().await;
        sessions.get(session_id).cloned()
    }

    /// Reset stats for a session (e.g., after manual rebuild)
    pub async fn reset_session(&self, session_id: &str) {
        let mut sessions = self.sessions.lock().await;
        sessions.remove(session_id);
    }

    /// Get total sessions tracked
    pub async fn session_count(&self) -> usize {
        let sessions = self.sessions.lock().await;
        sessions.len()
    }
}

impl Default for RegenerationLoop {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_skip_when_fresh() {
        let loop_ = RegenerationLoop::new();
        let decision = loop_.check("test-session", 50).await;
        assert_eq!(decision, RegenDecision::Skip);
    }

    #[tokio::test]
    async fn test_skip_below_min_growth() {
        let loop_ = RegenerationLoop::with_config(RegenerationConfig {
            stale_after_secs: 600,
            growth_ratio_threshold: 0.25,
            min_growth_tokens: 100,
            cooldown_secs: 1,
            max_rebuilds_per_window: 10,
        });

        // First, need a baseline by manually setting tokens_at_last_rebuild
        {
            let mut sessions = loop_.sessions.lock().await;
            let stats = sessions.entry("test-session".to_string())
                .or_insert_with(SessionRegenStats::default);
            stats.tokens_at_last_rebuild = 1000;
            stats.total_tokens_seen = 1050; // +50 tokens, threshold is 100
        }

        let decision = loop_.check("test-session", 0).await;
        assert_eq!(decision, RegenDecision::Skip);
    }

    #[tokio::test]
    async fn test_trigger_on_growth() {
        let loop_ = RegenerationLoop::with_config(RegenerationConfig {
            stale_after_secs: 600,
            growth_ratio_threshold: 0.20, // 20% growth
            min_growth_tokens: 50,
            cooldown_secs: 1,
            max_rebuilds_per_window: 10,
        });

        // Set baseline: 1000 tokens, then add 300 (30% growth > 20% threshold)
        {
            let mut sessions = loop_.sessions.lock().await;
            let stats = sessions.entry("test-session".to_string())
                .or_insert_with(SessionRegenStats::default);
            stats.tokens_at_last_rebuild = 1000;
            stats.total_tokens_seen = 1300;
            stats.last_rebuild_at = Instant::now() - Duration::from_secs(120);
        }

        let decision = loop_.check("test-session", 0).await;
        match decision {
            RegenDecision::Growth { growth_ratio, .. } => {
                assert!((growth_ratio - 0.3).abs() < 0.01);
            }
            other => panic!("Expected Growth decision, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_rate_limit() {
        let loop_ = RegenerationLoop::with_config(RegenerationConfig {
            ..Default::default()
        });

        // Set session with max rebuilds already hit
        {
            let mut sessions = loop_.sessions.lock().await;
            let stats = sessions.entry("test-session".to_string())
                .or_insert_with(SessionRegenStats::default);
            stats.rebuild_count = 10; // max_rebuilds_per_window is 10
        }

        let decision = loop_.check("test-session", 500).await;
        match decision {
            RegenDecision::RateLimited { .. } => {} // Expected
            other => panic!("Expected RateLimited, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_new_session_does_not_trigger_stale() {
        let loop_ = RegenerationLoop::with_config(RegenerationConfig {
            stale_after_secs: 1,
            ..Default::default()
        });

        // A new session has rebuild_count = 0, so staleness check is skipped
        tokio::time::sleep(Duration::from_millis(100)).await;

        let decision = loop_.check("new-session", 10).await;
        // New sessions shouldn't trigger staleness (no baseline yet)
        assert_eq!(decision, RegenDecision::Skip);
    }

    #[tokio::test]
    async fn test_cycle_no_orchestrator() {
        let loop_ = RegenerationLoop::new();
        let (decision, selected) = loop_.cycle("s1", 50, &[]).await;
        assert_eq!(decision, RegenDecision::Skip);
        assert!(selected.is_none());
    }

    #[tokio::test]
    async fn test_session_count() {
        let loop_ = RegenerationLoop::new();
        assert_eq!(loop_.session_count().await, 0);

        loop_.check("s1", 50).await;
        assert_eq!(loop_.session_count().await, 1);

        loop_.check("s2", 50).await;
        assert_eq!(loop_.session_count().await, 2);
    }

    #[tokio::test]
    async fn test_reset_session() {
        let loop_ = RegenerationLoop::new();
        loop_.check("s1", 50).await;
        assert_eq!(loop_.session_count().await, 1);

        loop_.reset_session("s1").await;
        assert_eq!(loop_.session_count().await, 0);
    }
}
