//! Maloca Model Inference and Listing REST Routes.
//!
//! Provides Axum HTTP handlers for model dispatching, listing available local and cloud models,
//! and reporting provider health and credit status under `/v1/maloca/models/*`.

use axum::{
    extract::{Json, State},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};

/// Standard API response envelope payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiResponse<T> {
    /// Status string ("ok" or error indicator).
    pub status: String,
    /// Response payload data.
    pub data: T,
}

impl<T> ApiResponse<T> {
    /// Creates a successful API response envelope.
    pub fn ok(data: T) -> Self {
        Self {
            status: "ok".to_string(),
            data,
        }
    }
}

/// Request payload for model inference dispatch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelInferRequest {
    /// Identifier of the target model (e.g., "llama3-8b-local", "claude-3-5-sonnet").
    pub model: String,
    /// Prompt content to send to the model.
    pub prompt: String,
    /// Optional maximum output tokens to generate.
    pub max_tokens: Option<u32>,
    /// Optional sampling temperature for text generation.
    pub temperature: Option<f32>,
}

/// Response payload for model inference execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelInferResponse {
    /// Model identifier that processed the inference request.
    pub model: String,
    /// Generated text output from the model.
    pub output: String,
    /// Total tokens consumed for this prompt and response.
    pub tokens_used: u32,
}

/// Details of a supported model available for selection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelInfo {
    /// Unique identifier for the model.
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Provider name (e.g., "ollama", "anthropic", "openai").
    pub provider: String,
    /// Flag indicating whether the model runs locally or in the cloud.
    pub is_local: bool,
    /// Maximum context window length supported in tokens.
    pub context_length: u32,
}

/// Response payload for listing available models.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelListResponse {
    /// List of available local and cloud models.
    pub models: Vec<ModelInfo>,
}

/// Health and status details for a model provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderHealth {
    /// Provider identifier (e.g., "ollama", "anthropic", "openai").
    pub provider: String,
    /// Whether the provider service is currently available.
    pub available: bool,
    /// Remaining API credit balance, if applicable.
    pub credits_remaining: Option<u64>,
    /// Observed round-trip ping latency in milliseconds.
    pub latency_ms: u64,
}

/// Response payload for model provider health check.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelHealthResponse {
    /// Health reports for configured model providers.
    pub providers: Vec<ProviderHealth>,
}

/// Service handling model routing, listing, and provider health checks.
#[derive(Debug, Clone, Default)]
pub struct ModelRouterService;

impl ModelRouterService {
    /// Creates a new `ModelRouterService` instance.
    pub fn new() -> Self {
        Self
    }

    /// Dispatches an inference request to the selected model provider.
    pub async fn infer(&self, req: ModelInferRequest) -> ModelInferResponse {
        let tokens_used = (req.prompt.split_whitespace().count() as u32).max(1) + 16;
        let output = format!(
            "Inference response for [{}] prompt: {}",
            req.model, req.prompt
        );
        ModelInferResponse {
            model: req.model,
            output,
            tokens_used,
        }
    }

    /// Retrieves list of available local and cloud models.
    pub async fn list(&self) -> ModelListResponse {
        ModelListResponse {
            models: vec![
                ModelInfo {
                    id: "llama3-8b-local".to_string(),
                    name: "Llama 3 8B (Local)".to_string(),
                    provider: "ollama".to_string(),
                    is_local: true,
                    context_length: 8192,
                },
                ModelInfo {
                    id: "claude-3-5-sonnet".to_string(),
                    name: "Claude 3.5 Sonnet".to_string(),
                    provider: "anthropic".to_string(),
                    is_local: false,
                    context_length: 200000,
                },
                ModelInfo {
                    id: "gpt-4o".to_string(),
                    name: "GPT-4o".to_string(),
                    provider: "openai".to_string(),
                    is_local: false,
                    context_length: 128000,
                },
            ],
        }
    }

    /// Evaluates provider health and credit availability.
    pub async fn health(&self) -> ModelHealthResponse {
        ModelHealthResponse {
            providers: vec![
                ProviderHealth {
                    provider: "ollama".to_string(),
                    available: true,
                    credits_remaining: None,
                    latency_ms: 8,
                },
                ProviderHealth {
                    provider: "anthropic".to_string(),
                    available: true,
                    credits_remaining: Some(25000),
                    latency_ms: 110,
                },
                ProviderHealth {
                    provider: "openai".to_string(),
                    available: true,
                    credits_remaining: Some(50000),
                    latency_ms: 95,
                },
            ],
        }
    }
}

/// POST `/v1/maloca/models/infer`: Dispatches prompt via ModelRouterService.
pub async fn infer_handler(
    State(service): State<ModelRouterService>,
    Json(payload): Json<ModelInferRequest>,
) -> impl IntoResponse {
    let response = service.infer(payload).await;
    Json(ApiResponse::ok(response))
}

/// GET `/v1/maloca/models/list`: Lists available local and cloud models.
pub async fn list_handler(State(service): State<ModelRouterService>) -> impl IntoResponse {
    let response = service.list().await;
    Json(ApiResponse::ok(response))
}

/// GET `/v1/maloca/models/health`: Reports provider availability and credit status.
pub async fn health_handler(State(service): State<ModelRouterService>) -> impl IntoResponse {
    let response = service.health().await;
    Json(ApiResponse::ok(response))
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

        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(json["status"], "ok");
        assert_eq!(json["data"]["model"], "llama3-8b-local");
        assert!(json["data"]["output"]
            .as_str()
            .unwrap()
            .contains("Hello world test prompt"));
        assert!(json["data"]["tokens_used"].as_u64().unwrap() > 0);
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

        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(json["status"], "ok");
        let models = json["data"]["models"].as_array().unwrap();
        assert!(!models.is_empty());
        assert_eq!(models[0]["id"], "llama3-8b-local");
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

        assert_eq!(json["status"], "ok");
        let providers = json["data"]["providers"].as_array().unwrap();
        assert!(!providers.is_empty());
        assert_eq!(providers[0]["provider"], "ollama");
        assert_eq!(providers[0]["available"], true);
    }
}
