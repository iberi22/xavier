//! Navigation API handlers for shell-like interaction (ls, cd, pwd)

use crate::cli::state::CliState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Deserialize)]
pub struct LsParams {
    pub path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CdParams {
    pub path: String,
}

pub async fn ls_handler(
    State(state): State<CliState>,
    Query(params): Query<LsParams>,
) -> impl IntoResponse {
    let path = params.path.unwrap_or_default();
    match state.memory.ls(&path).await {
        Ok(entries) => Json(json!({
            "status": "ok",
            "path": path,
            "entries": entries
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "status": "error",
                "message": e.to_string()
            })),
        )
            .into_response(),
    }
}

pub async fn cd_handler(
    State(state): State<CliState>,
    Json(params): Json<CdParams>,
) -> impl IntoResponse {
    // CD in the API context just validates if the path "exists" as a directory or document
    match state.memory.ls(&params.path).await {
        Ok(entries) => {
            if entries.is_empty() {
                // If it's empty, we should check if the path itself is a document
                match state.memory.get(&params.path).await {
                    Ok(Some(_)) => Json(json!({
                        "status": "ok",
                        "path": params.path,
                        "is_doc": true,
                        "is_dir": false
                    }))
                    .into_response(),
                    _ => (
                        StatusCode::NOT_FOUND,
                        Json(json!({
                            "status": "error",
                            "message": "Path not found"
                        })),
                    )
                        .into_response(),
                }
            } else {
                Json(json!({
                    "status": "ok",
                    "path": params.path,
                    "is_doc": false,
                    "is_dir": true
                }))
                .into_response()
            }
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "status": "error",
                "message": e.to_string()
            })),
        )
            .into_response(),
    }
}

pub async fn pwd_handler() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "message": "PWD is handled client-side in the CLI, but the API is ready for navigation."
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::rate_limit::RateLimitManager;
    use crate::app::proxy_use_case::ProxyUseCase;
    use crate::app::qmd_memory_adapter::QmdMemoryAdapter;
    use crate::cli::state::CliState;
    use crate::codebase::conversations_db::ConversationsDb;
    use crate::coordination::KeyLendingEngine;
    use crate::coordination::SimpleAgentRegistry;
    use crate::coordination::XavierEventBus;
    use crate::embedding::MockEmbedder;
    use crate::memory::agent_indexer::AgentIndexer;
    use crate::memory::file_indexer::{FileIndexer, FileIndexerConfig};
    use crate::memory::qmd_memory::QmdMemory;
    use crate::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
    use crate::ports::inbound::MemoryQueryPort;
    use crate::security::sessions::SessionManager;
    use crate::tasks::store::{InMemoryTaskStore, TaskService};
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::{get, post},
        Router,
    };
    use parking_lot::Mutex;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::RwLock as AsyncRwLock;
    use tower::util::ServiceExt;

    async fn test_state() -> CliState {
        use crate::agents::provider::router::{ProviderKind, ProviderRouter};
        use crate::app::security_service::SecurityService;
        use crate::memory::sqlite_vec_store::DEFAULT_EMBEDDING_DIMENSIONS;
        use crate::secrets::audit::QmdAuditLogger;

        let docs = Arc::new(AsyncRwLock::new(Vec::new()));
        let qmd_memory = Arc::new(QmdMemory::new_with_workspace(docs, "test-ws"));
        let memory_port = Arc::new(QmdMemoryAdapter::new(Arc::clone(&qmd_memory)));

        let config = VecSqliteStoreConfig {
            path: PathBuf::from(":memory:"),
            embedding_dimensions: DEFAULT_EMBEDDING_DIMENSIONS,
        };
        let store = Arc::new(VecSqliteMemoryStore::new(config).await.unwrap());

        CliState {
            memory: memory_port,
            qmd_memory,
            store: store.clone(),
            workspace_id: "test-ws".to_string(),
            workspace_dir: PathBuf::from("."),
            code_db: Arc::new(
                code_graph::db::CodeGraphDB::new(&PathBuf::from(":memory:")).unwrap(),
            ),
            code_indexer: Arc::new(code_graph::indexer::Indexer::new(Arc::new(
                code_graph::db::CodeGraphDB::new(&PathBuf::from(":memory:")).unwrap(),
            ))),
            code_query: Arc::new(code_graph::query::QueryEngine::new(Arc::new(
                code_graph::db::CodeGraphDB::new(&PathBuf::from(":memory:")).unwrap(),
            ))),
            security: Arc::new(SecurityService::new()),
            security_scan: Arc::new(SecurityService::new()),
            _time_store: None,
            agent_registry: Arc::new(SimpleAgentRegistry::new()),
            panel_store: Arc::new(
                ConversationsDb::open_in_memory("test-project")
                    .await
                    .unwrap(),
            ),
            secrets_engine: Arc::new(KeyLendingEngine::new(Box::new(QmdAuditLogger::new()))),
            event_bus: XavierEventBus::new(10),
            tasks: Arc::new(TaskService::new(Arc::new(InMemoryTaskStore::new()))),
            rate_manager: Arc::new(RateLimitManager::new()),
            prompt_cache: Arc::new(Mutex::new(HashMap::new())),
            http_client: reqwest::Client::new(),
            proxy_use_case: Arc::new(ProxyUseCase::new(
                Arc::new(RateLimitManager::new()),
                Arc::new(Mutex::new(HashMap::new())),
            )),
            session_manager: Arc::new(SessionManager::new(60)),
            provider_router: Arc::new(tokio::sync::RwLock::new(ProviderRouter::new(
                ProviderKind::OpenAI,
            ))),
            embedder: Arc::new(MockEmbedder::new()),
            agent_indexer: Arc::new(AgentIndexer::new(FileIndexer::new(
                FileIndexerConfig::default(),
                None,
            ))),
        }
    }

    #[tokio::test]
    async fn test_nav_api() {
        let state = test_state().await;
        state
            .qmd_memory
            .add_document("docs/test".to_string(), "content".to_string(), json!({}))
            .await
            .unwrap();

        let app = Router::new()
            .route("/v1/nav/ls", get(ls_handler))
            .route("/v1/nav/cd", post(cd_handler))
            .with_state(state);

        // Test LS
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/nav/ls?path=docs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Test CD (valid)
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/nav/cd")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"path": "docs"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Test CD (invalid)
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/nav/cd")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"path": "nonexistent"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
