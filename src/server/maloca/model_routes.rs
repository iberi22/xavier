//! Maloca Model Inference and Listing REST Routes.
//!
//! Provides placeholder Axum HTTP handlers for model dispatching, listing available local and cloud models,
//! and reporting provider health under `/v1/maloca/models/*`. These routes are placeholders and not a working router.

use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Router,
};

/// Service handling model routing, listing, and provider health checks.
#[derive(Debug, Clone, Default)]
pub struct ModelRouterService;

impl ModelRouterService {
    /// Creates a new `ModelRouterService` instance.
    pub fn new() -> Self {
        Self
    }
}

/// POST `/v1/maloca/models/infer`: Placeholder handler that does not run inference.
pub async fn infer_handler(State(_service): State<ModelRouterService>) -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "status": "error",
            "error": "/v1/maloca/models/infer is a placeholder and does not run inference"
        })),
    )
}

/// GET `/v1/maloca/models/list`: Placeholder handler that does not list models.
pub async fn list_handler(State(_service): State<ModelRouterService>) -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "status": "error",
            "error": "/v1/maloca/models/list is a placeholder and does not list models"
        })),
    )
}

/// GET `/v1/maloca/models/health`: public liveness of the placeholder router.
/// Answers 200 (the route is up) but says plainly that no provider is checked.
pub async fn health_handler(State(_service): State<ModelRouterService>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "unavailable",
            "implemented": false,
            "message": "/v1/maloca/models/health is a placeholder and does not check provider health"
        })),
    )
}

/// Constructs the Axum router for model inference, listing, and health checks.
pub fn router(service: ModelRouterService) -> Router {
    Router::new()
        .route("/v1/maloca/models/infer", post(infer_handler))
        .route("/v1/maloca/models/list", get(list_handler))
        .route("/v1/maloca/models/health", get(health_handler))
        .with_state(service)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_infer_endpoint() {
        let app = router(ModelRouterService::new());
        let payload = serde_json::json!({
            "model": "llama3-8b-local",
            "prompt": "Hello world test prompt",
            "max_tokens": 100,
            "temperature": 0.7
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/maloca/models/infer")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(json["status"], "error");
        assert_eq!(
            json["error"],
            "/v1/maloca/models/infer is a placeholder and does not run inference"
        );
    }

    #[tokio::test]
    async fn test_list_endpoint() {
        let app = router(ModelRouterService::new());

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/maloca/models/list")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(json["status"], "error");
        assert_eq!(
            json["error"],
            "/v1/maloca/models/list is a placeholder and does not list models"
        );
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let app = router(ModelRouterService::new());

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/maloca/models/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(json["status"], "unavailable");
        assert_eq!(json["implemented"], false);
    }
}
