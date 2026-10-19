//! MKT-02 acceptance: a marketplace v2 listing and receipt survive a daemon
//! restart, exercised on the production router with `market-v2` on.
//!
//! A fresh router built over the same SQLite file stands in for a restarted
//! daemon. This file is its own test binary, so the env vars it sets cannot
//! leak into the rest of the suite.
//!
//! The file is compiled only with the `market-v2` feature. Without it the
//! target is an empty binary, so feature-less runs (pre-commit, ci-safe) still
//! build it.

#![cfg(feature = "market-v2")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use tempfile::TempDir;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::util::ServiceExt;
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
use xavier::market_v2;
use xavier::memory::agent_indexer::AgentIndexer;
use xavier::memory::file_indexer::{FileIndexer, FileIndexerConfig};
use xavier::memory::qmd_memory::QmdMemory;
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use xavier::ports::inbound::AgentLifecyclePort;
use xavier::secrets::audit::QmdAuditLogger;
use xavier::tasks::store::{InMemoryTaskStore, TaskService};
use xavier::workspace::{WorkspaceContext, WorkspaceState};
use xavier_market::Signer;

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();

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

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Points the market store at a tempdir file and turns the beta flag on.
struct BetaEnv {
    _lock: MutexGuard<'static, ()>,
    _dir: tempfile::TempDir,
}

impl BetaEnv {
    fn new() -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Keep every user-state path (data dir, state, home) inside a sandbox.
        xavier::test_support::isolate_and_redirect();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var(
            "XAVIER_MARKET_V2_DB_PATH",
            dir.path().join("market.sqlite3"),
        );
        std::env::set_var("XAVIER_MARKET_V2", "beta");
        Self {
            _lock: lock,
            _dir: dir,
        }
    }
}

impl Drop for BetaEnv {
    fn drop(&mut self) {
        std::env::remove_var("XAVIER_MARKET_V2");
        std::env::remove_var("XAVIER_MARKET_V2_DB_PATH");
    }
}

async fn send(
    router: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let builder = Request::builder().method(method).uri(uri);
    let builder = if let Some(t) = token {
        builder.header("X-Xavier-Token", t)
    } else {
        builder
    };
    let request = match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())),
        None => builder.body(Body::empty()),
    }
    .expect("build request");
    let response = router.clone().oneshot(request).await.expect("route");
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

/// The publisher key that owns the listings in this test.
fn owner() -> Signer {
    Signer::from_seed([7; 32])
}

fn listing_body() -> Value {
    json!({
        "seller_id": owner().public_key_hex(),
        "title": "Weather station logs",
        "license": "CC-BY-4.0",
        "price_credits": 25,
        "consent_ref": "consent-2026-10-19"
    })
}

/// Like [`send`], but the request is also signed by `signer` over `uri`'s path
/// and the body, as the publisher-bound mutation routes require.
async fn send_signed(
    router: &Router,
    signer: &Signer,
    method: Method,
    uri: &str,
    body: Value,
    token: &str,
) -> (StatusCode, Value) {
    let bytes = body.to_string();
    let path = uri.split('?').next().unwrap_or(uri);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let signed = signer.sign(method.as_str(), path, now_ms, bytes.as_bytes());
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("X-Xavier-Token", token)
        .header(market_v2::ACTOR_KEY_HEADER, signed.public_key_hex)
        .header(
            market_v2::ACTOR_TIMESTAMP_HEADER,
            signed.timestamp_ms.to_string(),
        )
        .header(market_v2::ACTOR_SIGNATURE_HEADER, signed.signature_hex)
        .header("content-type", "application/json")
        .body(Body::from(bytes))
        .expect("build request");
    let response = router.clone().oneshot(request).await.expect("route");
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

#[tokio::test]
async fn listing_and_receipt_survive_a_daemon_restart() {
    let env = BetaEnv::new();

    let expected_root_token = "market-v2-smoke-root-token-9999";
    std::env::set_var("XAVIER_TOKEN", expected_root_token);

    let state = create_test_cli_state(&env._dir).await;

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

    let before = build_app(state.clone(), deps.clone());

    // Check UNAUTHORIZED
    let (status, _listing) = send(
        &before,
        Method::POST,
        "/v2/marketplace/listings",
        Some(listing_body()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, listing) = send_signed(
        &before,
        &owner(),
        Method::POST,
        "/v2/marketplace/listings",
        listing_body(),
        expected_root_token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let listing_id = listing["id"].as_str().unwrap().to_string();

    let (status, receipt) = send(
        &before,
        Method::POST,
        "/v2/marketplace/receipts",
        Some(json!({"listing_id": listing_id, "buyer_id": "buyer-1"})),
        Some(expected_root_token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    drop(before);

    let after = build_app(state.clone(), deps.clone());
    let (status, fetched) = send(
        &after,
        Method::GET,
        &format!("/v2/marketplace/listings/{listing_id}"),
        None,
        Some(expected_root_token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mut fetched_listing = fetched.clone();
    fetched_listing
        .as_object_mut()
        .unwrap()
        .remove("provenance");
    assert_eq!(fetched_listing, listing);

    let (status, receipts) = send(
        &after,
        Method::GET,
        "/v2/marketplace/receipts?buyer_id=buyer-1",
        None,
        Some(expected_root_token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipts, json!([receipt]));

    std::env::remove_var("XAVIER_TOKEN");
}

/// Builds the production router (`build_app`) over a fresh fixture state.
async fn production_app(dir: &TempDir) -> Router {
    let state = create_test_cli_state(dir).await;
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
    build_app(state, deps)
}

/// MKT-02 "default off": with the flag unset, any value other than the exact
/// `beta`, or a bad beta config, the production router serves no `/v2/*`
/// marketplace route even to a valid root token. `beta` is the control.
#[tokio::test]
async fn flag_off_or_bad_config_mounts_nothing_on_the_production_router() {
    let env = BetaEnv::new();
    let token = "market-v2-flag-root-token-9999";
    std::env::set_var("XAVIER_TOKEN", token);

    let cases: [(Option<&str>, Option<&str>, StatusCode); 5] = [
        (None, None, StatusCode::NOT_FOUND),
        (Some("on"), None, StatusCode::NOT_FOUND),
        (Some(""), None, StatusCode::NOT_FOUND),
        (Some("beta"), Some("on_chain"), StatusCode::NOT_FOUND),
        (Some("beta"), None, StatusCode::OK),
    ];
    for (flag, settlement, expected) in cases {
        match flag {
            Some(v) => std::env::set_var("XAVIER_MARKET_V2", v),
            None => std::env::remove_var("XAVIER_MARKET_V2"),
        }
        match settlement {
            Some(v) => std::env::set_var("XAVIER_MARKET_SETTLEMENT", v),
            None => std::env::remove_var("XAVIER_MARKET_SETTLEMENT"),
        }
        let app = production_app(&env._dir).await;
        let (status, _) = send(
            &app,
            Method::GET,
            "/v2/marketplace/listings",
            None,
            Some(token),
        )
        .await;
        assert_eq!(status, expected, "flag {flag:?} settlement {settlement:?}");
    }

    std::env::remove_var("XAVIER_MARKET_SETTLEMENT");
    std::env::remove_var("XAVIER_TOKEN");
}
