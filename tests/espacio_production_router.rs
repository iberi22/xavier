//! WP13e: the espacio API on the PRODUCTION wiring.
//!
//! The daemon router is built inline in `start_http_server`, so this builds
//! it from the same two shared pieces the daemon uses:
//! `guarded_espacio_router` (routes + rate limit + clearance + the real
//! `auth_middleware`) and `install_space_manager` (the request extension the
//! `xsp_` path needs), plus probe routes standing in for `/memory/search` and
//! `/mcp/tools/call` behind the same auth layer. Tempdirs and a static KEK
//! only; nothing touches the real data dir or master key.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware,
    routing::post,
    Router,
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
use xavier::cli::http_setup::auth_middleware;
use xavier::cli::state::{
    guarded_espacio_router, install_space_manager, open_space_manager_with_kek, CliState,
    CodeGraphState,
};
use xavier::codebase::conversations_db::ConversationsDb;
use xavier::coordination::KeyLendingEngine;
use xavier::coordination::SimpleAgentRegistry;
use xavier::embedding::NoopEmbedder;
use xavier::espacio::{NodeKek, SpaceManager, StaticNodeKek};
use xavier::memory::agent_indexer::AgentIndexer;
use xavier::memory::file_indexer::{FileIndexer, FileIndexerConfig};
use xavier::memory::qmd_memory::QmdMemory;
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use xavier::ports::inbound::AgentLifecyclePort;
use xavier::secrets::audit::QmdAuditLogger;
use xavier::tasks::store::{InMemoryTaskStore, TaskService};

const ROOT: &str = "wp13e-root-token";

/// Serializes the process-global env (`XAVIER_TOKEN`, `XAVIER_SPACES`).
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn kek() -> Arc<dyn NodeKek> {
    Arc::new(StaticNodeKek::new([9u8; 32]))
}

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

fn production_app(state: &CliState, manager: Option<Arc<SpaceManager>>) -> Router {
    let probes = Router::new()
        .route("/memory/search", post(|| async { "reached" }))
        .route("/mcp/tools/call", post(|| async { "reached" }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    let app = Router::new()
        .merge(guarded_espacio_router(state))
        .merge(probes);
    install_space_manager(app, manager).with_state(state.clone())
}

async fn send(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut b = Request::builder().method(method).uri(path);
    if let Some(t) = token {
        b = b.header("X-Xavier-Token", t);
    }
    let req = match body {
        Some(v) => b
            .header("content-type", "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

fn create_body(id: &str, owner: &str) -> serde_json::Value {
    serde_json::json!({"id": id, "name": id, "description": "", "owner_node": owner})
}

/// Root creates `esp_a` and `esp_b`; returns (owner token a, recovery a).
async fn create_two(app: &Router) -> (String, String) {
    let (st, a) = send(
        app,
        "POST",
        "/api/v1/espacio/admin/spaces",
        Some(ROOT),
        Some(create_body("esp_a", "alice")),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{a}");
    let token = a["owner_token"].as_str().expect("owner token").to_string();
    let recovery = a["recovery_code"]
        .as_str()
        .expect("recovery code")
        .to_string();
    assert!(token.starts_with("xsp_esp_a_"), "{token}");
    assert!(!recovery.is_empty());
    let (st, b) = send(
        app,
        "POST",
        "/api/v1/espacio/admin/spaces",
        Some(ROOT),
        Some(create_body("esp_b", "bob")),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{b}");
    (token, recovery)
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn espacio_production_wiring_auth_tokens_and_restart() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XAVIER_TOKEN", ROOT);
    std::env::remove_var("XAVIER_SPACES");
    let tmp = TempDir::new().unwrap();
    let state = create_test_cli_state(&tmp).await;
    let root = tmp.path().join("state");
    let manager = open_space_manager_with_kek(&root, kek()).expect("manager");
    let app = production_app(&state, Some(manager.clone()));

    // No token: 401. Wrong token: 401 (not a valid root, not an xsp_ token).
    let (st, _) = send(&app, "GET", "/api/v1/espacio/admin/spaces", None, None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, _) = send(
        &app,
        "GET",
        "/api/v1/espacio/admin/spaces",
        Some("nope"),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);

    // Root creates; token + recovery code are in the response.
    let (owner_a, _recovery) = create_two(&app).await;

    // Root list sees both; and they were persisted on disk.
    let (st, list) = send(
        &app,
        "GET",
        "/api/v1/espacio/admin/spaces",
        Some(ROOT),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 2);
    assert!(root.join("spaces/esp_a/space.json").exists());

    // The list never leaks tokens or recovery codes.
    assert!(!list.to_string().contains("xsp_"));
    assert!(!list.to_string().contains("recovery"));

    // Owner token: own space ok, other space 403, admin plane 403,
    // workspace-global routes 403.
    let (st, got) = send(
        &app,
        "GET",
        "/api/v1/espacio/spaces/esp_a",
        Some(&owner_a),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{got}");
    for (m, p) in [
        ("GET", "/api/v1/espacio/spaces/esp_b"),
        ("GET", "/api/v1/espacio/admin/spaces"),
        ("POST", "/api/v1/espacio/admin/spaces"),
        ("POST", "/memory/search"),
        ("POST", "/mcp/tools/call"),
    ] {
        let (st, _) = send(&app, m, p, Some(&owner_a), Some(serde_json::json!({}))).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{m} {p}");
    }
    // Root and scoped xav_-style callers cannot use the own-space route (the
    // handler demands a space token); root uses the admin plane instead.
    let (st, _) = send(
        &app,
        "GET",
        "/api/v1/espacio/spaces/esp_a",
        Some(ROOT),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, got) = send(
        &app,
        "GET",
        "/api/v1/espacio/admin/spaces/esp_b",
        Some(ROOT),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{got}");
    // Root still reaches the probes (they are real routes behind auth).
    let (st, _) = send(
        &app,
        "POST",
        "/memory/search",
        Some(ROOT),
        Some(serde_json::json!({})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // Restart: a new manager over the same dir sees the spaces, and the
    // owner token still verifies against the new app.
    drop(app);
    drop(manager);
    let manager2 = open_space_manager_with_kek(&root, kek()).expect("reopen");
    assert_eq!(manager2.list().await.len(), 2);
    let app2 = production_app(&state, Some(manager2));
    let (st, _) = send(
        &app2,
        "GET",
        "/api/v1/espacio/spaces/esp_a",
        Some(&owner_a),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn espacio_off_boots_and_routes_answer_503() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XAVIER_TOKEN", ROOT);
    std::env::set_var("XAVIER_SPACES", "off");
    let tmp = TempDir::new().unwrap();
    let state = create_test_cli_state(&tmp).await;
    let manager = open_space_manager_with_kek(&tmp.path().join("state"), kek());
    std::env::remove_var("XAVIER_SPACES");
    assert!(manager.is_none(), "kill switch must disable espacio");
    assert!(!tmp.path().join("state").exists());

    let app = production_app(&state, manager);
    let (st, _) = send(
        &app,
        "GET",
        "/api/v1/espacio/admin/spaces",
        Some(ROOT),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE);
    let (st, _) = send(
        &app,
        "POST",
        "/api/v1/espacio/admin/spaces",
        Some(ROOT),
        Some(create_body("esp_a", "alice")),
    )
    .await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE);
    // An xsp_ token cannot authenticate without a manager.
    let (st, _) = send(
        &app,
        "GET",
        "/api/v1/espacio/spaces/esp_a",
        Some("xsp_esp_a_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    // The rest of the API still works.
    let (st, _) = send(
        &app,
        "POST",
        "/memory/search",
        Some(ROOT),
        Some(serde_json::json!({})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
}
