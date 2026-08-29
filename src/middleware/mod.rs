//! HTTP middleware layer for Xavier's API server.
//!
//! Provides token-bucket rate limiting middleware that integrates with
//! the enterprise rate-limit service and RBAC authorization middleware.

pub mod auth;
pub mod token_bucket;

pub use auth::require_permission;
pub use token_bucket::{rate_limit_middleware, IpRateLimiter, RateLimiter, TokenBucket};
