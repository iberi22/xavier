//! Rate Limiter Module
//!
//! Provides high-concurrency sliding window rate limiting and token bucket primitives
//! for DoS prevention and MCP throttling.

pub mod sliding_window;

pub use sliding_window::{RateLimitConfig, RateLimitResult, SlidingWindowLimiter};
