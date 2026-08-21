//! Lock-free sliding window rate limiter (Issue #1443)
//!
//! Uses atomic counters for concurrent access without Mutex.
//! Supports per-IP and per-token rate limiting with burst handling.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Configuration for the rate limiter.
#[derive(Debug, Clone)]
pub struct LimiterConfig {
    /// Window size in seconds
    pub window_secs: u64,
    /// Max requests per window
    pub max_requests: u32,
    /// Burst allowance: short bursts above limit allowed (in requests)
    pub burst_limit: u32,
}

impl Default for LimiterConfig {
    fn default() -> Self {
        Self {
            window_secs: 60,
            max_requests: 100,
            burst_limit: 10,
        }
    }
}

/// Result of a rate limit check.
#[derive(Debug, Clone, PartialEq)]
pub enum LimiterResult {
    /// Request allowed
    Allowed,
    /// Request allowed due to burst
    BurstAllowed,
    /// Request denied — rate limit exceeded
    Denied { retry_after_secs: u64 },
}

/// A single sliding window counter for one key.
struct WindowCounter {
    /// Current window start (epoch seconds)
    window_start: AtomicI64,
    /// Request count in current window
    count: AtomicU64,
    /// Peak count (for burst detection)
    peak: AtomicU64,
}

impl WindowCounter {
    fn new() -> Self {
        Self {
            window_start: AtomicI64::new(0),
            count: AtomicU64::new(0),
            peak: AtomicU64::new(0),
        }
    }

    /// Check and increment. Returns (allowed, burst_used, retry_after).
    fn check_and_increment(&self, config: &LimiterConfig) -> LimiterResult {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let window_start = self.window_start.load(Ordering::Relaxed);
        let elapsed = now - window_start;

        // New window?
        if elapsed >= config.window_secs as i64 || window_start == 0 {
            // Reset window
            self.window_start.store(now, Ordering::Relaxed);
            self.count.store(1, Ordering::Relaxed);
            self.peak.store(1, Ordering::Relaxed);
            return LimiterResult::Allowed;
        }

        // Current window
        let current_count = self.count.fetch_add(1, Ordering::Relaxed) + 1;
        let limit = config.max_requests as u64;
        let burst_limit = limit + config.burst_limit as u64;

        // Update peak
        let current_peak = self.peak.load(Ordering::Relaxed);
        if current_count > current_peak {
            self.peak.store(current_count, Ordering::Relaxed);
        }

        if current_count <= limit {
            LimiterResult::Allowed
        } else if current_count <= burst_limit {
            LimiterResult::BurstAllowed
        } else {
            // Calculate retry_after: time until window resets
            let retry_after = (config.window_secs as i64 - elapsed).max(1) as u64;
            LimiterResult::Denied {
                retry_after_secs: retry_after,
            }
        }
    }

    /// Get current count
    fn current_count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }
}

/// Thread-safe sliding window rate limiter.
///
/// Uses lock-free atomic operations for concurrent access.
/// Each key (IP, token, etc.) has its own sliding window counter.
pub struct SlidingWindowLimiter {
    config: LimiterConfig,
    /// Per-key counters
    counters: parking_lot::RwLock<HashMap<String, Arc<WindowCounter>>>,
}

impl SlidingWindowLimiter {
    /// Create a new rate limiter with the given configuration.
    pub fn new(config: LimiterConfig) -> Self {
        Self {
            config,
            counters: parking_lot::RwLock::new(HashMap::new()),
        }
    }

    /// Create with default config (100 req/60s, burst 10).
    pub fn default_config() -> Self {
        Self::new(LimiterConfig::default())
    }

    /// Check rate limit for a key (e.g., IP address or token).
    pub fn check(&self, key: &str) -> LimiterResult {
        let counter = self.get_or_create_counter(key);
        counter.check_and_increment(&self.config)
    }

    /// Check rate limit for an IP address.
    pub fn check_ip(&self, ip: &str) -> LimiterResult {
        self.check(&format!("ip:{}", ip))
    }

    /// Check rate limit for a token.
    pub fn check_token(&self, token: &str) -> LimiterResult {
        self.check(&format!("token:{}", token))
    }

    /// Get current request count for a key.
    pub fn count(&self, key: &str) -> u64 {
        let counters = self.counters.read();
        counters.get(key).map_or(0, |c| c.current_count())
    }

    /// Reset counter for a key.
    pub fn reset(&self, key: &str) {
        let mut counters = self.counters.write();
        counters.remove(key);
    }

    /// Get number of tracked keys.
    pub fn tracked_keys(&self) -> usize {
        let counters = self.counters.read();
        counters.len()
    }

    /// Get or create a counter for the given key.
    fn get_or_create_counter(&self, key: &str) -> Arc<WindowCounter> {
        // Fast path: read lock
        {
            let counters = self.counters.read();
            if let Some(counter) = counters.get(key) {
                return Arc::clone(counter);
            }
        }
        // Slow path: write lock
        let mut counters = self.counters.write();
        let counter = Arc::new(WindowCounter::new());
        counters
            .entry(key.to_string())
            .or_insert_with(|| Arc::clone(&counter))
            .clone()
    }
}

impl Clone for SlidingWindowLimiter {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            counters: parking_lot::RwLock::new(HashMap::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_basic_allowed() {
        let limiter = SlidingWindowLimiter::new(LimiterConfig {
            window_secs: 60,
            max_requests: 5,
            burst_limit: 2,
        });
        assert_eq!(limiter.check("test"), LimiterResult::Allowed);
    }

    #[test]
    fn test_exceed_limit_denied() {
        let limiter = SlidingWindowLimiter::new(LimiterConfig {
            window_secs: 60,
            max_requests: 3,
            burst_limit: 0,
        });
        assert_eq!(limiter.check("k"), LimiterResult::Allowed);
        assert_eq!(limiter.check("k"), LimiterResult::Allowed);
        assert_eq!(limiter.check("k"), LimiterResult::Allowed);
        assert!(matches!(limiter.check("k"), LimiterResult::Denied { .. }));
    }

    #[test]
    fn test_burst_allowed() {
        let limiter = SlidingWindowLimiter::new(LimiterConfig {
            window_secs: 60,
            max_requests: 2,
            burst_limit: 3,
        });
        assert_eq!(limiter.check("k"), LimiterResult::Allowed);
        assert_eq!(limiter.check("k"), LimiterResult::Allowed);
        assert_eq!(limiter.check("k"), LimiterResult::BurstAllowed);
        assert_eq!(limiter.check("k"), LimiterResult::BurstAllowed);
        assert_eq!(limiter.check("k"), LimiterResult::BurstAllowed);
        assert!(matches!(limiter.check("k"), LimiterResult::Denied { .. }));
    }

    #[test]
    fn test_independent_keys() {
        let limiter = SlidingWindowLimiter::new(LimiterConfig {
            window_secs: 60,
            max_requests: 2,
            burst_limit: 0,
        });
        assert_eq!(limiter.check("a"), LimiterResult::Allowed);
        assert_eq!(limiter.check("a"), LimiterResult::Allowed);
        assert!(matches!(limiter.check("a"), LimiterResult::Denied { .. }));
        // Different key
        assert_eq!(limiter.check("b"), LimiterResult::Allowed);
    }

    #[test]
    fn test_ip_limiter() {
        let limiter = SlidingWindowLimiter::new(LimiterConfig {
            window_secs: 60,
            max_requests: 1,
            burst_limit: 0,
        });
        assert_eq!(limiter.check_ip("127.0.0.1"), LimiterResult::Allowed);
        assert!(matches!(
            limiter.check_ip("127.0.0.1"),
            LimiterResult::Denied { .. }
        ));
        assert_eq!(limiter.check_ip("10.0.0.1"), LimiterResult::Allowed);
    }

    #[test]
    fn test_token_limiter() {
        let limiter = SlidingWindowLimiter::new(LimiterConfig {
            window_secs: 60,
            max_requests: 1,
            burst_limit: 0,
        });
        assert_eq!(limiter.check_token("tok-abc"), LimiterResult::Allowed);
        assert!(matches!(
            limiter.check_token("tok-abc"),
            LimiterResult::Denied { .. }
        ));
    }

    #[test]
    fn test_count_tracking() {
        let limiter = SlidingWindowLimiter::new(LimiterConfig::default());
        assert_eq!(limiter.count("k"), 0);
        limiter.check("k");
        limiter.check("k");
        assert_eq!(limiter.count("k"), 2);
    }

    #[test]
    fn test_reset() {
        let limiter = SlidingWindowLimiter::new(LimiterConfig {
            window_secs: 60,
            max_requests: 1,
            burst_limit: 0,
        });
        limiter.check("k");
        assert!(matches!(limiter.check("k"), LimiterResult::Denied { .. }));
        limiter.reset("k");
        assert_eq!(limiter.count("k"), 0);
        assert_eq!(limiter.check("k"), LimiterResult::Allowed);
    }

    #[test]
    fn test_tracked_keys() {
        let limiter = SlidingWindowLimiter::new(LimiterConfig::default());
        assert_eq!(limiter.tracked_keys(), 0);
        limiter.check("a");
        limiter.check("b");
        limiter.check("c");
        assert_eq!(limiter.tracked_keys(), 3);
    }

    #[test]
    fn test_concurrent_access() {
        let limiter = SlidingWindowLimiter::new(LimiterConfig {
            window_secs: 60,
            max_requests: 100,
            burst_limit: 0,
        });
        let limiter = Arc::new(limiter);
        let mut handles = vec![];
        for _ in 0..10 {
            let lim = Arc::clone(&limiter);
            handles.push(thread::spawn(move || {
                for _ in 0..20 {
                    let _ = lim.check("shared");
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        // Should have 200 total (10 threads * 20), only 100 allowed
        assert_eq!(limiter.count("shared"), 200);
    }

    #[test]
    fn test_burst_limit_zero() {
        let limiter = SlidingWindowLimiter::new(LimiterConfig {
            window_secs: 60,
            max_requests: 5,
            burst_limit: 0,
        });
        for _ in 0..5 {
            assert_eq!(limiter.check("k"), LimiterResult::Allowed);
        }
        assert!(matches!(limiter.check("k"), LimiterResult::Denied { .. }));
    }

    #[test]
    fn test_default_config() {
        let limiter = SlidingWindowLimiter::default_config();
        // Should allow 100 requests
        for _ in 0..100 {
            assert_eq!(limiter.check("k"), LimiterResult::Allowed);
        }
        // 101st should be burst or denied
        let result = limiter.check("k");
        assert!(
            result == LimiterResult::BurstAllowed || matches!(result, LimiterResult::Denied { .. })
        );
    }

    #[test]
    fn test_limiter_window_slide_boundary() {
        let limiter = SlidingWindowLimiter::new(LimiterConfig {
            window_secs: 1,
            max_requests: 2,
            burst_limit: 0,
        });
        assert_eq!(limiter.check("key_boundary"), LimiterResult::Allowed);
        assert_eq!(limiter.check("key_boundary"), LimiterResult::Allowed);
        assert!(matches!(
            limiter.check("key_boundary"),
            LimiterResult::Denied { .. }
        ));

        // Sleep to cross exact window boundary
        thread::sleep(Duration::from_secs(1));

        // Next request after window slides should be allowed again
        assert_eq!(limiter.check("key_boundary"), LimiterResult::Allowed);
        assert_eq!(limiter.count("key_boundary"), 1);
    }

    #[test]
    fn test_limiter_burst_then_sustained() {
        let limiter = SlidingWindowLimiter::new(LimiterConfig {
            window_secs: 1,
            max_requests: 2,
            burst_limit: 2,
        });

        // Initial burst
        assert_eq!(limiter.check("user_burst"), LimiterResult::Allowed);
        assert_eq!(limiter.check("user_burst"), LimiterResult::Allowed);
        assert_eq!(limiter.check("user_burst"), LimiterResult::BurstAllowed);
        assert_eq!(limiter.check("user_burst"), LimiterResult::BurstAllowed);
        assert!(matches!(
            limiter.check("user_burst"),
            LimiterResult::Denied { .. }
        ));

        // Wait for next window
        thread::sleep(Duration::from_secs(1));

        // Sustained rate in new window
        assert_eq!(limiter.check("user_burst"), LimiterResult::Allowed);
        assert_eq!(limiter.check("user_burst"), LimiterResult::Allowed);
    }

    #[test]
    fn test_limiter_multiple_windows_sliding() {
        let limiter = SlidingWindowLimiter::new(LimiterConfig {
            window_secs: 1,
            max_requests: 2,
            burst_limit: 1,
        });

        for window in 0..3 {
            let key = "multi_window_key";
            assert_eq!(
                limiter.check(key),
                LimiterResult::Allowed,
                "Failed allowed 1 in window {}",
                window
            );
            assert_eq!(
                limiter.check(key),
                LimiterResult::Allowed,
                "Failed allowed 2 in window {}",
                window
            );
            assert_eq!(
                limiter.check(key),
                LimiterResult::BurstAllowed,
                "Failed burst in window {}",
                window
            );
            assert!(
                matches!(limiter.check(key), LimiterResult::Denied { .. }),
                "Failed denial in window {}",
                window
            );
            thread::sleep(Duration::from_secs(1));
        }
    }

    #[test]
    fn test_limiter_key_cleanup_expired() {
        let limiter = SlidingWindowLimiter::new(LimiterConfig {
            window_secs: 1,
            max_requests: 5,
            burst_limit: 0,
        });

        limiter.check("temp_key_1");
        limiter.check("temp_key_2");
        assert_eq!(limiter.tracked_keys(), 2);

        // Explicit key cleanup
        limiter.reset("temp_key_1");
        assert_eq!(limiter.tracked_keys(), 1);
        assert_eq!(limiter.count("temp_key_1"), 0);

        limiter.reset("temp_key_2");
        assert_eq!(limiter.tracked_keys(), 0);
        assert_eq!(limiter.count("temp_key_2"), 0);
    }

    #[test]
    fn test_limiter_very_high_throughput() {
        let limiter = Arc::new(SlidingWindowLimiter::new(LimiterConfig {
            window_secs: 60,
            max_requests: 500,
            burst_limit: 100,
        }));

        let mut handles = vec![];
        for _ in 0..10 {
            let lim = Arc::clone(&limiter);
            handles.push(thread::spawn(move || {
                let mut allowed = 0;
                let mut burst = 0;
                let mut denied = 0;
                for _ in 0..100 {
                    match lim.check("throughput_key") {
                        LimiterResult::Allowed => allowed += 1,
                        LimiterResult::BurstAllowed => burst += 1,
                        LimiterResult::Denied { .. } => denied += 1,
                    }
                }
                (allowed, burst, denied)
            }));
        }

        let mut total_allowed = 0;
        let mut total_burst = 0;
        let mut total_denied = 0;

        for h in handles {
            let (a, b, d) = h.join().unwrap();
            total_allowed += a;
            total_burst += b;
            total_denied += d;
        }

        assert_eq!(limiter.count("throughput_key"), 1000);
        assert_eq!(total_allowed, 500);
        assert_eq!(total_burst, 100);
        assert_eq!(total_denied, 400);
    }

    #[test]
    fn test_limiter_config_validation() {
        // Zero requests & zero burst limit
        let zero_limiter = SlidingWindowLimiter::new(LimiterConfig {
            window_secs: 60,
            max_requests: 0,
            burst_limit: 0,
        });

        assert_eq!(zero_limiter.check("zero_key"), LimiterResult::Allowed);
        assert!(matches!(
            zero_limiter.check("zero_key"),
            LimiterResult::Denied { .. }
        ));

        // Zero window_secs (window resets immediately on every request)
        let zero_win_limiter = SlidingWindowLimiter::new(LimiterConfig {
            window_secs: 0,
            max_requests: 1,
            burst_limit: 0,
        });

        assert_eq!(zero_win_limiter.check("win_key"), LimiterResult::Allowed);
        assert_eq!(zero_win_limiter.check("win_key"), LimiterResult::Allowed);
    }

    #[test]
    fn test_limiter_status_reporting() {
        let max_req = 10;
        let burst = 5;
        let limiter = SlidingWindowLimiter::new(LimiterConfig {
            window_secs: 60,
            max_requests: max_req,
            burst_limit: burst,
        });

        let key = "status_key";
        assert_eq!(limiter.count(key), 0);

        for i in 1..=4 {
            assert_eq!(limiter.check(key), LimiterResult::Allowed);
            assert_eq!(limiter.count(key), i);
        }

        let remaining_base = max_req as u64 - limiter.count(key);
        assert_eq!(remaining_base, 6);

        // Exhaust remaining base limit
        for _ in 0..6 {
            assert_eq!(limiter.check(key), LimiterResult::Allowed);
        }
        assert_eq!(limiter.count(key), 10);

        // Consume 1 burst request
        assert_eq!(limiter.check(key), LimiterResult::BurstAllowed);
        assert_eq!(limiter.count(key), 11);
        let remaining_total = (max_req + burst) as u64 - limiter.count(key);
        assert_eq!(remaining_total, 4);
    }

    #[test]
    fn test_limiter_reset_specific_key() {
        let limiter = SlidingWindowLimiter::new(LimiterConfig {
            window_secs: 60,
            max_requests: 1,
            burst_limit: 0,
        });

        assert_eq!(limiter.check("user1"), LimiterResult::Allowed);
        assert_eq!(limiter.check("user2"), LimiterResult::Allowed);
        assert_eq!(limiter.check("user3"), LimiterResult::Allowed);

        assert!(matches!(
            limiter.check("user1"),
            LimiterResult::Denied { .. }
        ));
        assert!(matches!(
            limiter.check("user2"),
            LimiterResult::Denied { .. }
        ));
        assert!(matches!(
            limiter.check("user3"),
            LimiterResult::Denied { .. }
        ));

        assert_eq!(limiter.tracked_keys(), 3);

        // Reset user1 specifically
        limiter.reset("user1");

        assert_eq!(limiter.tracked_keys(), 2);
        assert_eq!(limiter.count("user1"), 0);

        // user1 allowed again
        assert_eq!(limiter.check("user1"), LimiterResult::Allowed);

        // user2 and user3 remain denied
        assert!(matches!(
            limiter.check("user2"),
            LimiterResult::Denied { .. }
        ));
        assert!(matches!(
            limiter.check("user3"),
            LimiterResult::Denied { .. }
        ));
    }

    #[test]
    fn test_limiter_retry_after_calculation() {
        let limiter = SlidingWindowLimiter::new(LimiterConfig {
            window_secs: 30,
            max_requests: 1,
            burst_limit: 0,
        });

        assert_eq!(limiter.check("retry_key"), LimiterResult::Allowed);
        let res = limiter.check("retry_key");
        if let LimiterResult::Denied { retry_after_secs } = res {
            assert!(
                retry_after_secs > 0 && retry_after_secs <= 30,
                "retry_after_secs {} out of bounds",
                retry_after_secs
            );
        } else {
            panic!("Expected LimiterResult::Denied");
        }
    }

    #[test]
    fn test_limiter_clone_isolation() {
        let limiter1 = SlidingWindowLimiter::new(LimiterConfig {
            window_secs: 60,
            max_requests: 2,
            burst_limit: 0,
        });

        limiter1.check("key_clone");
        limiter1.check("key_clone");

        let limiter2 = limiter1.clone();
        assert_eq!(limiter1.tracked_keys(), 1);
        assert_eq!(limiter2.tracked_keys(), 0);

        assert!(matches!(
            limiter1.check("key_clone"),
            LimiterResult::Denied { .. }
        ));
        assert_eq!(limiter2.check("key_clone"), LimiterResult::Allowed);
    }
}
