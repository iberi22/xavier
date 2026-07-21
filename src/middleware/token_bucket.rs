// SPDX-License-Identifier: MIT OR LICENSE-MESH
use parking_lot::Mutex;
use std::time::{Duration, Instant};

/// A simple Token Bucket rate limiter.
pub struct TokenBucket {
    capacity: f64,
    tokens: f64,
    fill_rate: f64, // tokens per second
    last_fill: Instant,
}

impl TokenBucket {
    /// Create a new Token Bucket.
    /// capacity: max tokens in bucket.
    /// fill_rate: tokens added per second.
    pub fn new(capacity: f64, fill_rate: f64) -> Self {
        Self {
            capacity,
            tokens: capacity,
            fill_rate,
            last_fill: Instant::now(),
        }
    }

    /// Try to consume tokens from the bucket.
    /// Returns true if successful, false if not enough tokens.
    pub fn try_consume(&mut self, amount: f64) -> bool {
        self.refill();
        if self.tokens >= amount {
            self.tokens -= amount;
            true
        } else {
            false
        }
    }

    /// Returns the number of tokens currently in the bucket.
    pub fn tokens(&mut self) -> f64 {
        self.refill();
        self.tokens
    }

    /// Returns the time until at least `amount` tokens will be available.
    pub fn retry_after(&mut self, amount: f64) -> Duration {
        self.refill();
        if self.tokens >= amount {
            Duration::from_secs(0)
        } else {
            let needed = amount - self.tokens;
            Duration::from_secs_f64(needed / self.fill_rate)
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_fill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.fill_rate).min(self.capacity);
        self.last_fill = now;
    }
}

/// Thread-safe wrapper for TokenBucket.
pub struct RateLimiter {
    bucket: Mutex<TokenBucket>,
}

impl RateLimiter {
    pub fn new(capacity: f64, fill_rate: f64) -> Self {
        Self {
            bucket: Mutex::new(TokenBucket::new(capacity, fill_rate)),
        }
    }

    pub fn try_consume_sync(&self, amount: f64) -> bool {
        let mut bucket = self.bucket.lock();
        bucket.try_consume(amount)
    }

    pub fn tokens_sync(&self) -> f64 {
        let mut bucket = self.bucket.lock();
        bucket.tokens()
    }

    pub fn retry_after_sync(&self, amount: f64) -> Duration {
        let mut bucket = self.bucket.lock();
        bucket.retry_after(amount)
    }

    pub async fn try_consume(&self, amount: f64) -> bool {
        self.try_consume_sync(amount)
    }

    pub async fn tokens(&self) -> f64 {
        self.tokens_sync()
    }

    pub async fn retry_after(&self, amount: f64) -> Duration {
        self.retry_after_sync(amount)
    }
}
