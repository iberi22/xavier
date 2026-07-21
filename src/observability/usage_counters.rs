// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! In-process LLM proxy usage counters (Ola 3 / issue #578).
//!
//! Complements `RateLimitManager` persistence with a cheap, process-local
//! snapshot for `/v1/account/usage` and `/v1/usage`.

use parking_lot::RwLock;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

/// Per-provider aggregated usage.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct ProviderUsage {
    pub requests: u64,
    pub tokens: u64,
    pub errors: u64,
    pub cost_usd: f64,
}

/// Point-in-time snapshot of proxy usage (cheap clone).
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct UsageSnapshot {
    pub by_provider: HashMap<String, ProviderUsage>,
    pub memory_fallback_hits: u64,
    pub fallback_chain_hops: u64,
    pub total_requests: u64,
    pub total_tokens: u64,
    pub total_errors: u64,
    pub total_cost_usd: f64,
}

/// Thread-safe shared counters for the proxy path.
#[derive(Debug, Default)]
pub struct UsageCounters {
    inner: RwLock<UsageSnapshot>,
}

impl UsageCounters {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(UsageSnapshot::default()),
        }
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    pub fn record_success(&self, provider: &str, tokens: u64, cost_usd: f64) {
        let mut snap = self.inner.write();
        {
            let entry = snap.by_provider.entry(provider.to_string()).or_default();
            entry.requests = entry.requests.saturating_add(1);
            entry.tokens = entry.tokens.saturating_add(tokens);
            entry.cost_usd += cost_usd;
        }
        snap.total_requests = snap.total_requests.saturating_add(1);
        snap.total_tokens = snap.total_tokens.saturating_add(tokens);
        snap.total_cost_usd += cost_usd;
    }

    pub fn record_error(&self, provider: &str) {
        let mut snap = self.inner.write();
        {
            let entry = snap.by_provider.entry(provider.to_string()).or_default();
            entry.errors = entry.errors.saturating_add(1);
            // Count failed attempts as requests for visibility.
            entry.requests = entry.requests.saturating_add(1);
        }
        snap.total_errors = snap.total_errors.saturating_add(1);
        snap.total_requests = snap.total_requests.saturating_add(1);
    }

    pub fn record_fallback_hop(&self) {
        let mut snap = self.inner.write();
        snap.fallback_chain_hops = snap.fallback_chain_hops.saturating_add(1);
    }

    pub fn record_memory_fallback(&self) {
        let mut snap = self.inner.write();
        snap.memory_fallback_hits = snap.memory_fallback_hits.saturating_add(1);
    }

    pub fn snapshot(&self) -> UsageSnapshot {
        self.inner.read().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_counters_success_error_fallback() {
        let counters = UsageCounters::new();

        counters.record_success("local", 100, 0.0);
        counters.record_success("local", 50, 0.0);
        counters.record_success("openai", 200, 0.01);
        counters.record_error("local");
        counters.record_error("groq");
        counters.record_fallback_hop();
        counters.record_memory_fallback();

        let snap = counters.snapshot();

        assert_eq!(snap.total_requests, 5); // 3 success + 2 error
        assert_eq!(snap.total_tokens, 350);
        assert_eq!(snap.total_errors, 2);
        assert!((snap.total_cost_usd - 0.01).abs() < f64::EPSILON);
        assert_eq!(snap.fallback_chain_hops, 1);
        assert_eq!(snap.memory_fallback_hits, 1);

        let local = snap.by_provider.get("local").expect("local stats");
        assert_eq!(local.requests, 3); // 2 success + 1 error
        assert_eq!(local.tokens, 150);
        assert_eq!(local.errors, 1);

        let openai = snap.by_provider.get("openai").expect("openai stats");
        assert_eq!(openai.requests, 1);
        assert_eq!(openai.tokens, 200);
        assert_eq!(openai.errors, 0);

        let groq = snap.by_provider.get("groq").expect("groq stats");
        assert_eq!(groq.requests, 1);
        assert_eq!(groq.errors, 1);
    }

    #[test]
    fn test_usage_counters_snapshot_is_clone() {
        let counters = UsageCounters::new();
        counters.record_success("local", 10, 0.0);
        let a = counters.snapshot();
        counters.record_success("local", 5, 0.0);
        let b = counters.snapshot();
        assert_eq!(a.total_tokens, 10);
        assert_eq!(b.total_tokens, 15);
    }
}
