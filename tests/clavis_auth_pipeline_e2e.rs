use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    middleware, Router,
};
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;

use xavier::agents::provider::router::{ProviderKind, ProviderRouter};
use xavier::agents::rate_limit::RateLimitManager;
use xavier::app::proxy_use_case::ProxyUseCase;
use xavier::app::qmd_memory_adapter::QmdMemoryAdapter;
use xavier::cli::http_setup::auth_middleware;
use xavier::cli::state::{CliState, CodeGraphState};
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
use xavier::security::auth::{User, UserRole};
use xavier::tasks::store::{InMemoryTaskStore, TaskService};

/// Minimal `CliState` fixture, mimicking `tests/test_store_path_hierarchy.rs` and `tests/test_maloca_auth_gate.rs`.
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

    let cg_db = Arc::new(code_graph::db::CodeGraphDB::in_memory().unwrap());
    let cg_state = Arc::new(tokio::sync::RwLock::new(CodeGraphState {
        db: cg_db.clone(),
        indexer: Arc::new(code_graph::indexer::Indexer::new(cg_db.clone())),
        query: Arc::new(code_graph::query::QueryEngine::new(cg_db)),
    }));

    CliState {
        memory: memory_port,
        qmd_memory,
        store,
        workspace_id: "test-ws".to_string(),
        workspace_dir: temp_dir.path().to_path_buf(),
        state_dir: temp_dir.path().to_path_buf(),
        auth_db: None,
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
        openclaw_indexer: Arc::new(xavier::memory::openclaw_indexer::OpenClawAgentIndexer::new(
            Arc::new(NoopEmbedder),
        )),
        multi_db: xavier::storage::multi_db::MultiDbManager::new(),
        system_scan_cache: Arc::new(tokio::sync::RwLock::new(None)),
        maloca: xavier::maloca::MalocaStore::open(&temp_dir.path().join("xavier-maloca")),
    }
}

/// Helper to execute requests against the router.
async fn run_request(
    router: &Router,
    token: Option<&str>,
    uri: &str,
    is_bearer: bool,
) -> axum::response::Response {
    let mut builder = Request::builder().uri(uri).method(Method::GET);
    if let Some(tok) = token {
        if is_bearer {
            builder = builder.header("Authorization", format!("Bearer {}", tok));
        } else {
            builder = builder.header("X-Xavier-Token", tok);
        }
    }
    let req = builder.body(Body::empty()).unwrap();
    // Using `.clone()` is necessary because `.oneshot()` consumes the router.
    router.clone().oneshot(req).await.unwrap()
}

/// Puts env vars back when the test ends, including on assertion failure.
struct RestoreEnv {
    saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl RestoreEnv {
    fn set(pairs: &[(&'static str, &std::path::Path)]) -> Self {
        let saved = pairs
            .iter()
            .map(|(key, value)| {
                let prev = std::env::var_os(key);
                std::env::set_var(key, value);
                (*key, prev)
            })
            .collect();
        Self { saved }
    }

    fn set_str(pairs: &[(&'static str, &str)]) -> Self {
        let saved = pairs
            .iter()
            .map(|(key, value)| {
                let prev = std::env::var_os(key);
                std::env::set_var(key, value);
                (*key, prev)
            })
            .collect();
        Self { saved }
    }
}

impl Drop for RestoreEnv {
    fn drop(&mut self) {
        for (key, prev) in self.saved.drain(..) {
            match prev {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    }
}

#[tokio::test]
async fn clavis_auth_pipeline_verifies_tokens_via_production_middleware() {
    let temp = tempfile::tempdir().unwrap();
    // Use the test isolation pattern.
    let _env = RestoreEnv::set(&[("HOME", temp.path()), ("XAVIER_DATA_DIR", temp.path())]);

    // Setup the correct expected root token.
    let expected_root_token = "e2e-root-token-9999";
    // Provide a push secret for minting a valid, but lower-privileged token.
    let push_secret = "push-secret-for-signing-jwt";
    let _env_str = RestoreEnv::set_str(&[
        ("XAVIER_TOKEN", expected_root_token),
        ("XAVIER_JWT_SECRET", push_secret),
    ]);

    let state = create_test_cli_state(&temp).await;

    // Build the router exactly as in `src/cli/server.rs` protected routes
    let app = xavier::cli::server::clavis_routes()
        .layer(middleware::from_fn(
            xavier::adapters::inbound::http::middleware::clearance::clearance_session_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .with_state(state.clone());

    let endpoint = "/v1/clavis/keys/test_key_protected";

    // 1. No token -> HTTP 401 Unauthorized
    let resp = run_request(&app, None, endpoint, false).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "Expected 401 for missing token"
    );

    // 2. Invalid token -> HTTP 401 Unauthorized
    let resp = run_request(&app, Some("invalid-token-123"), endpoint, false).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "Expected 401 for invalid token"
    );

    // 3. Valid token, but insufficient permissions -> HTTP 403 Forbidden
    // We sign a token as `UserRole::User` which lacks `can_manage_secrets()`.
    let user = User::new(
        "test_user".to_string(),
        "user@example.com".to_string(),
        UserRole::User,
    );
    let user_token = xavier::security::auth::generate_jwt(&user, push_secret.as_bytes()).unwrap();
    let resp = run_request(&app, Some(&user_token), endpoint, true).await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "Expected 403 for valid token lacking secrets permission"
    );

    // 4. Root token -> Passes `auth_middleware` and role gate.
    // It should hit the handler and return 404 NOT_FOUND since the key doesn't exist,
    // rather than 401 or 403.
    let resp = run_request(&app, Some(expected_root_token), endpoint, false).await;
    let status = resp.status();
    assert!(
        status != StatusCode::UNAUTHORIZED && status != StatusCode::FORBIDDEN,
        "Expected authorized user querying missing key to pass auth, got {}",
        status
    );
}

xavier::isolate_test_process!();
