//! MCP Security and Authentication
//!
//! Provides token-based authentication and Origin header validation
//! for MCP Streamable HTTP transport (spec 2026-07-28).

use crate::security::auth::resolve_xavier_token;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use subtle::ConstantTimeEq;
use tracing::warn;

/// Authentication middleware for MCP HTTP+SSE transport
pub async fn mcp_auth_middleware(req: Request<Body>, next: Next) -> Response {
    let path = req.uri().path();
    if !path.starts_with("/mcp") {
        return next.run(req).await;
    }

    // Spec 2026-07-28: Validate Origin header in ALL connections
    if let Some(origin) = req.headers().get("Origin") {
        let origin_str = origin.to_str().unwrap_or("");

        let is_trusted = if origin_str.is_empty() {
            false
        } else {
            let url = url::Url::parse(origin_str);
            match url {
                Ok(u) => {
                    let host = u.host_str().unwrap_or("");
                    host == "localhost" || host == "127.0.0.1" || host == "[::1]"
                }
                Err(_) => false,
            }
        };

        if !is_trusted {
            warn!(origin = %origin_str, "MCP access rejected: invalid Origin");
            return (StatusCode::FORBIDDEN, "Forbidden: Invalid Origin").into_response();
        }
    } else {
        // Relax: Allow missing Origin header for local connections (localhost / loopback)
        let is_localhost = if let Some(axum::extract::ConnectInfo(addr)) =
            req.extensions()
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        {
            addr.ip().is_loopback()
        } else if let Some(host) = req.headers().get("Host").and_then(|h| h.to_str().ok()) {
            host.starts_with("localhost:")
                || host.starts_with("127.0.0.1:")
                || host == "localhost"
                || host == "127.0.0.1"
                || host.starts_with("[::1]:")
                || host == "[::1]"
        } else {
            false
        };

        if !is_localhost {
            warn!("MCP access rejected: missing Origin header from non-local connection");
            return (StatusCode::FORBIDDEN, "Forbidden: Missing Origin header").into_response();
        }
    }

    // Validate Token
    let expected_token = resolve_xavier_token();
    if expected_token.is_empty() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Security token not configured",
        )
            .into_response();
    }

    let provided_token_str = req
        .headers()
        .get("X-Xavier-Token")
        .and_then(|value| value.to_str().ok())
        .or_else(|| {
            req.headers()
                .get("Authorization")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
        })
        .unwrap_or("")
        .to_string();

    let is_match: bool = provided_token_str
        .as_bytes()
        .ct_eq(expected_token.as_bytes())
        .into();

    let mut req = req;
    if is_match {
        req.extensions_mut()
            .insert(crate::security::auth::Claims::new(
                "root".to_string(),
                "admin@swal.dev".to_string(),
                crate::security::auth::UserRole::Admin,
                chrono::Duration::hours(1),
            ));
        return next.run(req).await;
    }

    if let Ok(secret) = std::env::var("XAVIER_JWT_SECRET") {
        if let Ok(claims) =
            crate::security::auth::validate_jwt(&provided_token_str, secret.as_bytes())
        {
            req.extensions_mut().insert(claims);
            return next.run(req).await;
        }
    }

    warn!("Unauthorized MCP access attempt from {}", req.uri());
    (StatusCode::UNAUTHORIZED, "Unauthorized").into_response()
}

/// Validates an incoming Stdio connection
pub fn validate_stdio_connection() -> anyhow::Result<()> {
    if resolve_xavier_token().is_empty() {
        return Err(anyhow::anyhow!(
            "Stdio MCP connection rejected: XAVIER_TOKEN not set. Security enforcement enabled."
        ));
    }
    Ok(())
}
