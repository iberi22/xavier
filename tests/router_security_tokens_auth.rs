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
use xavier::security::auth::{User, UserRole};
use xavier::tasks::store::{InMemoryTaskStore, TaskService};
use xavier::workspace::{WorkspaceContext, WorkspaceState};

/// Minimal `CliState` fixture, matching `tests/production_router_smoke.rs`.
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
        auth_store: Some(Arc::new(
            xavier::security::auth_store::AuthStore::open(":memory:").unwrap(),
        )),
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
    method: Method,
    token: Option<&str>,
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
    // We send an empty JSON object because some routes might be POST/DELETE and require it
    let req = builder
        .header("Content-Type", "application/json")
        .body(Body::empty())
        .unwrap();
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
async fn router_security_tokens_auth_matrix() {
    let temp = tempfile::tempdir().unwrap();
    let _env = RestoreEnv::set(&[("HOME", temp.path()), ("XAVIER_DATA_DIR", temp.path())]);

    let push_secret = "push-secret-for-signing-jwt";
    let _env_str = RestoreEnv::set_str(&[
        ("XAVIER_TOKEN", "prod-root-token"),
        ("XAVIER_JWT_SECRET", push_secret),
    ]);

    let state = create_test_cli_state(&temp).await;

    // Initialize TokenStore and create a scoped token
    let token_store = xavier::security::tokens::TokenStore::new();
    token_store.init_schema_async().await.unwrap();
    let (xav_token, _) = token_store
        .create_token("test_token".into(), vec!["all".into()], None)
        .await
        .unwrap();

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

    let app = build_app(state.clone(), deps);

    let endpoints = vec![
        (Method::GET, "/security/tokens"),
        (Method::POST, "/security/tokens"),
        (Method::DELETE, "/security/tokens/123"),
        (Method::POST, "/security/tokens/123/rotate"),
        (Method::GET, "/v1/auth/sessions"),
        (Method::DELETE, "/v1/auth/sessions/123"),
    ];

    let user = User::new(
        "test_user".to_string(),
        "user@example.com".to_string(),
        UserRole::User,
    );
    let user_jwt = xavier::security::auth::generate_jwt(&user, push_secret.as_bytes()).unwrap();

    for (method, path) in endpoints {
        // 1. No token -> 401 on all
        let resp = run_request(&app, method.clone(), None, path, false).await;
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "Expected 401 for missing token on {} {}",
            method,
            path
        );

        // 2. User JWT -> 403 on create/delete (POST/DELETE).
        let resp_user = run_request(&app, method.clone(), Some(&user_jwt), path, true).await;
        if method == Method::POST || method == Method::DELETE {
            assert_eq!(
                resp_user.status(),
                StatusCode::FORBIDDEN,
                "Expected 403 for User JWT on create/delete {} {}",
                method,
                path
            );
        }

        // 3. Non-root scoped token -> 403 on root-only prefix
        let resp_scoped = run_request(&app, method.clone(), Some(&xav_token), path, false).await;
        if path.starts_with("/security/tokens") {
            assert_eq!(
                resp_scoped.status(),
                StatusCode::FORBIDDEN,
                "Expected 403 for scoped token on root-only prefix {} {}",
                method,
                path
            );
        }
    }
}
