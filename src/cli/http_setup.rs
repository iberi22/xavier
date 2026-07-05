//! Middleware and setup for the CLI HTTP server.
//!
//! This module implements authentication and rate-limiting middleware used by the
//! CLI's HTTP API. It ensures secure access via token validation and prevents
//! resource exhaustion through global rate limits.

use crate::cli::config::resolve_http_token;
use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use xavier::coordination::secrets::SecretLease;
use axum::{
    body::Body,
    extract::{ConnectInfo, State},
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};
use std::net::SocketAddr;
use tracing::warn;

pub async fn auth_middleware(
    State(state): State<CliState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path();
    if path == "/health" || path == "/headless/health" {
        return next.run(req).await;
    }

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
        // Root token bypasses RBAC for now as "Super Admin"
        let mut req = req;
        req.extensions_mut().insert(SessionInfo {
            is_ephemeral: false,
            api_token: None,
            lease: None,
        });
        return next.run(req).await;
    }

    // 2. Check Lease Token (F3 - Proxy Authentication)
    if let Some(lease) = state.secrets_engine.get_lease(provided_token_str).await {
        if !lease.is_expired() {
            let mut req = req;
            req.extensions_mut().insert(SessionInfo {
                is_ephemeral: true,
                api_token: None,
                lease: Some(lease),
            });
            return next.run(req).await;
        }
    }

    // 3. Check Ephemeral Session (Zero-Trust Frontend)
    if state.session_manager.validate_session(provided_token_str) {
        let mut req = req;
        req.extensions_mut().insert(SessionInfo {
            is_ephemeral: true,
            api_token: None,
            lease: None,
        });
        return next.run(req).await;
    }

    // 4. Check Persistent API Tokens
    if provided_token_str.starts_with("xav_") {
        let store = xavier::security::tokens::TokenStore::new();
        if let Ok(Some(token_meta)) = store.validate_token(provided_token_str).await {
            // Scope validation
            let has_scope = match path {
                p if p.starts_with("/memory/search") || p.starts_with("/v1/memories/search") => {
                    token_meta.scopes.contains(&"read".to_string()) || token_meta.scopes.contains(&"all".to_string())
                }
                p if p.starts_with("/memory/add") || p.starts_with("/v1/memories") => {
                    token_meta.scopes.contains(&"write".to_string()) || token_meta.scopes.contains(&"all".to_string())
                }
                _ => true, // Default allow for other endpoints for now, or refine as needed
            };

            if !has_scope {
                return json_response(
                    StatusCode::FORBIDDEN,
                    serde_json::json!({"status":"error","message":"Insufficient scopes"}),
                );
            }

            // Integrate RBAC authorization check for persistent tokens
            let permission = match path {
                p if p.contains("/add") || p.contains("/update") || p.contains("/delete") => {
                    xavier::enterprise::rbac::Permission::Write
                }
                _ => xavier::enterprise::rbac::Permission::Read,
            };

            // Scaffolding for RBAC authorize call
            if let Err(e) = xavier::enterprise::rbac::authorize(
                uuid::Uuid::nil(), // Placeholder for real user_id from token_meta
                permission,
                path.to_string(),
            ) {
                return json_response(
                    StatusCode::FORBIDDEN,
                    serde_json::json!({"status":"error","message": e.to_string()}),
                );
            }

            let mut req = req;
            req.extensions_mut().insert(SessionInfo {
                is_ephemeral: false,
                api_token: Some(token_meta),
                lease: None,
            });
            return next.run(req).await;
        }
    }

    json_response(
        StatusCode::UNAUTHORIZED,
        serde_json::json!({"status":"error","message":"Unauthorized"}),
    )
}

#[derive(Clone, Debug)]
pub struct SessionInfo {
    pub is_ephemeral: bool,
    pub api_token: Option<xavier::security::tokens::ApiTokenMetadata>,
    pub lease: Option<SecretLease>,
}

pub async fn rate_limit_middleware(
    State(state): State<CliState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path();

    // Default provider based on IP if available
    let mut provider = if let Some(addr) = req.extensions().get::<ConnectInfo<SocketAddr>>() {
        format!("ip:{}", addr.0.ip())
    } else {
        "api_gateway".to_string()
    };

    // Override with agent_id if lease is present
    if let Some(session) = req.extensions().get::<SessionInfo>() {
        if let Some(lease) = &session.lease {
            provider = format!("agent:{}", lease.agent_id);
        }
    }

    // F3: Proxy RPM Rate Limiting
    if path.starts_with("/v1/proxy/") {
        let limit = std::env::var("XAVIER_PROXY_RATE_LIMIT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60);

        match state.rate_manager.check_rpm_limit(&provider, limit).await {
            Ok(allowed) => {
                if !allowed {
                    warn!("Proxy rate limit exceeded for {}", provider);
                    return json_response(
                        StatusCode::TOO_MANY_REQUESTS,
                        serde_json::json!({
                            "status": "error",
                            "message": "Proxy rate limit exceeded. Max 60 requests per minute.",
                        }),
                    );
                }
            }
            Err(e) => {
                warn!("Failed to check proxy rate limit: {}", e);
            }
        }

        // Record the request immediately for accurate RPM tracking
        if let Err(e) = state
            .rate_manager
            .track_request(&provider, 1, 200, 0.0, false)
            .await
        {
            warn!("Failed to track proxy request: {}", e);
        }
    }

    match state.rate_manager.get_status(&provider).await {
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
        .track_request(&provider, 1, response.status().as_u16(), 0.0, false)
        .await
    {
        warn!("Failed to track rate limit usage: {}", e);
    }

    response
}
