use axum::body::to_bytes;
use axum::{
    body::Body,
    http::{Method, Request},
};
use serde_json::Value;
use serial_test::serial;
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;

use xavier::app::security_service::SecurityService;
use xavier::workspace::{WorkspaceConfig, WorkspaceRegistry, WorkspaceState};
use xavier::AppState;

xavier::isolate_test_process!();

struct EnvGuard {
    vars: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn new(keys: &[&'static str]) -> Self {
        let vars = keys.iter().map(|&k| (k, std::env::var(k).ok())).collect();
        Self { vars }
    }

    fn set(&self, key: &'static str, value: &str) {
        std::env::set_var(key, value);
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (k, v) in &self.vars {
            if let Some(v) = v {
                std::env::set_var(k, v);
            } else {
                std::env::remove_var(k);
            }
        }
    }
}

#[tokio::test]
#[serial]
async fn mcp_endpoints_expose_same_tool_list() {
    let temp = TempDir::new().expect("tempdir");

    let vec_path = temp.path().join("vec.sqlite3");
    let cg_path = temp.path().join("code_graph.db");

    let _guard = EnvGuard::new(&[
        "XAVIER_TOKEN",
        "XAVIER_JWT_SECRET",
        "XAVIER_MEMORY_VEC_PATH",
        "XAVIER_CODE_GRAPH_DB_PATH",
        "XAVIER_EMBEDDER",
    ]);

    _guard.set("XAVIER_TOKEN", "test-token");
    _guard.set("XAVIER_JWT_SECRET", "test-secret");
    _guard.set("XAVIER_MEMORY_VEC_PATH", vec_path.to_str().unwrap());
    _guard.set("XAVIER_CODE_GRAPH_DB_PATH", cg_path.to_str().unwrap());
    _guard.set("XAVIER_EMBEDDER", "disabled");

    // Explicitly construct the stores to avoid env var overrides and cwd mutation.
    let security_service = Arc::new(SecurityService::new());
    let workspace_registry = Arc::new(WorkspaceRegistry::new());

    let cg_db = Arc::new(code_graph::db::CodeGraphDB::in_memory().unwrap());
    let code_indexer = Arc::new(code_graph::indexer::Indexer::new(Arc::clone(&cg_db)));
    let code_query = Arc::new(code_graph::query::QueryEngine::new(Arc::clone(&cg_db)));

    let state = AppState {
        workspace_registry: Arc::clone(&workspace_registry),
        code_indexer: Arc::clone(&code_indexer),
        code_query,
        code_db: cg_db,
        indexer: xavier::memory::file_indexer::FileIndexer::new(
            xavier::memory::file_indexer::FileIndexerConfig::default(),
            Some(code_indexer.clone()),
        ),
        agent_indexer: xavier::memory::agent_indexer::AgentIndexer::new(
            xavier::memory::file_indexer::FileIndexer::new(
                xavier::memory::file_indexer::FileIndexerConfig::default(),
                Some(code_indexer.clone()),
            ),
        ),
        security_service,
        code_graph_dump_path: None,
        telecom: None,
    };

    // Disable embedder inside WorkspaceState::new to avoid HTTP network calls
    let workspace = WorkspaceState::new(
        WorkspaceConfig {
            id: "default".to_string(),
            token: "mcp-token".to_string(),
            plan: xavier::workspace::PlanTier::Personal,
            memory_backend: xavier::memory::store::MemoryBackend::Vec,
            storage_limit_bytes: None,
            request_limit: None,
            request_unit_limit: None,
            embedding_provider_mode: xavier::workspace::EmbeddingProviderMode::BringYourOwn,
            managed_google_embeddings: false,
            sync_policy: xavier::workspace::SyncPolicy::CloudMirror,
            protocol: Default::default(),
            dedup: xavier::settings::types::DedupSettings::default(),
        },
        xavier::agents::RuntimeConfig::default(),
        temp.path().join("xavier-mcp"),
    )
    .await
    .expect("WorkspaceState::new failed");

    workspace_registry.insert(workspace).await.unwrap();
    let workspace = workspace_registry
        .authenticate("mcp-token")
        .await
        .expect("authenticate workspace");

    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "tools/list",
        "id": 1
    });

    // 1. Simulate the JSON-RPC POST /mcp tools/list handler
    let app =
        xavier::server::mcp::transport::build_mcp_http_router(state.clone(), workspace.clone());

    let user = xavier::security::auth::User::new(
        "test-sub".to_string(),
        "test@example.com".to_string(),
        xavier::security::auth::UserRole::Admin,
    );
    let token = xavier::security::auth::generate_jwt(&user, b"test-secret").expect("jwt");

    let req = Request::builder()
        .uri("/mcp")
        .method(Method::POST)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", token))
        .header("Origin", "http://localhost")
        .body(Body::from(payload.to_string()))
        .expect("request");

    let response = app.oneshot(req).await.expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");

    assert!(
        status.is_success(),
        "HTTP request failed: {:?} - body: {:?}",
        status,
        String::from_utf8_lossy(&bytes)
    );

    let json_rpc_response: Value = serde_json::from_slice(&bytes).expect("json");

    // 2. Simulate the stdio / xavier mcp tools/list path
    let stdio_response = xavier::server::mcp_stdio::dispatch_stdio_value(
        state.clone(),
        workspace.clone(),
        payload.clone(),
    )
    .await
    .expect("dispatch stdio")
    .expect("expected response");

    // Extract tool names
    fn extract_tool_names(response: &Value) -> Vec<String> {
        let tools_array = response
            .get("result")
            .expect("missing result")
            .get("tools")
            .expect("missing tools array")
            .as_array()
            .expect("tools is not an array");

        let mut names: Vec<String> = tools_array
            .iter()
            .map(|t| {
                t.get("name")
                    .expect("missing name")
                    .as_str()
                    .expect("name is not string")
                    .to_string()
            })
            .collect();
        assert!(!names.is_empty(), "Tool list is empty");

        let initial_len = names.len();
        names.sort();
        names.dedup();
        assert_eq!(initial_len, names.len(), "Tool list contains duplicates");

        names
    }

    let json_rpc_names = extract_tool_names(&json_rpc_response);
    let stdio_names = extract_tool_names(&stdio_response);

    // 3. Compare the sets
    let mut missing_in_rpc = Vec::new();
    for name in &stdio_names {
        if !json_rpc_names.contains(name) {
            missing_in_rpc.push(name.clone());
        }
    }

    let mut missing_in_stdio = Vec::new();
    for name in &json_rpc_names {
        if !stdio_names.contains(name) {
            missing_in_stdio.push(name.clone());
        }
    }

    let mut error_msg = String::new();
    if !missing_in_rpc.is_empty() {
        error_msg.push_str(&format!(
            "Tools missing from JSON-RPC MCP endpoint: {:?}
",
            missing_in_rpc
        ));
    }
    if !missing_in_stdio.is_empty() {
        error_msg.push_str(&format!(
            "Tools missing from stdio MCP endpoint: {:?}
",
            missing_in_stdio
        ));
    }

    assert!(
        error_msg.is_empty(),
        "Tool lists differ between endpoints!
{}",
        error_msg
    );
}
