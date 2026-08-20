//! Sliding Window Rate Limiter implementation
//!
//! Provides thread-safe, high-concurrency sliding window rate limiting with burst support,
//! exact boundary enforcement, and dynamic backoff calculations.

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Configuration for a sliding window rate limiter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum allowed requests within the sliding window duration.
    pub max_requests: usize,
    /// Duration of the sliding window.
    pub window_duration: Duration,
    /// Additional request allowance for short high-throughput bursts.
    pub burst_capacity: usize,
}

impl RateLimitConfig {
    /// Create a new rate limit configuration.
    pub fn new(max_requests: usize, window_duration: Duration, burst_capacity: usize) -> Self {
        Self {
            max_requests,
            window_duration,
            burst_capacity,
        }
    }

    /// Returns the total capacity allowed in a single window, including burst capacity.
    pub fn effective_capacity(&self) -> usize {
        self.max_requests.saturating_add(self.burst_capacity)
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests: 100,
            window_duration: Duration::from_secs(60),
            burst_capacity: 20,
        }
    }
}

/// Evaluation result from checking the rate limiter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimitResult {
    /// Indicates if the request is allowed.
    pub allowed: bool,
    /// Number of remaining allowed requests in the current window.
    pub remaining: usize,
    /// Total maximum requests limit for the bucket.
    pub limit: usize,
    /// Recommended delay before retrying when rate limited.
    pub retry_after: Duration,
}

impl RateLimitResult {
    /// Constructs an allowed result.
    pub fn allowed(remaining: usize, limit: usize) -> Self {
        Self {
            allowed: true,
            remaining,
            limit,
            retry_after: Duration::ZERO,
        }
    }

    /// Constructs a denied result with a retry duration.
    pub fn denied(limit: usize, retry_after: Duration) -> Self {
        Self {
            allowed: false,
            remaining: 0,
            limit,
            retry_after,
        }
    }
}

#[derive(Debug)]
struct WindowEntry {
    timestamps: VecDeque<Instant>,
    last_access: Instant,
}

impl WindowEntry {
    fn new(now: Instant) -> Self {
        Self {
            timestamps: VecDeque::new(),
            last_access: now,
        }
    }

    fn prune_expired(&mut self, now: Instant, window: Duration) {
        self.last_access = now;
        if window.is_zero() {
            self.timestamps.clear();
            return;
        }
        let cutoff = now.checked_sub(window).unwrap_or(now);
        while let Some(&ts) = self.timestamps.front() {
            if ts <= cutoff {
                self.timestamps.pop_front();
            } else {
                break;
            }
        }
    }
}

/// Thread-safe high-concurrency sliding window rate limiter.
#[derive(Debug)]
pub struct SlidingWindowLimiter {
    default_config: RateLimitConfig,
    custom_configs: RwLock<HashMap<String, RateLimitConfig>>,
    entries: RwLock<HashMap<String, Arc<Mutex<WindowEntry>>>>,
}

impl SlidingWindowLimiter {
    /// Creates a new `SlidingWindowLimiter` with default configuration.
    pub fn new(default_config: RateLimitConfig) -> Self {
        Self {
            default_config,
            custom_configs: RwLock::new(HashMap::new()),
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// Creates a new `SlidingWindowLimiter` with global default values.
    pub fn with_defaults() -> Self {
        Self::new(RateLimitConfig::default())
    }

    /// Sets custom rate limit configuration for a specific key.
    pub fn set_config(&self, key: &str, config: RateLimitConfig) {
        self.custom_configs.write().insert(key.to_string(), config);
    }

    /// Returns the rate limit configuration for a key.
    pub fn get_config(&self, key: &str) -> RateLimitConfig {
        self.custom_configs
            .read()
            .get(key)
            .cloned()
            .unwrap_or_else(|| self.default_config.clone())
    }

    /// Evaluates if 1 request is permitted for the given key at the current time.
    pub fn check(&self, key: &str) -> RateLimitResult {
        self.check_n_at(key, 1, Instant::now())
    }

    /// Evaluates if 1 request is permitted for the given key at a specific `Instant`.
    pub fn check_at(&self, key: &str, now: Instant) -> RateLimitResult {
        self.check_n_at(key, 1, now)
    }

    /// Evaluates if `n` requests are permitted for the given key at the current time.
    pub fn check_n(&self, key: &str, count: usize) -> RateLimitResult {
        self.check_n_at(key, count, Instant::now())
    }

    /// Evaluates if `n` requests are permitted for the given key at a specific `Instant`.
    pub fn check_n_at(&self, key: &str, count: usize, now: Instant) -> RateLimitResult {
        let config = self.get_config(key);
        let effective_limit = config.effective_capacity();

        if effective_limit == 0 {
            if count == 0 {
                return RateLimitResult::allowed(0, 0);
            }
            return RateLimitResult::denied(0, config.window_duration);
        }

        let entry_arc = self.get_or_create_entry(key, now);
        let mut entry = entry_arc.lock();
        entry.prune_expired(now, config.window_duration);

        let current_count = entry.timestamps.len();
        let available = effective_limit.saturating_sub(current_count);

        if count == 0 {
            return RateLimitResult::allowed(available, effective_limit);
        }

        if count <= available {
            for _ in 0..count {
                entry.timestamps.push_back(now);
            }
            let remaining = effective_limit.saturating_sub(current_count + count);
            RateLimitResult::allowed(remaining, effective_limit)
        } else {
            let retry_after = if config.window_duration.is_zero() {
                Duration::ZERO
            } else {
                let expired_needed = (current_count + count).saturating_sub(effective_limit);
                if expired_needed > entry.timestamps.len() {
                    config.window_duration
                } else if expired_needed > 0 {
                    let earliest_needed_ts = entry.timestamps[expired_needed - 1];
                    let expiry = earliest_needed_ts + config.window_duration;
                    expiry.saturating_duration_since(now)
                } else {
                    Duration::ZERO
                }
            };

            RateLimitResult::denied(effective_limit, retry_after)
        }
    }

    /// Returns current active window request count for a key.
    pub fn get_usage(&self, key: &str) -> usize {
        self.get_usage_at(key, Instant::now())
    }

    /// Returns active window request count for a key at a specific time.
    pub fn get_usage_at(&self, key: &str, now: Instant) -> usize {
        let config = self.get_config(key);
        if let Some(entry_arc) = self.entries.read().get(key).cloned() {
            let mut entry = entry_arc.lock();
            entry.prune_expired(now, config.window_duration);
            entry.timestamps.len()
        } else {
            0
        }
    }

    /// Resets usage state for a given key.
    pub fn reset(&self, key: &str) {
        if let Some(entry_arc) = self.entries.read().get(key).cloned() {
            let mut entry = entry_arc.lock();
            entry.timestamps.clear();
        }
    }

    /// Clears all entries and custom configurations.
    pub fn clear(&self) {
        self.entries.write().clear();
        self.custom_configs.write().clear();
    }

    /// Prunes expired requests across all keys and removes idle keys with no active requests.
    pub fn cleanup(&self) {
        self.cleanup_at(Instant::now());
    }

    /// Prunes expired requests across all keys at a specific `Instant`.
    pub fn cleanup_at(&self, now: Instant) {
        let mut entries = self.entries.write();
        entries.retain(|key, entry_arc| {
            let config = self
                .custom_configs
                .read()
                .get(key)
                .cloned()
                .unwrap_or_else(|| self.default_config.clone());
            let mut entry = entry_arc.lock();
            entry.prune_expired(now, config.window_duration);
            !entry.timestamps.is_empty()
        });
    }

    /// Returns total active keys tracked in the limiter.
    pub fn active_keys_count(&self) -> usize {
        self.entries.read().len()
    }

    fn get_or_create_entry(&self, key: &str, now: Instant) -> Arc<Mutex<WindowEntry>> {
        if let Some(entry) = self.entries.read().get(key).cloned() {
            return entry;
        }

        let mut entries = self.entries.write();
        entries
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(WindowEntry::new(now))))
            .clone()
    }
}

impl Default for SlidingWindowLimiter {
    fn default() -> Self {
        Self::with_defaults()
    }
}
