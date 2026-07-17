use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post, delete},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::plugin::{PluginManager, FallbackStep, PluginHealthMonitor};
use crate::LanguageDiscovery;
use crate::types::Language;

#[derive(Clone)]
pub struct PluginApiState {
    pub manager: Arc<PluginManager>,
    pub health: Option<Arc<PluginHealthMonitor>>,
    pub discovery: Option<Arc<dyn LanguageDiscovery>>,
}

#[derive(Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Serialize, Deserialize)]
pub struct InstallRequest {
    pub name: String,
    pub version: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct FallbackUpdateRequest {
    pub steps: Vec<FallbackStep>,
}

#[derive(Serialize, Deserialize)]
pub struct FallbackChainResponse {
    pub lang: String,
    pub steps: Vec<FallbackStep>,
}

async fn list(State(state): State<PluginApiState>) -> Json<Vec<crate::plugin::PluginDescriptor>> {
    Json(state.manager.list())
}

async fn available(
    State(state): State<PluginApiState>,
) -> Result<Json<Vec<crate::plugin::RegistryEntry>>, (StatusCode, Json<ErrorResponse>)> {
    match state.manager.registry().fetch_index().await {
        Ok(entries) => Ok(Json(entries)),
        Err(e) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: format!("Registry unreachable: {}", e),
            }),
        )),
    }
}

async fn health_aggregate(State(state): State<PluginApiState>) -> impl IntoResponse {
    if state.health.is_some() {
        // Placeholder for #485
        StatusCode::NOT_IMPLEMENTED.into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "Health monitoring not enabled (#485)".to_string(),
            }),
        )
            .into_response()
    }
}

async fn install(
    State(state): State<PluginApiState>,
    Json(req): Json<InstallRequest>,
) -> Result<Json<crate::plugin::PluginDescriptor>, (StatusCode, Json<ErrorResponse>)> {
    match state.manager.install(&req.name, req.version).await {
        Ok(desc) => Ok(Json(desc)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

async fn update(
    State(state): State<PluginApiState>,
    Path(name): Path<String>,
) -> Result<Json<Vec<crate::plugin::PluginDescriptor>>, (StatusCode, Json<ErrorResponse>)> {
    match state.manager.update(Some(name)).await {
        Ok(descs) => Ok(Json(descs)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

async fn rollback(
    State(state): State<PluginApiState>,
    Path(name): Path<String>,
) -> Result<Json<crate::plugin::PluginDescriptor>, (StatusCode, Json<ErrorResponse>)> {
    match state.manager.rollback(&name).await {
        Ok(desc) => Ok(Json(desc)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

async fn uninstall(
    State(state): State<PluginApiState>,
    Path(name): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    match state.manager.uninstall(&name).await {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

async fn plugin_health(
    State(state): State<PluginApiState>,
    Path(_name): Path<String>,
) -> impl IntoResponse {
    if state.health.is_some() {
        StatusCode::NOT_IMPLEMENTED.into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "Health monitoring not enabled (#485)".to_string(),
            }),
        )
            .into_response()
    }
}

async fn plugin_metrics(
    State(state): State<PluginApiState>,
    Path(_name): Path<String>,
) -> impl IntoResponse {
    if state.health.is_some() {
        StatusCode::NOT_IMPLEMENTED.into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "Health monitoring not enabled (#485)".to_string(),
            }),
        )
            .into_response()
    }
}

async fn list_fallbacks(State(state): State<PluginApiState>) -> Json<Vec<FallbackChainResponse>> {
    let chains = state.manager.fallback().read().all_chains();
    let response = chains
        .into_iter()
        .map(|(lang, steps)| FallbackChainResponse {
            lang: lang.as_str().to_string(),
            steps,
        })
        .collect();
    Json(response)
}

async fn set_fallback(
    State(state): State<PluginApiState>,
    Path(lang_str): Path<String>,
    Json(req): Json<FallbackUpdateRequest>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let lang = Language::from_db_str(&lang_str);
    {
        let mut fallback = state.manager.fallback().write();
        fallback.set(&lang, req.steps);
        fallback.save();
    }
    Ok(StatusCode::OK)
}

async fn languages(State(state): State<PluginApiState>) -> impl IntoResponse {
    if state.discovery.is_some() {
        StatusCode::NOT_IMPLEMENTED.into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "Language discovery not enabled (#486)".to_string(),
            }),
        )
            .into_response()
    }
}

pub fn router<S>(state: PluginApiState) -> Router<S> {
    Router::new()
        .route("/api/v1/plugins", get(list))
        .route("/api/v1/plugins/available", get(available))
        .route("/api/v1/plugins/health", get(health_aggregate))
        .route("/api/v1/plugins/install", post(install))
        .route("/api/v1/plugins/{name}/update", post(update))
        .route("/api/v1/plugins/{name}/rollback", post(rollback))
        .route("/api/v1/plugins/{name}", delete(uninstall))
        .route("/api/v1/plugins/{name}/health", get(plugin_health))
        .route("/api/v1/plugins/{name}/metrics", get(plugin_metrics))
        .route("/api/v1/plugins/fallback", get(list_fallbacks))
        .route("/api/v1/plugins/fallback/{lang}", post(set_fallback))
        .route("/api/v1/languages", get(languages))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use crate::plugin::types::{RegistryEntry, PluginRegistry, PluginDescriptor};
    use crate::plugin::PluginManager;
    use crate::error::Result;
    use tower::ServiceExt;
    use std::collections::HashMap;

    struct MockRegistry {
        entries: Vec<RegistryEntry>,
    }

    impl PluginRegistry for MockRegistry {
        fn fetch_index(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<RegistryEntry>>> + Send>> {
            let entries = self.entries.clone();
            Box::pin(async move { Ok(entries) })
        }

        fn search(&self, _query: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<RegistryEntry>>> + Send>> {
            let entries = self.entries.clone();
            Box::pin(async move { Ok(entries) })
        }

        fn get(&self, name: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<RegistryEntry>> + Send>> {
            let entry = self.entries.iter().find(|e| e.name == name).cloned().expect("not found");
            Box::pin(async move { Ok(entry) })
        }
    }

    fn setup_test_router() -> Router<()> {
        let registry = Arc::new(MockRegistry {
            entries: vec![RegistryEntry {
                name: "mock-plugin".to_string(),
                display_name: "Mock Plugin".to_string(),
                description: "A mock plugin".to_string(),
                version: "1.0.0".to_string(),
                author: "test".to_string(),
                languages: vec![Language::Python],
                capabilities: vec!["parse".to_string()],
                platform: HashMap::new(),
                min_engine_version: "0.1.0".to_string(),
                license: "MIT".to_string(),
            }],
        });
        let manager = Arc::new(PluginManager::with_engine_and_registry(
            Arc::new(crate::plugin::engine::ProcessEngine::default()),
            registry,
        ));
        // Ensure we start with a clean state for tests
        manager.fallback().write().clear(&Language::Python);

        let state = PluginApiState {
            manager,
            health: None,
            discovery: None,
        };
        router(state)
    }

    #[tokio::test]
    async fn test_install_and_list() {
        let app = setup_test_router();

        // 1. POST /api/v1/plugins/install
        let response = app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/plugins/install")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name": "mock-plugin"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // 2. GET /api/v1/plugins
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/plugins")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        let plugins: Vec<PluginDescriptor> = serde_json::from_slice(&body).unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "mock-plugin");
    }

    #[tokio::test]
    async fn test_delete_plugin() {
        let app = setup_test_router();

        // Install first
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/plugins/install")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name": "mock-plugin"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        // DELETE /api/v1/plugins/mock-plugin
        let response = app.clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/plugins/mock-plugin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Verify list is empty
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/plugins")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        let plugins: Vec<PluginDescriptor> = serde_json::from_slice(&body).unwrap();
        assert!(plugins.is_empty());
    }

    #[tokio::test]
    async fn test_fallback_chains() {
        let app = setup_test_router();

        // 1. GET /api/v1/plugins/fallback (should be empty initially because all_chains() only returns overrides)
        let response = app.clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/plugins/fallback")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        let chains: Vec<FallbackChainResponse> = serde_json::from_slice(&body).unwrap();
        assert!(chains.is_empty());

        // 2. POST /api/v1/plugins/fallback/python
        let response = app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/plugins/fallback/python")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"steps": [{"Plugin": "mock-plugin"}]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // 3. GET /api/v1/plugins/fallback again
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/plugins/fallback")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        let chains: Vec<FallbackChainResponse> = serde_json::from_slice(&body).unwrap();
        assert_eq!(chains.len(), 1);
        assert_eq!(chains[0].lang, "python");
        assert_eq!(chains[0].steps, vec![FallbackStep::Plugin("mock-plugin".to_string())]);
    }

    #[tokio::test]
    async fn test_503_endpoints() {
        let app = setup_test_router();

        let endpoints = vec![
            "/api/v1/plugins/health",
            "/api/v1/plugins/mock-plugin/health",
            "/api/v1/plugins/mock-plugin/metrics",
            "/api/v1/languages",
        ];

        for ep in endpoints {
            let response = app.clone()
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(ep)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        }
    }
}
