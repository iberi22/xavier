//! Headless API server configuration.
//!
//! Provides the HTTP server setup for headless mode, including route
//! registration, middleware stack, and shutdown handling.

pub mod auth;
pub mod routes;

pub use routes::*;
