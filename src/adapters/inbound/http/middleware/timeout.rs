//! Timeout HTTP Middleware for Xavier API
//!
//! Provides timeout enforcement for HTTP endpoints.
//! Configurable via the `XAVIER_HTTP_TIMEOUT_SECS` environment variable (default: 10s).

use axum::{
    body::Body,
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::time::Duration;
use tokio::time::timeout;

/// Default timeout duration in seconds when XAVIER_HTTP_TIMEOUT_SECS is not specified.
pub const DEFAULT_TIMEOUT_SECS: u64 = 10;
/// Default timeout duration for bulk operations (e.g. agent index, code-graph scan, sync, prune).
pub const DEFAULT_BULK_TIMEOUT_SECS: u64 = 600;

/// Resolves the timeout duration from `XAVIER_HTTP_TIMEOUT_SECS` or falls back to `DEFAULT_TIMEOUT_SECS`.
pub fn resolve_timeout_duration() -> Duration {
    let secs = std::env::var("XAVIER_HTTP_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

/// Resolves the timeout duration for bulk operations from `XAVIER_BULK_TIMEOUT_SECS` or falls back to `DEFAULT_BULK_TIMEOUT_SECS`.
pub fn resolve_bulk_timeout_duration() -> Duration {
    let secs = std::env::var("XAVIER_BULK_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_BULK_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

/// Determines if an HTTP path represents a long-running administrative or bulk operation.
pub fn is_bulk_path(path: &str) -> bool {
    path.ends_with("/index")
        || path.contains("/scan")
        || path.contains("/reindex")
        || path.contains("/sync")
        || path.contains("/vacuum")
        || path.contains("/prune")
}

/// Axum middleware that limits request processing time to `resolve_timeout_duration()`
/// (or `resolve_bulk_timeout_duration()` for bulk routes).
/// If downstream handler execution exceeds the timeout, returns an HTTP 504 Gateway Timeout.
pub async fn timeout_middleware(req: Request<Body>, next: Next) -> Response {
    let duration = if is_bulk_path(req.uri().path()) {
        resolve_bulk_timeout_duration()
    } else {
        resolve_timeout_duration()
    };
    match timeout(duration, next.run(req)).await {
        Ok(response) => response,
        Err(_) => (
            StatusCode::GATEWAY_TIMEOUT,
            Json(json!({
                "status": "error",
                "message": "Request timeout",
            })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Router};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_timeout_middleware_passes_when_fast() {
        let app = Router::new()
            .route("/fast", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(timeout_middleware));

        let req = Request::builder().uri("/fast").body(Body::empty()).unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_timeout_middleware_triggers_504_when_slow() {
        std::env::set_var("XAVIER_HTTP_TIMEOUT_SECS", "1");
        let app = Router::new()
            .route(
                "/slow",
                get(|| async {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    "done"
                }),
            )
            .layer(axum::middleware::from_fn(timeout_middleware));

        let req = Request::builder().uri("/slow").body(Body::empty()).unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::GATEWAY_TIMEOUT);
    }

    #[test]
    fn test_is_bulk_path_detection() {
        assert!(is_bulk_path("/xavier/antigravity/index"));
        assert!(is_bulk_path("/xavier/opencode/index"));
        assert!(is_bulk_path("/code-graph/scan"));
        assert!(is_bulk_path("/memory/prune"));
        assert!(!is_bulk_path("/health"));
        assert!(!is_bulk_path("/memory/search"));
    }
}
