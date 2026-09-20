//! High-performance in-memory sliding-window rate limiter per node/wallet (#2397 / WAVE-27.19)

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Rate limit decision outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitDecision {
    pub allowed: bool,
    pub remaining_requests: usize,
    pub reset_after_ms: u64,
}

/// Sliding window rate limiter tracking message bursts per peer/wallet identity.
#[derive(Clone)]
pub struct TelecomRateLimiter {
    max_requests_per_window: usize,
    window_duration: Duration,
    records: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
}

impl TelecomRateLimiter {
    /// Creates a new rate limiter with specified quota per time window.
    pub fn new(max_requests_per_window: usize, window_duration: Duration) -> Self {
        Self {
            max_requests_per_window,
            window_duration,
            records: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Checks if a request from `node_or_wallet_id` is within quota and records it if allowed.
    pub async fn check_rate_limit(&self, node_or_wallet_id: &str) -> RateLimitDecision {
        let mut map = self.records.lock().await;
        let now = Instant::now();
        let cutoff = now.checked_sub(self.window_duration).unwrap_or(now);

        let timestamps = map.entry(node_or_wallet_id.to_string()).or_default();
        timestamps.retain(|&t| t > cutoff);

        if timestamps.len() < self.max_requests_per_window {
            timestamps.push(now);
            RateLimitDecision {
                allowed: true,
                remaining_requests: self.max_requests_per_window - timestamps.len(),
                reset_after_ms: self.window_duration.as_millis() as u64,
            }
        } else {
            let earliest = timestamps.first().copied().unwrap_or(now);
            let elapsed = now.duration_since(earliest);
            let remaining = self.window_duration.saturating_sub(elapsed);
            RateLimitDecision {
                allowed: false,
                remaining_requests: 0,
                reset_after_ms: remaining.as_millis() as u64,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_allows_and_throttles() {
        let limiter = TelecomRateLimiter::new(3, Duration::from_millis(100));

        let res1 = limiter.check_rate_limit("wallet_1").await;
        assert!(res1.allowed);
        assert_eq!(res1.remaining_requests, 2);

        let res2 = limiter.check_rate_limit("wallet_1").await;
        assert!(res2.allowed);

        let res3 = limiter.check_rate_limit("wallet_1").await;
        assert!(res3.allowed);

        // 4th request must be rejected
        let res4 = limiter.check_rate_limit("wallet_1").await;
        assert!(!res4.allowed);
        assert_eq!(res4.remaining_requests, 0);

        // Different wallet is independent
        let res_other = limiter.check_rate_limit("wallet_2").await;
        assert!(res_other.allowed);

        // Wait for sliding window to expire
        tokio::time::sleep(Duration::from_millis(120)).await;
        let res_after = limiter.check_rate_limit("wallet_1").await;
        assert!(res_after.allowed);
    }
}
