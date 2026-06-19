//! MCP Security and Authentication
//!
//! Provides token-based authentication for HTTP+SSE transport and
//! connection validation for Stdio transport. Also implements
//! session-based rate limiting using an LRU cache to prevent memory leaks.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use tracing::{warn, info};
use crate::security::auth::resolve_xavier_token;
use subtle::ConstantTimeEq;
use crate::middleware::token_bucket::RateLimiter;
use once_cell::sync::Lazy;
use moka::future::Cache;
use std::time::Duration;

static RATE_LIMITER: Lazy<McpRateLimiter> = Lazy::new(McpRateLimiter::new);

/// Rate limiter for MCP sessions with TTL and size limit
pub struct McpRateLimiter {
    limiters: Cache<String, Arc<RateLimiter>>,
}

impl McpRateLimiter {
    pub fn new() -> Self {
        Self {
            limiters: Cache::builder()
                .max_capacity(1000)
                .time_to_idle(Duration::from_secs(3600))
                .build(),
        }
    }

    pub async fn check(&self, session_id: &str) -> bool {
        // Use get_with for atomic "get or insert"
        let limiter = self.limiters.get_with(session_id.to_string(), async {
            // 60 RPM, burst of 10
            Arc::new(RateLimiter::new(10.0, 1.0))
        }).await;

        limiter.try_consume(1.0).await
    }
}

/// Authentication middleware for MCP HTTP+SSE transport
pub async fn mcp_auth_middleware(
    req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path();
    // Check if path starts with /mcp to be more robust
    if !path.starts_with("/mcp") {
        return next.run(req).await;
    }

    // 1. Validate Token
    let expected_token = match resolve_xavier_token() {
        Ok(token) => token,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Security token not configured").into_response();
        }
    };

    let provided_token = req
        .headers()
        .get("X-Xavier-Token")
        .and_then(|value| value.to_str().ok())
        .or_else(|| {
            req.headers()
                .get("Authorization")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
        });

    let provided_token_str = provided_token.unwrap_or("");
    let is_match: bool = provided_token_str.as_bytes().ct_eq(expected_token.as_bytes()).into();

    if !is_match {
        warn!("Unauthorized MCP access attempt from {}", req.uri());
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    // 2. Rate Limiting by Session ID
    let session_id = req
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    if let Some(sid) = session_id {
        if !RATE_LIMITER.check(&sid).await {
            warn!(session_id = %sid, "MCP rate limit exceeded");
            return (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded").into_response();
        }
    }

    next.run(req).await
}

/// Validates an incoming Stdio connection
pub fn validate_stdio_connection() -> anyhow::Result<()> {
    // For Stdio, we enforce that a token must be set in the environment.
    // This prevents unauthorized local processes from launching the server
    // without knowing the secret.
    if resolve_xavier_token().is_err() {
        return Err(anyhow::anyhow!("Stdio MCP connection rejected: XAVIER_TOKEN not set. Security enforcement enabled."));
    }
    info!("Stdio MCP connection validated via XAVIER_TOKEN presence.");
    Ok(())
}
