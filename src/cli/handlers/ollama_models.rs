//! Ollama models handlers for list, pull, and set-active (hot-swap backend) operations.
//!
//! Note: Setting XAVIER_LOCAL_LLM_MODEL or XAVIER_EMBEDDING_MODEL updates the process
//! environment. The ProxyUseCase and ModelProviderConfig read these variables dynamically on
//! subsequent requests, allowing model hot-swapping without restarting the Xavier server process.

use axum::{extract::State, http::StatusCode, response::Response, Json};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{error, info};

use crate::cli::handlers::json_response;
use crate::cli::state::CliState;

#[derive(Debug, Deserialize, Serialize)]
pub struct PullModelPayload {
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SetActivePayload {
    pub model: String,
    pub kind: String, // "llm" or "embedding"
}

/// Helper function to retrieve the Ollama base URL by stripping trailing `/v1` from `XAVIER_LOCAL_LLM_URL`
fn get_ollama_base_url() -> String {
    let url_str = std::env::var("XAVIER_LOCAL_LLM_URL")
        .unwrap_or_else(|_| "http://localhost:11434/v1".to_string());
    let mut base_url = url_str.trim().trim_end_matches('/').to_string();
    if base_url.ends_with("/v1") {
        base_url = base_url[..base_url.len() - 3].to_string();
    }
    if base_url.is_empty() {
        "http://localhost:11434".to_string()
    } else {
        base_url
    }
}

/// Handler to list local models currently available in Ollama
/// GET /v1/ollama/models
pub async fn list_models_handler(State(state): State<CliState>) -> Response {
    let base_url = get_ollama_base_url();
    let url = format!("{}/api/tags", base_url);

    info!("Querying Ollama tags from {}", url);

    // Use a reasonable timeout for checking tags
    let client = &state.http_client;
    match client
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        Ok(resp) => {
            if resp.status().is_success() {
                match resp.json::<serde_json::Value>().await {
                    Ok(json) => json_response(StatusCode::OK, json),
                    Err(e) => {
                        error!("Failed to parse Ollama tags response: {}", e);
                        json_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            serde_json::json!({
                                "error": format!("Failed to parse response from Ollama: {}", e)
                            }),
                        )
                    }
                }
            } else {
                let status = resp.status();
                error!("Ollama tags returned non-success status: {}", status);
                json_response(
                    StatusCode::BAD_GATEWAY,
                    serde_json::json!({
                        "error": format!("Ollama returned error status: {}", status)
                    }),
                )
            }
        }
        Err(e) => {
            error!("Ollama tags request failed (unreachable): {}", e);
            json_response(
                StatusCode::SERVICE_UNAVAILABLE,
                serde_json::json!({
                    "error": format!("ollama unreachable: {}", e)
                }),
            )
        }
    }
}

/// Handler to pull a model from Ollama library without streaming
/// POST /v1/ollama/pull
pub async fn pull_model_handler(
    State(state): State<CliState>,
    Json(payload): Json<PullModelPayload>,
) -> Response {
    let base_url = get_ollama_base_url();
    let url = format!("{}/api/pull", base_url);

    info!("Pulling model '{}' from Ollama at {}", payload.name, url);

    // Model pull can be a long running operation. Use a long timeout.
    let client = &state.http_client;
    match client
        .post(&url)
        .json(&serde_json::json!({
            "name": payload.name,
            "stream": false
        }))
        .timeout(Duration::from_secs(600)) // 10 minutes timeout for downloads
        .send()
        .await
    {
        Ok(resp) => {
            if resp.status().is_success() {
                match resp.json::<serde_json::Value>().await {
                    Ok(json) => json_response(StatusCode::OK, json),
                    Err(e) => {
                        error!("Failed to parse Ollama pull response: {}", e);
                        json_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            serde_json::json!({
                                "error": format!("Failed to parse pull response: {}", e)
                            }),
                        )
                    }
                }
            } else {
                let status = resp.status();
                error!("Ollama pull returned non-success status: {}", status);
                json_response(
                    StatusCode::BAD_GATEWAY,
                    serde_json::json!({
                        "error": format!("Ollama returned error status: {}", status)
                    }),
                )
            }
        }
        Err(e) => {
            error!("Ollama pull request failed (unreachable): {}", e);
            json_response(
                StatusCode::SERVICE_UNAVAILABLE,
                serde_json::json!({
                    "error": format!("ollama unreachable: {}", e)
                }),
            )
        }
    }
}

/// Handler to set active model in environment variables for hot-swapping
/// POST /v1/ollama/active
pub async fn set_active_handler(Json(payload): Json<SetActivePayload>) -> Response {
    let kind = payload.kind.trim().to_ascii_lowercase();
    if kind == "llm" {
        info!("Hot-swapping active local LLM model to '{}'", payload.model);
        std::env::set_var("XAVIER_LOCAL_LLM_MODEL", &payload.model);
        json_response(
            StatusCode::OK,
            serde_json::json!({
                "ok": true,
                "model": payload.model,
                "kind": "llm"
            }),
        )
    } else if kind == "embedding" {
        info!(
            "Hot-swapping active local embedding model to '{}'",
            payload.model
        );
        std::env::set_var("XAVIER_EMBEDDING_MODEL", &payload.model);
        json_response(
            StatusCode::OK,
            serde_json::json!({
                "ok": true,
                "model": payload.model,
                "kind": "embedding"
            }),
        )
    } else {
        error!("Invalid model hot-swap kind: '{}'", payload.kind);
        json_response(
            StatusCode::BAD_REQUEST,
            serde_json::json!({
                "error": "Invalid kind. Expected 'llm' or 'embedding'"
            }),
        )
    }
}

/// Handler to retrieve current active LLM and embedding models
/// GET /v1/ollama/active
pub async fn get_active_handler() -> Response {
    let settings = crate::settings::XavierSettings::current();
    let llm = std::env::var("XAVIER_LOCAL_LLM_MODEL")
        .or_else(|_| std::env::var("XAVIER_LLM_MODEL"))
        .ok()
        .or_else(|| Some(settings.models.local_llm_model.clone()))
        .or_else(|| settings.models.llm_model.clone())
        .unwrap_or_else(|| "qwen3-coder".to_string());

    let embedding = std::env::var("XAVIER_EMBEDDING_MODEL")
        .ok()
        .or_else(|| Some(settings.models.embedding_model.clone()));

    json_response(
        StatusCode::OK,
        serde_json::json!({
            "llm": llm,
            "embedding": embedding,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::state::CodeGraphState;
    use axum::body::to_bytes;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use tokio::sync::RwLock as AsyncRwLock;

    // A static mutex to serialize tests modifying environment variables
    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    async fn create_test_state() -> CliState {
        use xavier::agents::provider::router::{ProviderKind, ProviderRouter};
        use xavier::agents::rate_limit::RateLimitManager;
        use xavier::app::proxy_use_case::ProxyUseCase;
        use xavier::app::qmd_memory_adapter::QmdMemoryAdapter;
        use xavier::codebase::conversations_db::ConversationsDb;
        use xavier::coordination::KeyLendingEngine;
        use xavier::coordination::SimpleAgentRegistry;
        use xavier::embedding::NoopEmbedder;
        use xavier::memory::agent_indexer::AgentIndexer;
        use xavier::memory::file_indexer::{FileIndexer, FileIndexerConfig};
        use xavier::memory::qmd_memory::QmdMemory;
        use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
        use xavier::ports::inbound::AgentLifecyclePort;
        use xavier::secrets::audit::QmdAuditLogger;
        use xavier::tasks::store::{InMemoryTaskStore, TaskService};

        let docs = Arc::new(AsyncRwLock::new(Vec::new()));
        let qmd_memory = Arc::new(QmdMemory::new_with_workspace(docs, "test-ws"));
        let memory_port = Arc::new(QmdMemoryAdapter::new(Arc::clone(&qmd_memory)));

        let store_config = VecSqliteStoreConfig {
            path: PathBuf::from(":memory:"),
            embedding_dimensions: 1536,
        };
        let store = Arc::new(VecSqliteMemoryStore::new(store_config).await.unwrap());

        let cg_db = Arc::new(::code_graph::db::CodeGraphDB::in_memory().unwrap());
        let cg_state = Arc::new(tokio::sync::RwLock::new(CodeGraphState {
            db: cg_db.clone(),
            indexer: Arc::new(::code_graph::indexer::Indexer::new(cg_db.clone())),
            query: Arc::new(::code_graph::query::QueryEngine::new(cg_db)),
        }));

        CliState {
            memory: memory_port,
            qmd_memory,
            store,
            workspace_id: "test-ws".to_string(),
            workspace_dir: std::env::current_dir().unwrap(),
            code_graph: cg_state,
            security: Arc::new(xavier::app::security_service::SecurityService::new()),
            security_scan: Arc::new(xavier::app::security_service::SecurityService::new()),
            _time_store: None,
            agent_registry: SimpleAgentRegistry::new(None) as Arc<dyn AgentLifecyclePort>,
            panel_store: Arc::new(
                ConversationsDb::open_in_memory("test-project")
                    .await
                    .unwrap(),
            ),
            secrets_engine: Arc::new(KeyLendingEngine::new(Box::new(QmdAuditLogger::new()), None)),
            event_bus: xavier::coordination::XavierEventBus::new(10),
            tasks: Arc::new(TaskService::new(Arc::new(InMemoryTaskStore::new()))),
            rate_manager: Arc::new(RateLimitManager::new()),
            prompt_cache: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            http_client: reqwest::Client::new(),
            proxy_use_case: Arc::new(ProxyUseCase::new(
                Arc::new(RateLimitManager::new()),
                Arc::new(parking_lot::Mutex::new(HashMap::new())),
            )),
            usage_counters: Arc::new(xavier::observability::UsageCounters::new()),
            session_manager: Arc::new(xavier::security::sessions::SessionManager::new(60)),
            provider_router: Arc::new(tokio::sync::RwLock::new(ProviderRouter::new(
                ProviderKind::Local,
            ))),
            embedder: Arc::new(NoopEmbedder),
            agent_indexer: Arc::new(AgentIndexer::new(FileIndexer::new(
                FileIndexerConfig::default(),
                None,
            ))),
            auth_store: None,
            openclaw_indexer: Arc::new(crate::memory::openclaw_indexer::OpenClawAgentIndexer::new(
                Arc::new(NoopEmbedder),
            )),
            multi_db: xavier::storage::multi_db::MultiDbManager::new(),
            system_scan_cache: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    #[tokio::test]
    async fn test_get_ollama_base_url() {
        let _guard = ENV_LOCK.lock().await;

        std::env::set_var("XAVIER_LOCAL_LLM_URL", "http://my-ollama:11434/v1");
        assert_eq!(get_ollama_base_url(), "http://my-ollama:11434");

        std::env::set_var("XAVIER_LOCAL_LLM_URL", "http://my-ollama:11434/v1/");
        assert_eq!(get_ollama_base_url(), "http://my-ollama:11434");

        std::env::set_var("XAVIER_LOCAL_LLM_URL", "http://my-ollama-direct:12345");
        assert_eq!(get_ollama_base_url(), "http://my-ollama-direct:12345");

        std::env::remove_var("XAVIER_LOCAL_LLM_URL");
        assert_eq!(get_ollama_base_url(), "http://localhost:11434");
    }

    #[tokio::test]
    async fn test_get_active_handler() {
        let _guard = ENV_LOCK.lock().await;

        std::env::set_var("XAVIER_LOCAL_LLM_MODEL", "custom-model-llm");
        std::env::set_var("XAVIER_EMBEDDING_MODEL", "custom-model-emb");

        let response = get_active_handler().await;
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = to_bytes(response.into_body(), 2048).await.unwrap();
        let json_body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(json_body["llm"], "custom-model-llm");
        assert_eq!(json_body["embedding"], "custom-model-emb");

        std::env::remove_var("XAVIER_LOCAL_LLM_MODEL");
        std::env::remove_var("XAVIER_EMBEDDING_MODEL");
    }

    #[tokio::test]
    async fn test_set_active_handler() {
        let _guard = ENV_LOCK.lock().await;

        std::env::remove_var("XAVIER_LOCAL_LLM_MODEL");
        std::env::remove_var("XAVIER_EMBEDDING_MODEL");

        // 1. Swap LLM
        let payload = SetActivePayload {
            model: "swapped-llm-model".to_string(),
            kind: "llm".to_string(),
        };
        let response = set_active_handler(Json(payload)).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            std::env::var("XAVIER_LOCAL_LLM_MODEL").unwrap(),
            "swapped-llm-model"
        );

        // 2. Swap Embedding
        let payload = SetActivePayload {
            model: "swapped-emb-model".to_string(),
            kind: "embedding".to_string(),
        };
        let response = set_active_handler(Json(payload)).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            std::env::var("XAVIER_EMBEDDING_MODEL").unwrap(),
            "swapped-emb-model"
        );

        // 3. Swap Invalid
        let payload = SetActivePayload {
            model: "swapped-invalid".to_string(),
            kind: "invalid-kind".to_string(),
        };
        let response = set_active_handler(Json(payload)).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        std::env::remove_var("XAVIER_LOCAL_LLM_MODEL");
        std::env::remove_var("XAVIER_EMBEDDING_MODEL");
    }

    #[tokio::test]
    async fn test_list_models_handler_success() {
        let _guard = ENV_LOCK.lock().await;

        let mut server = mockito::Server::new_async().await;
        let mock_url = format!("{}/v1", server.url());
        std::env::set_var("XAVIER_LOCAL_LLM_URL", &mock_url);

        let response_data = serde_json::json!({
            "models": [
                {
                    "name": "llama3:latest",
                    "size": 2019393189
                }
            ]
        });

        let mock = server
            .mock("GET", "/api/tags")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&response_data).unwrap())
            .create_async()
            .await;

        let state = create_test_state().await;

        let response = list_models_handler(State(state)).await;
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = to_bytes(response.into_body(), 2048).await.unwrap();
        let json_body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(json_body["models"][0]["name"], "llama3:latest");

        mock.assert_async().await;
        std::env::remove_var("XAVIER_LOCAL_LLM_URL");
    }

    #[tokio::test]
    async fn test_pull_model_handler_success() {
        let _guard = ENV_LOCK.lock().await;

        let mut server = mockito::Server::new_async().await;
        let mock_url = format!("{}/v1", server.url());
        std::env::set_var("XAVIER_LOCAL_LLM_URL", &mock_url);

        let response_data = serde_json::json!({
            "status": "success"
        });

        let mock = server
            .mock("POST", "/api/pull")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "name": "llama3:latest",
                "stream": false
            })))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&response_data).unwrap())
            .create_async()
            .await;

        let state = create_test_state().await;

        let response = pull_model_handler(
            State(state),
            Json(PullModelPayload {
                name: "llama3:latest".to_string(),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = to_bytes(response.into_body(), 2048).await.unwrap();
        let json_body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(json_body["status"], "success");

        mock.assert_async().await;
        std::env::remove_var("XAVIER_LOCAL_LLM_URL");
    }

    #[tokio::test]
    async fn test_list_models_handler_unreachable() {
        let _guard = ENV_LOCK.lock().await;

        // Use a closed port URL
        std::env::set_var("XAVIER_LOCAL_LLM_URL", "http://127.0.0.1:54321/v1");

        let state = create_test_state().await;

        let response = list_models_handler(State(state)).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        let body_bytes = to_bytes(response.into_body(), 2048).await.unwrap();
        let json_body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(json_body["error"]
            .as_str()
            .unwrap()
            .contains("ollama unreachable"));

        std::env::remove_var("XAVIER_LOCAL_LLM_URL");
    }
}
