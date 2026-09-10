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

/// Resolves the timeout duration from `XAVIER_HTTP_TIMEOUT_SECS` or falls back to `DEFAULT_TIMEOUT_SECS`.
pub fn resolve_timeout_duration() -> Duration {
    let secs = std::env::var("XAVIER_HTTP_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

/// Axum middleware that limits request processing time to `resolve_timeout_duration()`.
/// If downstream handler execution exceeds the timeout, returns an HTTP 504 Gateway Timeout.
pub async fn timeout_middleware(req: Request<Body>, next: Next) -> Response {
    let duration = resolve_timeout_duration();
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
}
