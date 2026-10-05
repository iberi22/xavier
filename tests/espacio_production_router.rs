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
        .route("/memory/decay", post(|| async { "reached" }))
        .route("/mcp", post(|| async { "reached" }))
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

    // The list never leaks tokens or recovery codes (checked against the
    // real values, not a keyword the response could trivially lack).
    assert!(!list.to_string().contains("xsp_"));
    assert!(!list.to_string().contains(&owner_a));
    assert!(!list.to_string().contains(&_recovery));

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
    assert!(!got.to_string().contains("xsp_"));
    assert!(!got.to_string().contains(&owner_a));
    assert!(!got.to_string().contains(&_recovery));
    for (m, p) in [
        ("GET", "/api/v1/espacio/spaces/esp_b"),
        ("GET", "/api/v1/espacio/admin/spaces"),
        ("POST", "/api/v1/espacio/admin/spaces"),
        // Not audited as space-scoped (WP-13m): still refused.
        ("POST", "/memory/decay"),
        ("POST", "/mcp"),
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

const ADMIN_PATHS: [(&str, &str); 3] = [
    ("GET", "/api/v1/espacio/admin/spaces"),
    ("POST", "/api/v1/espacio/admin/spaces"),
    ("DELETE", "/api/v1/espacio/admin/spaces/esp_a?confirm=esp_a"),
];

/// BLOCKING 1: ephemeral sessions, admin-role JWTs and `xav_` write tokens
/// must not reach the admin plane; only the root credential does.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn espacio_admin_plane_is_root_only_for_every_credential() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XAVIER_TOKEN", ROOT);
    std::env::remove_var("XAVIER_SPACES");
    let jwt_secret = "wp13e-test-jwt-secret-0123456789abcdef";
    std::env::set_var("XAVIER_JWT_SECRET", jwt_secret);
    let tmp = TempDir::new().unwrap();
    let state = create_test_cli_state(&tmp).await;
    let manager = open_space_manager_with_kek(&tmp.path().join("state"), kek()).expect("manager");
    let app = production_app(&state, Some(manager.clone()));
    let (_owner, _rec) = create_two(&app).await;

    // Credentials: ephemeral session, admin JWT, xav_ write and all tokens
    // (the token DB is redirected to the tempdir).
    let session = state.session_manager.create_session().id;
    let user = xavier::security::auth::User::new(
        "admin@example.com".into(),
        "Admin".into(),
        xavier::security::auth::UserRole::Admin,
    );
    let jwt = xavier::security::auth::generate_jwt(&user, jwt_secret.as_bytes()).unwrap();
    xavier::codebase::connection_manager::ConnectionManager::global()
        .connect_with_path("security", tmp.path().join("security.db"))
        .unwrap();
    let store = xavier::security::tokens::TokenStore::new();
    store.init_schema_async().await.unwrap();
    let (xav_write, _) = store
        .create_token("w".into(), vec!["write".into()], None)
        .await
        .unwrap();
    let (xav_all, _) = store
        .create_token("a".into(), vec!["all".into()], None)
        .await
        .unwrap();

    for (name, tok) in [
        ("ephemeral", &session),
        ("jwt-admin", &jwt),
        ("xav_write", &xav_write),
        ("xav_all", &xav_all),
    ] {
        for (m, p) in ADMIN_PATHS {
            let body = (m == "POST").then(|| create_body("esp_x", "mallory"));
            let (st, _) = send(&app, m, p, Some(tok), body).await;
            assert_eq!(st, StatusCode::FORBIDDEN, "{name} {m} {p}");
        }
    }
    // Nothing was created or deleted by those attempts.
    assert_eq!(manager.list().await.len(), 2);
    assert!(manager.get("esp_x").await.is_err());

    // Root passes (and 404 only because the confirm path is for a real id).
    let (st, _) = send(&app, "GET", ADMIN_PATHS[0].1, Some(ROOT), None).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = send(
        &app,
        "POST",
        ADMIN_PATHS[1].1,
        Some(ROOT),
        Some(create_body("esp_x", "root-made")),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    std::env::remove_var("XAVIER_JWT_SECRET");
}

/// MINOR 2: delete = move to `.trash`, needs `?confirm=`, kills the tokens.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn espacio_delete_moves_to_trash_and_requires_confirm() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XAVIER_TOKEN", ROOT);
    std::env::remove_var("XAVIER_SPACES");
    let tmp = TempDir::new().unwrap();
    let state = create_test_cli_state(&tmp).await;
    let root = tmp.path().join("state");
    let manager = open_space_manager_with_kek(&root, kek()).expect("manager");
    let app = production_app(&state, Some(manager.clone()));
    let (owner_a, _) = create_two(&app).await;

    // Missing / wrong confirm: 400, nothing moves.
    for p in [
        "/api/v1/espacio/spaces/esp_a",
        "/api/v1/espacio/spaces/esp_a?confirm=esp_b",
    ] {
        let (st, _) = send(&app, "DELETE", p, Some(&owner_a), None).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{p}");
    }
    let (st, _) = send(
        &app,
        "DELETE",
        "/api/v1/espacio/admin/spaces/esp_b",
        Some(ROOT),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert!(root.join("spaces/esp_a/space.json").exists());

    // Own admin token deletes its space with confirm.
    let (st, body) = send(
        &app,
        "DELETE",
        "/api/v1/espacio/spaces/esp_a?confirm=esp_a",
        Some(&owner_a),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(!root.join("spaces/esp_a").exists());
    let trashed: Vec<_> = std::fs::read_dir(root.join("spaces/.trash"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(trashed.len(), 1, "{trashed:?}");
    assert!(trashed[0].starts_with("esp_a-"), "{trashed:?}");
    assert!(root
        .join("spaces/.trash")
        .join(&trashed[0])
        .join("space.json")
        .exists());

    // The trashed space's token no longer authenticates.
    let (st, _) = send(
        &app,
        "GET",
        "/api/v1/espacio/spaces/esp_a",
        Some(&owner_a),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);

    // Root deletes the other one the same way; a restart ignores the trash.
    let (st, _) = send(
        &app,
        "DELETE",
        "/api/v1/espacio/admin/spaces/esp_b?confirm=esp_b",
        Some(ROOT),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = send(
        &app,
        "DELETE",
        "/api/v1/espacio/admin/spaces/esp_b?confirm=esp_b",
        Some(ROOT),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    drop(app);
    drop(manager);
    let manager2 = open_space_manager_with_kek(&root, kek()).expect("reopen");
    assert!(manager2.list().await.is_empty());
}

/// MINOR 3: `password_required` is honoured by create, never echoes the
/// password, and the space is locked after the manager reopens.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn espacio_create_honours_password_required() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XAVIER_TOKEN", ROOT);
    std::env::remove_var("XAVIER_SPACES");
    let tmp = TempDir::new().unwrap();
    let state = create_test_cli_state(&tmp).await;
    let root = tmp.path().join("state");
    let manager = open_space_manager_with_kek(&root, kek()).expect("manager");
    let app = production_app(&state, Some(manager.clone()));
    let pw = "correct horse battery staple 42";

    let mut body = create_body("esp_p", "alice");
    body["unlock_mode"] = "password_required".into();
    body["password"] = pw.into();
    let (st, got) = send(
        &app,
        "POST",
        "/api/v1/espacio/admin/spaces",
        Some(ROOT),
        Some(body),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{got}");
    assert!(!got.to_string().contains(pw));
    assert!(got["recovery_code"].as_str().is_some());
    assert!(got["note"].as_str().unwrap().contains("node key is lost"));

    // password_required without a password is rejected, not downgraded.
    let mut bad = create_body("esp_q", "alice");
    bad["unlock_mode"] = "password_required".into();
    let (st, _) = send(
        &app,
        "POST",
        "/api/v1/espacio/admin/spaces",
        Some(ROOT),
        Some(bad),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    drop(app);
    drop(manager);
    let manager2 = open_space_manager_with_kek(&root, kek()).expect("reopen");
    assert!(manager2.key_ring().unwrap().is_locked("esp_p"));
}

/// MINOR 5: one authoritative manager; re-install replaces, never splits.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn espacio_install_keeps_a_single_authoritative_manager() {
    use xavier::adapters::inbound::http::routes::get_space_manager;
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("XAVIER_SPACES");
    let tmp = TempDir::new().unwrap();
    let state = create_test_cli_state(&tmp).await;
    let a = open_space_manager_with_kek(&tmp.path().join("a"), kek()).unwrap();
    let b = open_space_manager_with_kek(&tmp.path().join("b"), kek()).unwrap();

    let _ = production_app(&state, Some(a.clone()));
    assert!(Arc::ptr_eq(&get_space_manager().unwrap(), &a));
    let _ = production_app(&state, Some(b.clone()));
    assert!(Arc::ptr_eq(&get_space_manager().unwrap(), &b));
    assert!(!Arc::ptr_eq(&get_space_manager().unwrap(), &a));
    let _ = production_app(&state, None);
    assert!(get_space_manager().is_none());
}

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();
