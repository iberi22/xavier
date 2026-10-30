use axum::{
    body::Body,
    http::{Method, Request},
};
use serde_json::{json, Value};
use serial_test::serial;
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;

use xavier::app::security_service::SecurityService;
use xavier::workspace::{WorkspaceConfig, WorkspaceRegistry, WorkspaceState};
use xavier::AppState;

xavier::isolate_test_process!();

struct EnvGuard {
    xavier_token: Option<std::ffi::OsString>,
    xavier_jwt_secret: Option<std::ffi::OsString>,
    xavier_memory_vec_path: Option<std::ffi::OsString>,
    xavier_code_graph_db_path: Option<std::ffi::OsString>,
    xavier_embedder: Option<std::ffi::OsString>,
    xavier_memory_file_path: Option<std::ffi::OsString>,
    xavier_file_store_path: Option<std::ffi::OsString>,
    xavier_memory_sqlite_path: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn new() -> Self {
        Self {
            xavier_token: std::env::var_os("XAVIER_TOKEN"),
            xavier_jwt_secret: std::env::var_os("XAVIER_JWT_SECRET"),
            xavier_memory_vec_path: std::env::var_os("XAVIER_MEMORY_VEC_PATH"),
            xavier_code_graph_db_path: std::env::var_os("XAVIER_CODE_GRAPH_DB_PATH"),
            xavier_embedder: std::env::var_os("XAVIER_EMBEDDER"),
            xavier_memory_file_path: std::env::var_os("XAVIER_MEMORY_FILE_PATH"),
            xavier_file_store_path: std::env::var_os("XAVIER_FILE_STORE_PATH"),
            xavier_memory_sqlite_path: std::env::var_os("XAVIER_MEMORY_SQLITE_PATH"),
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        let vars = [
            ("XAVIER_TOKEN", &self.xavier_token),
            ("XAVIER_JWT_SECRET", &self.xavier_jwt_secret),
            ("XAVIER_MEMORY_VEC_PATH", &self.xavier_memory_vec_path),
            ("XAVIER_CODE_GRAPH_DB_PATH", &self.xavier_code_graph_db_path),
            ("XAVIER_EMBEDDER", &self.xavier_embedder),
            ("XAVIER_MEMORY_FILE_PATH", &self.xavier_memory_file_path),
            ("XAVIER_FILE_STORE_PATH", &self.xavier_file_store_path),
            ("XAVIER_MEMORY_SQLITE_PATH", &self.xavier_memory_sqlite_path),
        ];
        for (key, val) in vars {
            if let Some(v) = val {
                std::env::set_var(key, v);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}

#[tokio::test]
#[serial]
async fn default_server_hides_telecom_tools() {
    let _env_guard = EnvGuard::new();
    let temp = TempDir::new().expect("tempdir");

    let vec_path = temp.path().join("vec.sqlite3");
    let cg_path = temp.path().join("code_graph.db");

    std::env::set_var("XAVIER_TOKEN", "test-token");
    std::env::set_var("XAVIER_JWT_SECRET", "test-secret");
    std::env::set_var("XAVIER_MEMORY_VEC_PATH", vec_path.to_str().unwrap());
    std::env::set_var("XAVIER_CODE_GRAPH_DB_PATH", cg_path.to_str().unwrap());
    std::env::set_var("XAVIER_EMBEDDER", "disabled");
    std::env::set_var(
        "XAVIER_MEMORY_FILE_PATH",
        temp.path().join("memory.json").to_str().unwrap(),
    );
    std::env::set_var(
        "XAVIER_FILE_STORE_PATH",
        temp.path().join("file_store").to_str().unwrap(),
    );
    std::env::set_var(
        "XAVIER_MEMORY_SQLITE_PATH",
        temp.path().join("memory.sqlite3").to_str().unwrap(),
    );

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

    let payload = json!({
        "jsonrpc": "2.0",
        "method": "tools/list",
        "id": 1
    });

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
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");

    let json_rpc_response: Value = serde_json::from_slice(&bytes).expect("json");

    let tools_array = json_rpc_response
        .get("result")
        .expect("missing result")
        .get("tools")
        .expect("missing tools array")
        .as_array()
        .expect("tools is not an array");

    let telecom_tool = tools_array.iter().find(|t| {
        let name = t["name"].as_str().unwrap_or("");
        name.starts_with("telecom_")
    });

    assert!(
        telecom_tool.is_none(),
        "Telecom tools must not be advertised when telecom backend is absent"
    );
}
