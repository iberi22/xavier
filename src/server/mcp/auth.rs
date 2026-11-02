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

/// Entries are exact header values; a standalone "*" allows any nonempty value.
fn matches_allowlist(value: &str, allowed: &str) -> bool {
    !value.is_empty()
        && allowed
            .split(',')
            .map(str::trim)
            .any(|entry| entry == "*" || (!entry.is_empty() && entry == value))
}

fn is_trusted_origin(origin: &str, allowed: &str) -> bool {
    match url::Url::parse(origin) {
        Ok(url) if matches!(url.scheme(), "http" | "https") && url.host_str().is_some() => {
            matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
                || matches_allowlist(origin, allowed)
        }
        _ => false,
    }
}

/// Authentication middleware for MCP HTTP+SSE transport
pub async fn mcp_auth_middleware(req: Request<Body>, next: Next) -> Response {
    let path = req.uri().path();
    if !path.starts_with("/mcp") {
        return next.run(req).await;
    }

    // Spec 2026-07-28: Validate Origin header in ALL connections
    if let Some(origin) = req.headers().get("Origin") {
        let origin_str = origin.to_str().unwrap_or("");

        let allowed_origins = std::env::var("XAVIER_ALLOWED_ORIGINS").unwrap_or_default();
        let is_trusted = is_trusted_origin(origin_str, &allowed_origins);

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

        let allowed_hosts = std::env::var("XAVIER_ALLOWED_HOSTS").unwrap_or_default();
        let is_allowed_host = req
            .headers()
            .get("Host")
            .and_then(|host| host.to_str().ok())
            .is_some_and(|host| matches_allowlist(host, &allowed_hosts));

        if !is_localhost && !is_allowed_host {
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

    let provided_bytes = provided_token_str.as_bytes();
    let expected_bytes = expected_token.as_bytes();

    let is_match: bool = if provided_bytes.len() != expected_bytes.len() {
        let _ = expected_bytes.ct_eq(expected_bytes);
        false
    } else {
        provided_bytes.ct_eq(expected_bytes).into()
    };

    let mut req = req;
    if is_match {
        req.extensions_mut()
            .insert(crate::security::auth::Claims::new(
                "root".to_string(),
                "admin@swal.dev".to_string(),
                crate::security::auth::UserRole::Admin,
                chrono::Duration::hours(1),
            ));
        // Root-only tools key off this credential, never the claimed role.
        return super::server::with_root_credential(true, next.run(req)).await;
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

/// Validates an incoming Stdio connection.
///
/// Stdio runs over a local OS pipe to the process that spawned us (the MCP
/// client): there is no network boundary, so the HTTP bearer token is not
/// required here — this matches the stdio convention of MCP servers. The
/// HTTP/SSE transports above still enforce `XAVIER_TOKEN`/JWT strictly.
pub fn validate_stdio_connection() -> anyhow::Result<()> {
    if resolve_xavier_token().is_empty() {
        tracing::warn!(
            "XAVIER_TOKEN not set: allowing local stdio MCP session without HTTP token \
             (stdio inherits OS process trust; HTTP/SSE transports still require auth)."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{is_trusted_origin, matches_allowlist};

    #[test]
    fn allows_local_origins_by_default() {
        for origin in [
            "http://localhost:8006",
            "http://127.0.0.1",
            "http://[::1]:8006",
        ] {
            assert!(is_trusted_origin(origin, ""));
        }
    }

    #[test]
    fn allows_explicit_remote_origins() {
        let allowed = " https://app.example.com, ,https://other.example.com:8443 ";
        assert!(is_trusted_origin("https://app.example.com", allowed));
        assert!(is_trusted_origin("https://other.example.com:8443", allowed));
    }

    #[test]
    fn denies_unlisted_origins() {
        let allowed = "https://app.example.com";
        for origin in [
            "https://evil.example.com",
            "https://app.example.com.evil.com",
            "http://app.example.com",
            "https://app.example.com:8443",
            "",
            "null",
        ] {
            assert!(!is_trusted_origin(origin, allowed));
        }
        assert!(!is_trusted_origin("https://remote.example.com", ""));
    }

    #[test]
    fn wildcard_allows_valid_remote_origins() {
        assert!(is_trusted_origin("https://remote.example.com", " , * , "));
        for origin in ["", "null", "not a URL", "file:///tmp/test"] {
            assert!(!is_trusted_origin(origin, "*"));
        }
    }

    #[test]
    fn host_allowlist_matches_exact_authority_or_wildcard() {
        assert!(matches_allowlist(
            "mcp.example.com:8006",
            "other.example.com, mcp.example.com:8006"
        ));
        assert!(!matches_allowlist(
            "mcp.example.com:8007",
            "mcp.example.com:8006"
        ));
        assert!(!matches_allowlist("evil.example.com", ""));
        assert!(matches_allowlist("remote.example.com:8006", "*"));
        assert!(!matches_allowlist("", "*"));
    }
}
