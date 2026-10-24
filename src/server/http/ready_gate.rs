//! Readiness gate for the HTTP listener.
//!
//! The listener serves from the moment it binds. Until the application router is
//! installed with [`ReadyGate::open`], the gate answers every request itself:
//! `/health` returns 200 and `/ready` 503 with `{"status":"starting"}`, and any other
//! route returns 503 with `Retry-After`. Nothing waits for startup to finish, so
//! health checks and clients never sit in the accept backlog.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};

use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde_json::json;
use tokio::net::TcpListener;
use tower::ServiceExt;

/// Seconds a client should wait before retrying a request answered during startup.
const RETRY_AFTER_SECS: &str = "5";

/// Shared switch between the startup gate and the application router.
#[derive(Clone, Default)]
pub struct ReadyGate {
    app: Arc<OnceLock<Router>>,
}

impl ReadyGate {
    /// Creates a closed gate.
    pub fn new() -> Self {
        Self::default()
    }

    /// Installs the application router. Returns false when the gate was already open.
    pub fn open(&self, app: Router) -> bool {
        self.app.set(app).is_ok()
    }

    /// Whether the application router has been installed.
    pub fn is_open(&self) -> bool {
        self.app.get().is_some()
    }
}

/// Router that serves the gate: starting answers until the gate opens, then forwards.
pub fn gated_router(gate: ReadyGate) -> Router {
    Router::new()
        .fallback(ready_gate_middleware)
        .with_state(gate)
}

/// Serves `listener` through the gate until `graceful` resolves.
pub async fn serve_gated<F>(
    listener: TcpListener,
    gate: ReadyGate,
    graceful: F,
) -> std::io::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    axum::serve(
        listener,
        gated_router(gate).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(graceful)
    .await
}

async fn ready_gate_middleware(State(gate): State<ReadyGate>, request: Request) -> Response {
    let Some(app) = gate.app.get() else {
        return starting_response(request.uri().path());
    };
    match app.clone().oneshot(request).await {
        Ok(response) => response,
        Err(never) => match never {},
    }
}

fn starting_response(path: &str) -> Response {
    let body = Json(json!({ "status": "starting" }));
    if path == "/health" {
        return (StatusCode::OK, body).into_response();
    }
    let mut response = (StatusCode::SERVICE_UNAVAILABLE, body).into_response();
    response.headers_mut().insert(
        header::RETRY_AFTER,
        HeaderValue::from_static(RETRY_AFTER_SECS),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request as HttpRequest;
    use axum::routing::get;

    async fn send(router: &Router, path: &str) -> Response {
        let request = HttpRequest::builder()
            .uri(path)
            .body(Body::empty())
            .expect("request");
        router.clone().oneshot(request).await.expect("infallible")
    }

    async fn body_json(response: Response) -> serde_json::Value {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        serde_json::from_slice(&bytes).expect("json body")
    }

    #[tokio::test]
    async fn health_reports_starting_with_200_while_closed() {
        let router = gated_router(ReadyGate::new());
        let response = send(&router, "/health").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_json(response).await, json!({"status": "starting"}));
    }

    #[tokio::test]
    async fn ready_reports_starting_with_503_while_closed() {
        let router = gated_router(ReadyGate::new());
        let response = send(&router, "/ready").await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body_json(response).await, json!({"status": "starting"}));
    }

    #[tokio::test]
    async fn other_routes_return_503_with_retry_after_while_closed() {
        let router = gated_router(ReadyGate::new());
        let response = send(&router, "/v1/memories/search").await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()[header::RETRY_AFTER], "5");
        assert_eq!(body_json(response).await, json!({"status": "starting"}));
    }

    #[tokio::test]
    async fn requests_pass_through_once_open() {
        let gate = ReadyGate::new();
        let router = gated_router(gate.clone());
        let app = Router::new()
            .route("/v1/protected", get(|| async { "protected" }))
            .route("/ready", get(|| async { "ready" }));

        assert!(gate.open(app));
        assert!(gate.is_open());
        assert!(!gate.open(Router::new()), "a second open must be refused");

        let response = send(&router, "/v1/protected").await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&bytes[..], b"protected");

        let response = send(&router, "/ready").await;
        assert_eq!(response.status(), StatusCode::OK);
    }
}
