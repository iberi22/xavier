//! Middleware and setup for the CLI HTTP server.
//!
//! This module implements authentication and rate-limiting middleware used by the
//! CLI's HTTP API. It ensures secure access via token validation and prevents
//! resource exhaustion through global rate limits.

use crate::cli::config::resolve_http_token;
use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};
use tracing::warn;

pub async fn auth_middleware(
    State(state): State<CliState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let expected_token = match resolve_http_token() {
        Ok(token) => token,
        Err(e) => {
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"status":"error","message": format!("Token resolution failed: {e}")}),
            );
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

    // 1. Check Root Token
    use subtle::ConstantTimeEq;
    let provided_bytes = provided_token_str.as_bytes();
    let expected_bytes = expected_token.as_bytes();
    let is_match: bool = provided_bytes.ct_eq(expected_bytes).into();

    if is_match {
        let mut req = req;
        req.extensions_mut().insert(SessionInfo { is_ephemeral: false });
        return next.run(req).await;
    }

    // 2. Check Ephemeral Session (Zero-Trust Frontend)
    if state.session_manager.validate_session(provided_token_str) {
        let mut req = req;
        req.extensions_mut().insert(SessionInfo { is_ephemeral: true });
        return next.run(req).await;
    }

    json_response(
        StatusCode::UNAUTHORIZED,
        serde_json::json!({"status":"error","message":"Unauthorized"}),
    )
}

#[derive(Clone, Copy, Debug)]
pub struct SessionInfo {
    pub is_ephemeral: bool,
}

pub async fn rate_limit_middleware(
    State(state): State<CliState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let provider = "api_gateway";
    match state.rate_manager.get_status(provider).await {
        Ok(status) => {
            if let Some(until) = status.rate_limited_until {
                if until > chrono::Utc::now() {
                    warn!("Global rate limit reached, blocking request");
                    return json_response(
                        StatusCode::TOO_MANY_REQUESTS,
                        serde_json::json!({
                            "status": "error",
                            "message": "Rate limit exceeded. Please try again later.",
                            "retry_after": until.to_rfc3339()
                        }),
                    );
                }
            }
        }
        Err(e) => {
            warn!("Failed to check rate limit: {}", e);
        }
    }

    let response = next.run(req).await;

    if let Err(e) = state
        .rate_manager
        .track_request(provider, 1, response.status().as_u16(), 0.0, false)
        .await
    {
        warn!("Failed to track rate limit usage: {}", e);
    }

    response
}
