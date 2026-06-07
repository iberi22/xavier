//! Tests for MCP server
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    routing::post,
    Router,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tower::util::ServiceExt;

use super::session::mcp_delete_handler;
use super::session::mcp_get_handler;
use super::session::mcp_post_handler;
use crate::workspace::WorkspaceContext;
use crate::{
    agents::RuntimeConfig,
    memory::file_indexer::{FileIndexer, FileIndexerConfig},
    workspace::{WorkspaceConfig, WorkspaceRegistry, WorkspaceState},
    AppState,
};

const MCP_SESSION_HEADER: &str = "mcp-session-id";

fn unique_test_path(prefix: &str, suffix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("SystemTime before UNIX_EPOCH")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{unique}-{suffix}"))
}

async fn test_state() -> (AppState, WorkspaceContext) {
    let db_path = unique_test_path("xavier-code-mcp", "code_graph.db");
    let code_db = Arc::new(
        code_graph::db::CodeGraphDB::new(&db_path).expect("CodeGraphDB creation failed for test"),
    );
    let code_indexer = Arc::new(code_graph::indexer::Indexer::new(Arc::clone(&code_db)));
    let code_query = Arc::new(code_graph::query::QueryEngine::new(Arc::clone(&code_db)));
    let workspace_registry = Arc::new(WorkspaceRegistry::new());
    let workspace = WorkspaceState::new(
        WorkspaceConfig {
            id: "test".to_string(),
            token: "test-token".to_string(),
            plan: crate::workspace::PlanTier::Personal,
            memory_backend: crate::memory::store::MemoryBackend::File,
            storage_limit_bytes: Some(10 * 1024 * 1024),
            request_limit: Some(10_000),
            request_unit_limit: Some(20_000),
            embedding_provider_mode: crate::workspace::EmbeddingProviderMode::BringYourOwn,
            managed_google_embeddings: false,
            sync_policy: crate::workspace::SyncPolicy::CloudMirror,
        },
        RuntimeConfig::default(),
        unique_test_path("xavier-mcp-store", "threads"),
    )
    .await
    .expect("WorkspaceState creation failed for test");
    workspace_registry
        .insert(workspace)
        .await
        .expect("insert workspace into registry failed");
    let workspace = workspace_registry
        .authenticate("test-token")
        .await
        .expect("authenticate failed for test workspace");

    (
        AppState {
            workspace_registry,
            indexer: FileIndexer::new(FileIndexerConfig::default(), Some(code_indexer.clone())),
            agent_indexer: crate::memory::agent_indexer::AgentIndexer::new(FileIndexer::new(
                FileIndexerConfig::default(),
                Some(code_indexer.clone()),
            )),
            code_indexer,
            code_query,
            code_db,
            security_service: Arc::new(crate::app::security_service::SecurityService::new()),
        },
        workspace,
    )
}

fn test_router(state: AppState, workspace: WorkspaceContext) -> Router {
    Router::new()
        .route(
            "/mcp",
            post(mcp_post_handler)
                .get(mcp_get_handler)
                .delete(mcp_delete_handler),
        )
        .layer(axum::Extension(workspace))
        .with_state(state)
}

async fn post_json(app: Router, body: Value) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method(Method::POST)
            .uri("/mcp")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&body).expect("serialize json body failed"),
            ))
            .expect("build POST request failed"),
    )
    .await
    .expect("POST request to MCP endpoint failed")
}

#[tokio::test]
async fn initialize_returns_current_protocol_version() {
    let (state, workspace) = test_state().await;
    let response = post_json(test_router(state, workspace), json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26", "capabilities": { "tools": {} }, "clientInfo": { "name": "test", "version": "1.0" } } })).await;
    assert_eq!(response.status(), StatusCode::OK);
}
