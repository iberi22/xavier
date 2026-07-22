//! HTTP middleware layer for Xaviers API server.
//!
//! Provides token-bucket rate limiting middleware that integrates with
//! the enterprise rate-limit service. Middleware is applied via tower
//! Layer pattern in the HTTP router setup.

pub mod token_bucket;
