//! HTTP middleware layer for Xavier's API server.
//!
//! Provides token-bucket rate limiting middleware that integrates with
//! the enterprise rate-limit service and RBAC authorization middleware.
//!
//! The RBAC and rate-limiting implementations now live with the inbound HTTP
//! adapter that mounts them (`adapters::inbound::http::middleware`), so that
//! adapter no longer imports this facade (ADR-033 Wave 0). The aliases below keep
//! every pre-existing path — including the `auth` and `token_bucket` submodules and
//! the flat re-exports — resolving to the very same items, so no caller changes.

pub use crate::adapters::inbound::http::middleware::clearance;
pub use crate::adapters::inbound::http::middleware::rate_limit as token_bucket;
pub use crate::adapters::inbound::http::middleware::rbac as auth;

pub use crate::adapters::inbound::http::middleware::rate_limit::{
    rate_limit_middleware, IpRateLimiter, RateLimiter, TokenBucket,
};
pub use crate::adapters::inbound::http::middleware::rbac::require_permission;
