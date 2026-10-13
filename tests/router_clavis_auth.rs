xavier::isolate_test_process!();

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;

use xavier::adapters::inbound::http::handlers::clavis::set_clavis_vault;
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
use xavier::memory::agent_indexer::AgentIndexer;
use xavier::memory::file_indexer::{FileIndexer, FileIndexerConfig};
use xavier::memory::qmd_memory::QmdMemory;
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use xavier::ports::inbound::AgentLifecyclePort;
use xavier::secrets::audit::QmdAuditLogger;
use xavier::secrets::vault::HardwareVault;
use xavier::security::auth::{User, UserRole};
use xavier::tasks::store::{InMemoryTaskStore, TaskService};
use xavier::workspace::{WorkspaceContext, WorkspaceState};

/// Minimal `CliState` fixture, matching `tests/clavis_auth_pipeline_e2e.rs`.
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

    let auth_store = xavier::security::tokens::TokenStore::with_project_id("test_auth_store");
    auth_store.init_schema_async().await.unwrap();

    CliState {
        memory: memory_port,
        qmd_memory,
        store,
        workspace_id: "test-ws".to_string(),
        workspace_dir: temp_dir.path().to_path_buf(),
        state_dir: temp_dir.path().to_path_buf(),
        auth_db: Some(Arc::new(auth_store)),
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
    method: Method,
    uri: &str,
    is_bearer: bool,
) -> axum::response::Response {
    let mut builder = Request::builder().uri(uri).method(method);
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
async fn clavis_auth_tests_on_production_router() {
    let temp = tempfile::tempdir().unwrap();
    let _env = RestoreEnv::set(&[("HOME", temp.path()), ("XAVIER_DATA_DIR", temp.path())]);

    // Setup hermetic clavis vault
    let vault = Arc::new(HardwareVault::new("test-only-router").isolated(&temp, [0u8; 32]));
    set_clavis_vault(vault);

    let expected_root_token = "e2e-root-token-9999";
    let push_secret = "push-secret-for-signing-jwt";
    let _env_str = RestoreEnv::set_str(&[
        ("XAVIER_TOKEN", expected_root_token),
        ("XAVIER_JWT_SECRET", push_secret),
    ]);

    let state = create_test_cli_state(&temp).await;
    let workspace_state = Arc::new(
        WorkspaceState::new_minimal(
            state.workspace_id.clone(),
            state.workspace_dir.clone(),
            Arc::clone(&state.qmd_memory),
            Arc::clone(&state.store),
        )
        .await,
    );
    let workspace_ctx = WorkspaceContext {
        workspace_id: state.workspace_id.clone(),
        workspace: workspace_state,
    };

    let deps = AppDeps {
        workspace_ctx,
        space_manager: None,
        maloca_router: Router::new(),
        event_bus_for_ws: xavier::coordination::XavierEventBus::new(10),
        no_ui: true,
    };

    // Create xav_ token
    let auth_store = state.auth_db.as_ref().unwrap();
    let (xav_token, _) = auth_store
        .create_token("test_xav_token".to_string(), vec!["all".to_string()], None)
        .await
        .unwrap();

    let app = build_app(state.clone(), deps);

    let test_cases = vec![
        (Method::GET, "/v1/clavis/keys/nonexistent_key_123"),
        (Method::PUT, "/v1/clavis/keys/nonexistent_key_123"),
        (Method::POST, "/v1/clavis/proxy"),
    ];

    let user_token = xavier::security::auth::generate_jwt(
        &User::new(
            "test_user".to_string(),
            "user@example.com".to_string(),
            UserRole::User,
        ),
        push_secret.as_bytes(),
    )
    .unwrap();

    let readonly_token = xavier::security::auth::generate_jwt(
        &User::new(
            "test_readonly".to_string(),
            "readonly@example.com".to_string(),
            UserRole::Readonly,
        ),
        push_secret.as_bytes(),
    )
    .unwrap();

    let admin_token = xavier::security::auth::generate_jwt(
        &User::new(
            "test_admin".to_string(),
            "admin@example.com".to_string(),
            UserRole::Admin,
        ),
        push_secret.as_bytes(),
    )
    .unwrap();

    for (method, path) in test_cases {
        // 1. No token -> 401
        let resp = run_request(&app, None, method.clone(), path, false).await;
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "{} {} no token -> 401",
            method,
            path
        );

        // 2. User -> 403
        let resp = run_request(&app, Some(&user_token), method.clone(), path, true).await;
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "{} {} user token -> 403",
            method,
            path
        );

        // 3. Readonly -> 403
        let resp = run_request(&app, Some(&readonly_token), method.clone(), path, true).await;
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "{} {} readonly token -> 403",
            method,
            path
        );

        // 4. scoped xav_ token -> 403
        let resp = run_request(&app, Some(&xav_token), method.clone(), path, true).await;
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "{} {} xav_ token -> 403",
            method,
            path
        );

        // 5. Admin JWT -> neither 401 nor 403
        let resp = run_request(&app, Some(&admin_token), method.clone(), path, true).await;
        let status = resp.status();
        assert!(
            status != StatusCode::UNAUTHORIZED && status != StatusCode::FORBIDDEN,
            "{} {} Admin token should not be 401/403, got {}",
            method,
            path,
            status
        );

        // 6. Root token -> neither 401 nor 403
        let resp = run_request(&app, Some(expected_root_token), method.clone(), path, false).await;
        let status = resp.status();
        assert!(
            status != StatusCode::UNAUTHORIZED && status != StatusCode::FORBIDDEN,
            "{} {} Root token should not be 401/403, got {}",
            method,
            path,
            status
        );
    }
}
