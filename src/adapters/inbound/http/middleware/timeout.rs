//! Timeout HTTP Middleware for Xavier API
//!
//! Provides `timeout_middleware` that enforces a maximum execution duration per HTTP request,
//! configurable via `XAVIER_HTTP_TIMEOUT_SECS` (default 10s).
//! Returns HTTP 504 Gateway Timeout with `{"status": "error", "message": "Request timeout"}`
//! if execution exceeds the timeout duration.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::time::Duration;

/// Default HTTP request timeout in seconds.
pub const DEFAULT_TIMEOUT_SECS: u64 = 10;

/// Get the configured HTTP timeout duration from `XAVIER_HTTP_TIMEOUT_SECS`.
pub fn get_timeout_duration() -> Duration {
    let secs = std::env::var("XAVIER_HTTP_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

/// Axum middleware function that enforces a request timeout.
pub async fn timeout_middleware(req: Request<Body>, next: Next) -> Response {
    let duration = get_timeout_duration();
    timeout_middleware_with_duration(duration, req, next).await
}

/// Axum middleware function that enforces a custom request timeout duration.
pub async fn timeout_middleware_with_duration(
    duration: Duration,
    req: Request<Body>,
    next: Next,
) -> Response {
    match tokio::time::timeout(duration, next.run(req)).await {
        Ok(response) => response,
        Err(_) => (
            StatusCode::GATEWAY_TIMEOUT,
            Json(json!({
                "status": "error",
                "message": "Request timeout"
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
    async fn test_timeout_middleware_passes_fast_request() {
        let app = Router::new()
            .route("/fast", get(|| async { Json(json!({"status": "ok"})) }))
            .layer(axum::middleware::from_fn(timeout_middleware));

        let req = Request::builder().uri("/fast").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_timeout_middleware_times_out_slow_request() {
        let app = Router::new()
            .route(
                "/slow",
                get(|| async {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Json(json!({"status": "ok"}))
                }),
            )
            .layer(axum::middleware::from_fn(|req, next| {
                timeout_middleware_with_duration(Duration::from_millis(20), req, next)
            }));

        let req = Request::builder().uri("/slow").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::GATEWAY_TIMEOUT);

        let body = http_body_util::BodyExt::collect(resp.into_body())
            .await
            .unwrap()
            .to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "error");
        assert_eq!(json["message"], "Request timeout");
    }
}
