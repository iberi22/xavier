//! Rate limiting using token bucket algorithm
//!
//! Per-tenant, per-API-key, and per-IP rate limiting.

use crate::enterprise::tenant::TenantId;
use crate::middleware::token_bucket::TokenBucket;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

/// Rate limit configuration per tenant/API key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Requests per minute
    pub rpm: u32,
    /// Burst size (requests allowed at once)
    pub burst: u32,
}

impl RateLimitConfig {
    pub fn from_plan_rpm(rpm: u32) -> Self {
        Self {
            rpm,
            burst: rpm / 2, // Allow some burst
        }
    }

    pub fn custom(rpm: u32, burst: u32) -> Self {
        Self { rpm, burst }
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self::from_plan_rpm(30) // Free tier default
    }
}

/// Rate limit result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitResult {
    pub allowed: bool,
    pub remaining: u32,
    pub reset_at: u64,
    pub retry_after_ms: Option<u64>,
}

impl RateLimitResult {
    pub fn allowed(remaining: u32, reset_at: u64) -> Self {
        Self {
            allowed: true,
            remaining,
            reset_at,
            retry_after_ms: None,
        }
    }

    pub fn denied(reset_at: u64, retry_after_ms: u64) -> Self {
        Self {
            allowed: false,
            remaining: 0,
            reset_at,
            retry_after_ms: Some(retry_after_ms),
        }
    }
}

/// Rate limit key types
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum RateLimitKey {
    Tenant(TenantId),
    ApiKey(String),
    Ip(String),
}

/// Token bucket rate limiter
pub struct RateLimiter {
    limiters: HashMap<RateLimitKey, Arc<parking_lot::Mutex<TokenBucket>>>,
    config: HashMap<RateLimitKey, RateLimitConfig>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            limiters: HashMap::new(),
            config: HashMap::new(),
        }
    }

    /// Get or create a limiter for a key
    fn get_limiter(&mut self, key: RateLimitKey) -> Arc<parking_lot::Mutex<TokenBucket>> {
        // Check if limiter already exists
        if let Some(existing) = self.limiters.get(&key) {
            return existing.clone();
        }

        let config = self.config.get(&key).cloned().unwrap_or_default();

        // RPM to tokens per second
        let fill_rate = config.rpm as f64 / 60.0;
        let burst = config.burst as f64;

        let limiter = Arc::new(parking_lot::Mutex::new(TokenBucket::new(burst, fill_rate)));

        self.limiters.insert(key.clone(), limiter.clone());
        limiter
    }

    /// Check if request is allowed and consume a token
    pub fn check(&mut self, key: RateLimitKey) -> RateLimitResult {
        let limiter = self.get_limiter(key.clone());
        let mut bucket = limiter.lock();

        if bucket.try_consume(1.0) {
            RateLimitResult::allowed(bucket.tokens() as u32, 0)
        } else {
            let retry_after = bucket.retry_after(1.0);
            RateLimitResult::denied(0, retry_after.as_millis() as u64)
        }
    }

    /// Set custom config for a key
    pub fn set_config(&mut self, key: RateLimitKey, config: RateLimitConfig) {
        self.config.insert(key, config);
    }

    /// Remove limiter for a key
    pub fn remove(&mut self, key: &RateLimitKey) {
        self.limiters.remove(key);
        self.config.remove(key);
    }

    /// Clear all limiters
    pub fn clear(&mut self) {
        self.limiters.clear();
        self.config.clear();
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Rate limit error
#[derive(Error, Debug)]
pub enum RateLimitError {
    #[error("Rate limit exceeded, retry after {0}ms")]
    Exceeded(u64),
    #[error("Invalid rate limit configuration")]
    InvalidConfig,
}

/// Rate limit middleware state
#[derive(Debug, Clone)]
pub struct RateLimitState {
    pub key: RateLimitKey,
    pub config: RateLimitConfig,
    pub used: u64,
    pub reset_at: std::time::Instant,
}

impl RateLimitState {
    pub fn new(key: RateLimitKey, config: RateLimitConfig) -> Self {
        Self {
            key,
            config,
            used: 0,
            reset_at: std::time::Instant::now() + Duration::from_secs(60),
        }
    }

    pub fn is_expired(&self) -> bool {
        std::time::Instant::now() > self.reset_at
    }
}

/// Simple in-memory rate limiter for testing
#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_rate_limit_config() {
        let config = RateLimitConfig::from_plan_rpm(120);
        assert_eq!(config.rpm, 120);
        assert_eq!(config.burst, 60);
    }

    #[test]
    fn test_rate_limit_creation() {
        let mut limiter = RateLimiter::new();
        let tenant_id = Uuid::new_v4();
        let key = RateLimitKey::Tenant(tenant_id);

        // Should not panic
        let result = limiter.check(key);
        assert!(result.allowed);
    }
}
