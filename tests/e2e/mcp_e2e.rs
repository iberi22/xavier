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

use xavier::{
    agents::RuntimeConfig,
    memory::file_indexer::{FileIndexer, FileIndexerConfig},
    workspace::{WorkspaceConfig, WorkspaceContext, WorkspaceRegistry, WorkspaceState},
    AppState,
};

fn unique_test_path(prefix: &str, suffix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("SystemTime before UNIX_EPOCH")
        .as_nanos();
    let tid = std::thread::current().id();
    std::env::temp_dir().join(format!("{prefix}-{unique:016x}-{tid:?}-{suffix}"))
}

async fn test_state() -> (AppState, WorkspaceContext) {
    std::env::set_var("XAVIER_TOKEN", "e2e-mcp-token");
    let db_path = unique_test_path("xavier-e2e-mcp", "code_graph.db");
    let code_db = Arc::new(
        code_graph::db::CodeGraphDB::new(&db_path).expect("CodeGraphDB creation failed for test"),
    );
    let code_indexer = Arc::new(code_graph::indexer::Indexer::new(Arc::clone(&code_db)));
    let code_query = Arc::new(code_graph::query::QueryEngine::new(Arc::clone(&code_db)));
    let workspace_registry = Arc::new(WorkspaceRegistry::new());
    let workspace = WorkspaceState::new(
        WorkspaceConfig {
            id: format!("test-e2e-{}", ulid::Ulid::new()),
            token: "e2e-mcp-token".to_string(),
            plan: xavier::workspace::PlanTier::Personal,
            memory_backend: xavier::memory::store::MemoryBackend::File,
            storage_limit_bytes: Some(10 * 1024 * 1024),
            request_limit: Some(10_000),
            request_unit_limit: Some(20_000),
            embedding_provider_mode: xavier::workspace::EmbeddingProviderMode::BringYourOwn,
            managed_google_embeddings: false,
            sync_policy: xavier::workspace::SyncPolicy::CloudMirror,
            dedup: xavier::settings::types::DedupSettings::default(),
        },
        RuntimeConfig::default(),
        unique_test_path("xavier-e2e-mcp-store", "threads"),
    )
    .await
    .expect("WorkspaceState creation failed for test");
    workspace_registry
        .insert(workspace)
        .await
        .expect("insert workspace into registry failed");
    let workspace = workspace_registry
        .authenticate("e2e-mcp-token")
        .await
        .expect("authenticate failed for test workspace");

    (
        AppState {
            workspace_registry,
            indexer: FileIndexer::new(FileIndexerConfig::default(), Some(code_indexer.clone())),
            agent_indexer: xavier::memory::agent_indexer::AgentIndexer::new(FileIndexer::new(
                FileIndexerConfig::default(),
                Some(code_indexer.clone()),
            )),
            code_indexer,
            code_query,
            code_db,
            security_service: Arc::new(xavier::app::security_service::SecurityService::new()),
            code_graph_dump_path: None,
        },
        workspace,
    )
}

fn test_router(state: AppState, workspace: WorkspaceContext) -> Router {
    Router::new()
        .route("/mcp", post(xavier::server::mcp::session::mcp_post_handler))
        .layer(axum::middleware::from_fn(
            xavier::server::mcp::auth::mcp_auth_middleware,
        ))
        .layer(axum::Extension(workspace))
        .with_state(state)
}

async fn post_json_with_token(
    app: Router,
    body: Value,
    token: Option<&str>,
) -> axum::response::Response {
    let method = body
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("initialize");

    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/mcp")
        .header("content-type", "application/json")
        .header("Origin", "http://localhost:8080")
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", method);

    if method == "tools/call" {
        if let Some(name) = body
            .get("params")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
        {
            req = req.header("Mcp-Name", name);
        }
    }

    if let Some(tok) = token {
        req = req.header("X-Xavier-Token", tok);
    }

    app.oneshot(
        req.body(Body::from(
            serde_json::to_vec(&body).expect("serialize json body failed"),
        ))
        .expect("build POST request failed"),
    )
    .await
    .expect("POST request to MCP endpoint failed")
}

async fn post_json(app: Router, body: Value) -> axum::response::Response {
    post_json_with_token(app, body, Some("e2e-mcp-token")).await
}

async fn get_json_body(response: axum::response::Response) -> Value {
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body failed");
    serde_json::from_slice(&body_bytes).expect("parse JSON response body failed")
}

#[tokio::test]
async fn test_mcp_e2e_handshake_and_tools_list() {
    let (state, workspace) = test_state().await;
    let router = test_router(state, workspace);

    // 0. Verify Auth Enforcement: Request without token returns 401 Unauthorized
    let unauth_response = post_json_with_token(
        router.clone(),
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list"
        }),
        None,
    )
    .await;
    assert_eq!(unauth_response.status(), StatusCode::UNAUTHORIZED);

    // Request with invalid token returns 401 Unauthorized
    let invalid_token_response = post_json_with_token(
        router.clone(),
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list"
        }),
        Some("invalid-token"),
    )
    .await;
    assert_eq!(invalid_token_response.status(), StatusCode::UNAUTHORIZED);

    // 1. Handshake with protocol-version 2026-07-28
    let response = post_json(
        router.clone(),
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2026-07-28",
                "capabilities": { "tools": {} },
                "clientInfo": { "name": "e2e-test", "version": "1.0" }
            }
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = get_json_body(response).await;
    assert_eq!(body["result"]["protocolVersion"], "2026-07-28");
    assert_eq!(body["result"]["serverInfo"]["name"], "xavier-memory");

    // 2. List tools
    let response = post_json(
        router,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = get_json_body(response).await;
    let tools = body["result"]["tools"]
        .as_array()
        .expect("tools should be an array");
    assert!(
        tools.len() >= 15,
        "Expected at least 15 tools, found {}",
        tools.len()
    );
}

#[tokio::test]
async fn test_mcp_e2e_health_check() {
    let (state, workspace) = test_state().await;
    let router = test_router(state, workspace);

    let response = post_json(
        router,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "health_check",
                "arguments": {}
            }
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = get_json_body(response).await;
    let content = body["result"]["content"]
        .as_array()
        .expect("content should be an array");
    assert!(!content.is_empty());

    // Check structured content element
    let structured_item = content
        .iter()
        .find(|c| c["type"] == "structuredContent")
        .expect("structuredContent item should exist in response content");
    assert!(structured_item["structuredContent"]["status"].is_string());
    assert_eq!(structured_item["structuredContent"]["handshakeOk"], true);
}
