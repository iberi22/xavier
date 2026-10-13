//! Shared harness for request-level auth tests against the production router.
//!
//! Every `tests/router_*_auth.rs` builds the REAL router through
//! `xavier::cli::server::build_app` with this fixture instead of hand-writing
//! a `CliState` (field lists drift; a copy that compiles is the contract).
#![allow(dead_code)]

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

pub const ROOT_TOKEN: &str = "router-harness-root-token-4242";
pub const JWT_SECRET: &str = "router-harness-jwt-secret";

/// Minimal `CliState` fixture, matching `tests/clavis_auth_pipeline_e2e.rs`.
pub async fn create_test_cli_state(temp_dir: &TempDir) -> CliState {
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

/// Puts env vars back when the test ends, including on assertion failure.
pub struct RestoreEnv {
    saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl RestoreEnv {
    pub fn set(pairs: &[(&'static str, &std::path::Path)]) -> Self {
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

    pub fn set_str(pairs: &[(&'static str, &str)]) -> Self {
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

/// The production router plus the guards that must outlive it.
pub struct TestApp {
    pub app: Router,
    _env: RestoreEnv,
    _env_str: RestoreEnv,
    _temp: TempDir,
}

/// Build the production router (`build_app`) over a tempdir, with
/// `XAVIER_TOKEN = ROOT_TOKEN` and `XAVIER_JWT_SECRET = JWT_SECRET`.
pub async fn test_app() -> TestApp {
    let temp = tempfile::tempdir().expect("tempdir");
    let env = RestoreEnv::set(&[("HOME", temp.path()), ("XAVIER_DATA_DIR", temp.path())]);
    let env_str = RestoreEnv::set_str(&[
        ("XAVIER_TOKEN", ROOT_TOKEN),
        ("XAVIER_JWT_SECRET", JWT_SECRET),
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
    let deps = AppDeps {
        workspace_ctx: WorkspaceContext {
            workspace_id: state.workspace_id.clone(),
            workspace: workspace_state,
        },
        space_manager: None,
        maloca_router: Router::new(),
        event_bus_for_ws: xavier::coordination::XavierEventBus::new(10),
        no_ui: true,
    };
    TestApp {
        app: build_app(state, deps),
        _env: env,
        _env_str: env_str,
        _temp: temp,
    }
}

/// A JWT for `role`, signed with `JWT_SECRET`.
pub fn jwt(role: UserRole) -> String {
    let user = User::new(
        format!("harness-{role:?}").to_lowercase(),
        "harness@example.com".to_string(),
        role,
    );
    xavier::security::auth::generate_jwt(&user, JWT_SECRET.as_bytes()).expect("jwt")
}

/// Status of `method uri` with an optional bearer token and an empty JSON body.
pub async fn status(app: &Router, method: Method, uri: &str, bearer: Option<&str>) -> StatusCode {
    let mut req = Request::builder()
        .uri(uri)
        .method(method)
        .header("Content-Type", "application/json");
    if let Some(token) = bearer {
        req = req.header("Authorization", format!("Bearer {token}"));
    }
    app.clone()
        .oneshot(req.body(Body::empty()).expect("request"))
        .await
        .expect("response")
        .status()
}
