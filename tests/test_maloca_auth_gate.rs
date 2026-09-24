//! Regression test for the Maloca CORS/auth audit (fix/maloca-store-cors-backlog).
//!
//! Previously, `CorsLayer::permissive()` and the API's auth middleware were
//! applied to the Axum router *before* `xavier::maloca::nested_router(..)` /
//! `xavier::server::maloca::v1_maloca_router(..)` were merged in
//! (`src/cli/server.rs`), so every `/maloca/*` and `/v1/maloca/*` route —
//! including mutating ones like `POST /maloca/support` — was reachable
//! without CORS headers and without any token check.
//!
//! Maloca is intentionally public for reads (dogfood surface for
//! `@swal/maloca-client` / the panel), but every mutating verb must require
//! the same token as the rest of the API. This mirrors the production wiring
//! by layering `xavier::cli::http_setup::maloca_mutation_auth_middleware`
//! directly on the same two routers `cli::server::run` merges.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware,
};
use http_body_util::BodyExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use tower::ServiceExt;

use xavier::agents::provider::router::{ProviderKind, ProviderRouter};
use xavier::agents::rate_limit::RateLimitManager;
use xavier::app::proxy_use_case::ProxyUseCase;
use xavier::app::qmd_memory_adapter::QmdMemoryAdapter;
use xavier::cli::http_setup::maloca_mutation_auth_middleware;
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
use xavier::tasks::store::{InMemoryTaskStore, TaskService};

/// Guards `XAVIER_TOKEN` mutation. `auth_middleware` treats an empty expected
/// token as matching an empty provided token (root bypass), so the test must
/// pin a real, non-empty token for the "no token => 401" assertion to be
/// meaningful. Serialized via a lock since env vars are process-global.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Minimal `CliState` fixture, following the same construction pattern as
/// `tests/test_store_path_hierarchy.rs`.
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

#[tokio::test]
// The std Mutex only serialises XAVIER_TOKEN mutation across tests in this binary;
// holding it across awaits is intended and safe on the per-test runtime.
#[allow(clippy::await_holding_lock)]
async fn maloca_write_routes_require_token_reads_stay_public() {
    let _env_lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let prev_token = std::env::var("XAVIER_TOKEN").ok();
    std::env::set_var("XAVIER_TOKEN", "maloca-auth-gate-test-token");

    let temp = tempfile::tempdir().unwrap();
    let state = create_test_cli_state(&temp).await;
    let maloca_store = state.maloca.clone();

    // Mirrors src/cli/server.rs's production wiring: the same two Maloca
    // routers, layered with the same selective auth middleware.
    let app = xavier::maloca::nested_router::<CliState>(maloca_store)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            maloca_mutation_auth_middleware,
        ))
        .with_state(state.clone());

    // 1. POST /maloca/support with NO token -> 401 Unauthorized.
    let req = Request::builder()
        .method("POST")
        .uri("/maloca/support")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"title":"audit ticket","body":"no token attached"}"#,
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "POST /maloca/support without a token must be rejected"
    );

    // 2. GET /maloca/support with NO token -> stays public (200 OK), matching
    //    the router's existing public-dogfood-read design.
    let req = Request::builder()
        .method("GET")
        .uri("/maloca/support")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET /maloca/support must remain public even with the auth gate in place"
    );

    // 3. OPTIONS preflight on the mutating route must not be rejected by the
    //    auth gate (CorsLayer normally short-circuits this in production; at
    //    minimum our own middleware must not turn it into a 401).
    let req = Request::builder()
        .method("OPTIONS")
        .uri("/maloca/support")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_ne!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "OPTIONS preflight must not be blocked by the mutation auth gate"
    );

    // 4. POST /maloca/support WITH the correct token -> succeeds.
    let req = Request::builder()
        .method("POST")
        .uri("/maloca/support")
        .header("content-type", "application/json")
        .header("X-Xavier-Token", "maloca-auth-gate-test-token")
        .body(Body::from(
            r#"{"title":"audit ticket","body":"with valid token"}"#,
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        status,
        StatusCode::OK,
        "POST /maloca/support with a valid token must succeed, got body: {}",
        String::from_utf8_lossy(&body_bytes)
    );

    // 5. POST /maloca/support with a WRONG token -> still 401.
    let req = Request::builder()
        .method("POST")
        .uri("/maloca/support")
        .header("content-type", "application/json")
        .header("X-Xavier-Token", "not-the-right-token")
        .body(Body::from(
            r#"{"title":"audit ticket","body":"wrong token"}"#,
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    match prev_token {
        Some(v) => std::env::set_var("XAVIER_TOKEN", v),
        None => std::env::remove_var("XAVIER_TOKEN"),
    }
}
