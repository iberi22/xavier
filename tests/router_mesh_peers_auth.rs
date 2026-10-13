use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;
use xavier::agents::provider::router::{ProviderKind, ProviderRouter};
use xavier::agents::rate_limit::RateLimitManager;
use xavier::app::proxy_use_case::ProxyUseCase;
use xavier::app::qmd_memory_adapter::QmdMemoryAdapter;
use xavier::cli::server::{build_app, AppDeps};
use xavier::cli::state::{CliState, CodeGraphState};
use xavier::codebase::conversations_db::ConversationsDb;
use xavier::coordination::KeyLendingEngine;
use xavier::coordination::SimpleAgentRegistry;
use xavier::embedding::NoopEmbedder;
use xavier::memory::file_indexer::{FileIndexer, FileIndexerConfig};
use xavier::memory::qmd_memory::QmdMemory;
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use xavier::secrets::audit::QmdAuditLogger;
use xavier::security::auth::{User, UserRole};
use xavier::tasks::store::{InMemoryTaskStore, TaskService};

xavier::isolate_test_process!();

/// Minimal `CliState` fixture, mimicking `tests/production_router_smoke.rs` and `tests/clavis_auth_pipeline_e2e.rs`.
async fn create_test_cli_state(temp_dir: &TempDir) -> CliState {
    let docs = Arc::new(tokio::sync::RwLock::new(Vec::new()));
    let qmd_memory = Arc::new(QmdMemory::new_with_workspace(docs, "test-ws"));
    let memory_port = Arc::new(QmdMemoryAdapter::new(Arc::clone(&qmd_memory)));

    let db_path = temp_dir.path().join("test_store.db");
    let store_config = VecSqliteStoreConfig {
        path: db_path,
        embedding_dimensions: 1536,
    };
    let store = Arc::new(VecSqliteMemoryStore::new(store_config).await.unwrap());

    let state = CliState {
        auth_context: None,
        config: xavier::cli::config::CliConfig::default(),
        memory: Some(memory_port),
        mcp_servers: Vec::new(),
        skills_cache: xavier::memory::skills_cache::SkillsCache::new(),
        qmd: Some(qmd_memory),
        env_vars: std::collections::HashMap::new(),
        code_graph: CodeGraphState::default(),
        workspaces: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        agent_registry: Arc::new(SimpleAgentRegistry::new()),
        vec_store: Some(store),
        push_secret: Some("test-push-secret".to_string()),
        agent_identity: None,
        mesh_key: None,
    };
    state
}

#[tokio::test]
async fn test_mesh_peers_auth_matrix() {
    let temp_dir = TempDir::new().unwrap();
    let state = create_test_cli_state(&temp_dir).await;

    // Build the exact AppDeps as expected in `build_app`.
    let db_path = temp_dir.path().join("conversations.db");
    let conn = xavier::codebase::conversations_db::init_db(db_path.to_str().unwrap()).unwrap();
    let db = Arc::new(ConversationsDb::new(conn));
    let task_store = Arc::new(InMemoryTaskStore::new());
    let task_service = Arc::new(TaskService::new(task_store));

    let db_path_file = temp_dir.path().join("file_indexer.db");
    let file_config = FileIndexerConfig {
        db_path: db_path_file,
    };
    let file_indexer = Arc::new(FileIndexer::new(file_config).await.unwrap());

    let embedder = Arc::new(NoopEmbedder);
    let proxy = Arc::new(ProxyUseCase::new(
        state.agent_registry.clone(),
        state.workspaces.clone(),
    ));

    let audit_logger = Arc::new(QmdAuditLogger::new(
        state.qmd.as_ref().unwrap().clone(),
        "test-ws".to_string(),
    ));
    let key_lender = Arc::new(KeyLendingEngine::new(audit_logger));

    let rate_limiter = Arc::new(RateLimitManager::new());

    let deps = AppDeps {
        conversations_db: db,
        task_service,
        file_indexer,
        embedder,
        proxy_use_case: proxy,
        key_lending_engine: key_lender,
        rate_limiter,
        maloca_router: axum::Router::new(),
        event_bus_for_ws: xavier::coordination::XavierEventBus::new(10),
        no_ui: true,
    };

    let app = build_app(state.clone(), deps);

    // 1. Matrix test: 401 Unauthorized for mutating routes without token
    let routes_401 = vec![
        (Method::POST, "/v1/mesh/peers"),
        (Method::POST, "/v1/mesh/peers/pair"),
        (Method::POST, "/v1/mesh/peers/decode"),
        (Method::POST, "/v1/mesh/peers/generate-code"),
        (Method::PUT, "/v1/mesh/peers/some_node_id/acl"),
        (Method::DELETE, "/v1/mesh/peers/some_node_id"),
        (Method::POST, "/v1/mesh/workspaces/share"),
        (Method::POST, "/v1/mesh/workspaces/join"),
        (Method::POST, "/v1/mesh/workspaces/query"),
        (Method::POST, "/v1/mesh/consent/revoke"),
        (Method::POST, "/v1/mesh/bridges"),
    ];

    for (method, path) in routes_401 {
        let req = Request::builder()
            .uri(path)
            .method(method.clone())
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "Expected 401 for {} {} without auth token",
            method,
            path
        );
    }

    // 2. Matrix test: 403 Forbidden for config mutating routes with User token (UserRole::User)
    let user = User::new(
        "test_user".to_string(),
        "user@example.com".to_string(),
        UserRole::User,
    );
    let user_token =
        xavier::security::auth::generate_jwt(&user, state.push_secret.as_ref().unwrap().as_bytes())
            .unwrap();

    let routes_403 = vec![
        (Method::POST, "/v1/mesh/peers"),
        (Method::PUT, "/v1/mesh/peers/some_node_id/acl"),
        (Method::DELETE, "/v1/mesh/peers/some_node_id"),
    ];

    for (method, path) in routes_403 {
        let req = Request::builder()
            .uri(path)
            .method(method.clone())
            .header("Authorization", format!("Bearer {}", user_token))
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "Expected 403 for {} {} with regular User token",
            method,
            path
        );
    }
}
