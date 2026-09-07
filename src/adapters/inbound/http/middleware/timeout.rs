//! Timeout HTTP Middleware for Xavier API
//!
//! Provides timeout middleware that bounds handler execution times, preventing
//! indefinite hangs on routes like `/memory/search` or `/memory/stats`.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use std::time::Duration;

pub const DEFAULT_TIMEOUT_SECS: u64 = 10;

/// Returns the configured global timeout duration from `XAVIER_HTTP_TIMEOUT_SECS` env var
/// or defaults to 10 seconds.
pub fn get_default_timeout() -> Duration {
    let timeout_secs = std::env::var("XAVIER_HTTP_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TIMEOUT_SECS);
    Duration::from_secs(timeout_secs)
}

/// Axum middleware wrapping request execution with a configurable timeout.
pub async fn timeout_middleware(req: Request<Body>, next: Next) -> Response {
    let duration = get_default_timeout();
    timeout_with_duration(duration, req, next).await
}

/// Helper to execute request with a specific timeout duration.
pub async fn timeout_with_duration(duration: Duration, req: Request<Body>, next: Next) -> Response {
    match tokio::time::timeout(duration, next.run(req)).await {
        Ok(response) => response,
        Err(_) => (
            StatusCode::GATEWAY_TIMEOUT,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("Request timeout after {}s", duration.as_secs()),
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
    async fn test_timeout_middleware_success() {
        let app = Router::new()
            .route("/fast", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(timeout_middleware));

        let req = Request::builder().uri("/fast").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_timeout_middleware_triggers_timeout() {
        let app = Router::new()
            .route(
                "/slow",
                get(|| async {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    "too late"
                }),
            )
            .layer(axum::middleware::from_fn(|req, next| {
                timeout_with_duration(Duration::from_millis(50), req, next)
            }));

        let req = Request::builder().uri("/slow").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::GATEWAY_TIMEOUT);
    }
}
