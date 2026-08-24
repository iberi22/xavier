//! Lock-Free High-Concurrency Sliding Window Rate Limiter
//!
//! Provides a lock-free, atomic sliding window rate limiter designed for high-concurrency
//! agentic workloads without acquiring global Mutex locks during the hot path.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use parking_lot::RwLock;

/// Configuration for the LockFreeSlidingLimiter
#[derive(Debug, Clone)]
pub struct SlidingLimiterConfig {
    /// Maximum number of requests allowed within the window
    pub max_requests: u64,
    /// Duration of the sliding window
    pub window_duration: Duration,
    /// Number of sub-window buckets for granular sliding calculation
    pub num_buckets: usize,
}

impl Default for SlidingLimiterConfig {
    fn default() -> Self {
        Self {
            max_requests: 100,
            window_duration: Duration::from_secs(60),
            num_buckets: 60,
        }
    }
}

/// A bucket in the circular ring buffer holding request count and timestamp
#[derive(Debug)]
struct AtomicBucket {
    timestamp: AtomicU64,
    count: AtomicU64,
}

impl AtomicBucket {
    fn new() -> Self {
        Self {
            timestamp: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }
}

/// State for a single tracked client/key
#[derive(Debug)]
pub struct ClientRateState {
    buckets: Vec<AtomicBucket>,
    bucket_duration_ms: u64,
    window_duration_ms: u64,
}

impl ClientRateState {
    fn new(config: &SlidingLimiterConfig) -> Self {
        let num_buckets = config.num_buckets.max(1);
        let mut buckets = Vec::with_capacity(num_buckets);
        for _ in 0..num_buckets {
            buckets.push(AtomicBucket::new());
        }
        let window_duration_ms = config.window_duration.as_millis() as u64;
        let bucket_duration_ms = (window_duration_ms / num_buckets as u64).max(1);

        Self {
            buckets,
            bucket_duration_ms,
            window_duration_ms,
        }
    }

    /// Attempts to record a request. Returns true if allowed, false if limit exceeded.
    pub fn check_and_record(&self, max_requests: u64, now_ms: u64) -> bool {
        let bucket_idx = ((now_ms / self.bucket_duration_ms) % self.buckets.len() as u64) as usize;
        let current_bucket_time = (now_ms / self.bucket_duration_ms) * self.bucket_duration_ms;
        let window_start_ms = now_ms.saturating_sub(self.window_duration_ms);

        // Sum current requests across active buckets in window
        let mut total_requests: u64 = 0;
        for b in &self.buckets {
            let b_time = b.timestamp.load(Ordering::Acquire);
            if b_time >= window_start_ms && b_time <= now_ms {
                total_requests += b.count.load(Ordering::Acquire);
            }
        }

        if total_requests >= max_requests {
            return false;
        }

        // Increment or reset the current bucket
        let target_bucket = &self.buckets[bucket_idx];
        let prev_time = target_bucket.timestamp.load(Ordering::Acquire);

        if prev_time != current_bucket_time {
            // Attempt to update the bucket timestamp atomically
            if target_bucket.timestamp.compare_exchange(
                prev_time,
                current_bucket_time,
                Ordering::AcqRel,
                Ordering::Acquire,
            ).is_ok() {
                target_bucket.count.store(1, Ordering::Release);
                return true;
            }
        }

        target_bucket.count.fetch_add(1, Ordering::AcqRel);
        true
    }

    /// Gets the current estimated request count in the active window
    pub fn current_count(&self, now_ms: u64) -> u64 {
        let window_start_ms = now_ms.saturating_sub(self.window_duration_ms);
        let mut total: u64 = 0;
        for b in &self.buckets {
            let b_time = b.timestamp.load(Ordering::Acquire);
            if b_time >= window_start_ms && b_time <= now_ms {
                total += b.count.load(Ordering::Acquire);
            }
        }
        total
    }
}

/// LockFreeSlidingLimiter manages sliding window rate limits across multiple keys/clients
/// using fast Concurrent Read-Optimized maps and lock-free atomic counters.
#[derive(Debug, Clone)]
pub struct LockFreeSlidingLimiter {
    config: SlidingLimiterConfig,
    clients: Arc<RwLock<HashMap<String, Arc<ClientRateState>>>>,
}

impl LockFreeSlidingLimiter {
    /// Creates a new LockFreeSlidingLimiter with given configuration
    pub fn new(config: SlidingLimiterConfig) -> Self {
        Self {
            config,
            clients: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Helper getting current epoch time in milliseconds
    fn current_time_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    /// Checks if a request for `key` is allowed and records it if true
    pub fn check(&self, key: &str) -> bool {
        let now_ms = Self::current_time_ms();
        let state = {
            let read_guard = self.clients.read();
            if let Some(state) = read_guard.get(key) {
                Arc::clone(state)
            } else {
                drop(read_guard);
                let mut write_guard = self.clients.write();
                write_guard.entry(key.to_string())
                    .or_insert_with(|| Arc::new(ClientRateState::new(&self.config)))
                    .clone()
            }
        };

        state.check_and_record(self.config.max_requests, now_ms)
    }

    /// Gets current request count for a given key
    pub fn count(&self, key: &str) -> u64 {
        let now_ms = Self::current_time_ms();
        let state = {
            let read_guard = self.clients.read();
            read_guard.get(key).cloned()
        };

        if let Some(state) = state {
            state.current_count(now_ms)
        } else {
            0
        }
    }

    /// Clears expired clients from the map to prevent unbounded memory growth
    pub fn cleanup(&self) {
        let now_ms = Self::current_time_ms();
        let window_ms = self.config.window_duration.as_millis() as u64;
        let mut write_guard = self.clients.write();
        write_guard.retain(|_, state| {
            state.current_count(now_ms) > 0 || (now_ms.saturating_sub(state.buckets[0].timestamp.load(Ordering::Acquire))) < window_ms * 2
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lock_free_sliding_limiter_basic() {
        let config = SlidingLimiterConfig {
            max_requests: 5,
            window_duration: Duration::from_secs(10),
            num_buckets: 10,
        };
        let limiter = LockFreeSlidingLimiter::new(config);

        for _ in 0..5 {
            assert!(limiter.check("client_a"));
        }
        // 6th request should be rejected
        assert!(!limiter.check("client_a"));

        // Different client should still be allowed
        assert!(limiter.check("client_b"));
    }
}
