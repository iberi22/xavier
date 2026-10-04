//! WP13m: `/memory`, `/v1/memories` and MCP memory tools act only on the
//! caller's own space.
//!
//! Built on the production composition: the real `auth_middleware` and
//! clearance layer, the real memory handlers, a default `WorkspaceContext`
//! layered OUTSIDE the auth layer exactly like `start_http_server`, and
//! `install_space_manager` for the `xsp_` path. Tempdirs and a static KEK
//! only; nothing touches the real data dir or master key.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware, Extension, Router,
};
use http_body_util::BodyExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use tower::ServiceExt;

use xavier::adapters::inbound::http::middleware::clearance::clearance_session_middleware;
use xavier::agents::provider::router::{ProviderKind, ProviderRouter};
use xavier::agents::rate_limit::RateLimitManager;
use xavier::agents::RuntimeConfig;
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
use xavier::espacio::{KeyOptions, NodeKek, SpaceManager, SpaceRole, StaticNodeKek, UnlockMode};
use xavier::memory::agent_indexer::AgentIndexer;
use xavier::memory::file_indexer::{FileIndexer, FileIndexerConfig};
use xavier::memory::qmd_memory::QmdMemory;
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use xavier::memory::store::MemoryBackend;
use xavier::ports::inbound::AgentLifecyclePort;
use xavier::secrets::audit::QmdAuditLogger;
use xavier::tasks::store::{InMemoryTaskStore, TaskService};
use xavier::workspace::{
    EmbeddingProviderMode, PlanTier, SyncPolicy, WorkspaceConfig, WorkspaceContext, WorkspaceState,
};

const ROOT: &str = "wp13m-root-token";
const JWT_SECRET: &str = "wp13m-jwt-secret-0123456789abcdef";

/// Serializes the process-global env.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn kek() -> Arc<dyn NodeKek> {
    Arc::new(StaticNodeKek::new([5u8; 32]))
}

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

/// The node-default workspace, as `start_http_server` layers it.
async fn default_ctx(tmp: &TempDir) -> WorkspaceContext {
    let config = WorkspaceConfig {
        id: "test-ws".to_string(),
        token: "unused".to_string(),
        plan: PlanTier::Community,
        memory_backend: MemoryBackend::Vec,
        storage_limit_bytes: None,
        request_limit: None,
        request_unit_limit: None,
        embedding_provider_mode: EmbeddingProviderMode::BringYourOwn,
        managed_google_embeddings: false,
        sync_policy: SyncPolicy::LocalOnly,
        dedup: Default::default(),
        protocol: Default::default(),
    };
    let ws = WorkspaceState::new_for_space(
        config,
        RuntimeConfig::default(),
        tmp.path().join("default-ws"),
        tmp.path().join("default-store"),
    )
    .await
    .expect("default workspace");
    WorkspaceContext {
        workspace_id: "test-ws".to_string(),
        workspace: Arc::new(ws),
    }
}

/// The production memory route set (`cli::server::memory_routes`) with the same
/// layer order as the production router for the memory surface, plus the espacio routes.
fn memory_app(state: &CliState, manager: Arc<SpaceManager>, default: WorkspaceContext) -> Router {
    let protected = xavier::cli::server::memory_routes()
        .merge(xavier::cli::server::memory_large_body_routes())
        .layer(middleware::from_fn(clearance_session_middleware))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    let app = Router::new()
        .merge(protected)
        .merge(guarded_espacio_router(state))
        // Static default context, outermost, like server.rs.
        .layer(Extension(default));
    install_space_manager(app, Some(manager)).with_state(state.clone())
}

async fn send(
    app: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let b = Request::builder()
        .method(method)
        .uri(path)
        .header("X-Xavier-Token", token);
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

struct Fx {
    app: Router,
    manager: Arc<SpaceManager>,
    tmp: TempDir,
    state: CliState,
    admin_a: String,
    admin_b: String,
}

/// Create a space directly through the manager (the HTTP admin API always
/// creates encrypting spaces) and return its owner admin token. Memory tests
/// use `encrypt_records = false`: encrypting spaces refuse memory routes until
/// record encryption exists in the memory store.
async fn create_space(manager: &Arc<SpaceManager>, id: &str, owner: &str, encrypt: bool) -> String {
    manager
        .create_with_keys(
            id.into(),
            id.into(),
            "".into(),
            owner.into(),
            false,
            KeyOptions {
                mode: UnlockMode::NodeUnlock,
                password: None,
                encrypt_records: encrypt,
            },
        )
        .await
        .expect("create space");
    manager
        .stores()
        .get(id)
        .unwrap()
        .issue_token(id, owner, SpaceRole::Admin, None)
        .unwrap()
        .raw
}

async fn fixture() -> Fx {
    std::env::set_var("XAVIER_TOKEN", ROOT);
    std::env::set_var("XAVIER_JWT_SECRET", JWT_SECRET);
    std::env::set_var("XAVIER_EMBEDDING_PROVIDER_MODE", "disabled");
    std::env::remove_var("XAVIER_SPACES");
    let tmp = TempDir::new().unwrap();
    let state = create_test_cli_state(&tmp).await;
    let manager = open_space_manager_with_kek(&tmp.path().join("state"), kek()).expect("manager");
    let default = default_ctx(&tmp).await;
    let app = memory_app(&state, manager.clone(), default);
    let mut admins = Vec::new();
    for (id, owner) in [("esp_a", "alice"), ("esp_b", "bob")] {
        admins.push(create_space(&manager, id, owner, false).await);
    }
    Fx {
        app,
        manager,
        tmp,
        state,
        admin_b: admins.pop().unwrap(),
        admin_a: admins.pop().unwrap(),
    }
}

fn issue(f: &Fx, space: &str, member: &str, role: SpaceRole) -> String {
    let s = f.manager.stores().get(space).unwrap();
    s.add_member(member, role).unwrap();
    s.issue_token(space, member, role, None).unwrap().raw
}

async fn add(f: &Fx, tok: &str, route: &str, path: &str, text: &str) -> (StatusCode, String) {
    let (st, body) = send(
        &f.app,
        "POST",
        route,
        tok,
        Some(serde_json::json!({"content": text, "path": path})),
    )
    .await;
    (st, body["id"].as_str().unwrap_or("").to_string())
}

async fn search(f: &Fx, tok: &str, route: &str, q: &str) -> Vec<serde_json::Value> {
    let (st, body) = send(
        &f.app,
        "POST",
        route,
        tok,
        Some(serde_json::json!({"query": q, "limit": 20})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{route}: {body}");
    body["results"].as_array().cloned().unwrap_or_default()
}

fn ids(rows: &[serde_json::Value]) -> Vec<String> {
    rows.iter()
        .map(|r| r["id"].as_str().unwrap_or("").to_string())
        .collect()
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn http_memory_is_isolated_per_space() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;

    // Distinct and identical content (same text, same path) in both spaces.
    let (st, a_only) = add(&f, &f.admin_a, "/memory/add", "notes/a", "alphaonly quokka").await;
    assert_eq!(st, StatusCode::OK);
    let (_, b_only) = add(&f, &f.admin_b, "/memory/add", "notes/b", "betaonly wombat").await;
    let (_, a_same) = add(
        &f,
        &f.admin_a,
        "/memory/add",
        "notes/same",
        "identical zebra",
    )
    .await;
    let (_, b_same) = add(
        &f,
        &f.admin_b,
        "/v1/memories",
        "notes/same",
        "identical zebra",
    )
    .await;
    assert!(!a_only.is_empty() && !b_only.is_empty());

    for route in ["/memory/search", "/v1/memories/search"] {
        // Nothing crosses in either direction.
        assert!(search(&f, &f.admin_b, route, "alphaonly quokka")
            .await
            .is_empty());
        assert!(search(&f, &f.admin_a, route, "betaonly wombat")
            .await
            .is_empty());
        // Own rows are found.
        assert!(!search(&f, &f.admin_a, route, "alphaonly quokka")
            .await
            .is_empty());
        assert!(!search(&f, &f.admin_b, route, "betaonly wombat")
            .await
            .is_empty());
        // Identical text: each side sees only its own row id.
        let ra = ids(&search(&f, &f.admin_a, route, "identical zebra").await);
        let rb = ids(&search(&f, &f.admin_b, route, "identical zebra").await);
        assert_eq!(ra, vec![a_same.clone()], "{route}");
        assert_eq!(rb, vec![b_same.clone()], "{route}");
        // Root / default space does not see space rows.
        assert!(search(&f, ROOT, route, "alphaonly quokka").await.is_empty());
        assert!(search(&f, ROOT, route, "identical zebra").await.is_empty());
    }

    // Get by id / path: another space's id is not found.
    let (st, _) = send(
        &f.app,
        "GET",
        &format!("/memory/get?id={a_only}"),
        &f.admin_b,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, body) = send(
        &f.app,
        "GET",
        &format!("/memory/get?id={a_only}"),
        &f.admin_a,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(body["content"].as_str().unwrap().contains("alphaonly"));
    let (_, body) = send(
        &f.app,
        "GET",
        &format!("/v1/memories/{a_only}"),
        &f.admin_b,
        None,
    )
    .await;
    assert!(!body.to_string().contains("alphaonly"), "{body}");
    let (_, body) = send(
        &f.app,
        "GET",
        &format!("/v1/memories/{a_only}"),
        &f.admin_a,
        None,
    )
    .await;
    assert!(body.to_string().contains("alphaonly"), "{body}");

    // A root write is not visible to a space.
    let (st, _) = add(&f, ROOT, "/memory/add", "notes/root", "rootonly lynx").await;
    assert_eq!(st, StatusCode::OK);
    assert!(search(&f, &f.admin_a, "/memory/search", "rootonly lynx")
        .await
        .is_empty());

    // The rows live in each space's own store, not in the node store.
    let count = |id: &str| -> i64 {
        let p = f
            .tmp
            .path()
            .join("state/spaces")
            .join(id)
            .join("memory.sqlite");
        rusqlite::Connection::open(p)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM memory_records", [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(count("esp_a"), 2);
    assert_eq!(count("esp_b"), 2);
    let _ = &f.state;
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn global_by_nature_routes_are_refused_for_space_tokens() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    for tok in [&f.admin_a, &issue(&f, "esp_a", "rita", SpaceRole::Reader)] {
        for (m, p) in [
            ("GET", "/memory/export"),
            ("POST", "/memory/decay"),
            ("POST", "/memory/prune"),
            ("POST", "/v1/memories/prune"),
            ("GET", "/v1/memories/graph"),
            ("GET", "/v1/graph/export"),
        ] {
            let (st, _) = send(&f.app, m, p, tok, Some(serde_json::json!({}))).await;
            assert_eq!(st, StatusCode::FORBIDDEN, "{m} {p}");
        }
    }
    // Root still reaches them (unchanged behaviour).
    let (st, _) = send(&f.app, "GET", "/memory/export", ROOT, None).await;
    assert_eq!(st, StatusCode::OK);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn reader_cannot_add_member_can() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    let reader = issue(&f, "esp_a", "rita", SpaceRole::Reader);
    let member = issue(&f, "esp_a", "mia", SpaceRole::Member);
    for route in ["/memory/add", "/v1/memories"] {
        assert_eq!(
            add(&f, &reader, route, "n/r", "reader write").await.0,
            StatusCode::FORBIDDEN
        );
    }
    let (st, id) = add(&f, &member, "/memory/add", "n/m", "member wrote hippo").await;
    assert_eq!(st, StatusCode::OK);
    // Reader can search and get what the member wrote.
    assert_eq!(
        ids(&search(&f, &reader, "/memory/search", "member wrote hippo").await),
        vec![id.clone()]
    );
    let (st, _) = send(
        &f.app,
        "GET",
        &format!("/memory/get?id={id}"),
        &reader,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        search(&f, &f.admin_b, "/memory/search", "member wrote hippo")
            .await
            .is_empty()
    );
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn locked_space_is_refused_until_unlocked() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XAVIER_TOKEN", ROOT);
    std::env::set_var("XAVIER_EMBEDDING_PROVIDER_MODE", "disabled");
    std::env::remove_var("XAVIER_SPACES");
    let tmp = TempDir::new().unwrap();
    let state = create_test_cli_state(&tmp).await;
    let root = tmp.path().join("state");
    let first = open_space_manager_with_kek(&root, kek()).unwrap();
    first
        .create_with_keys(
            "esp_l".into(),
            "l".into(),
            "".into(),
            "lia".into(),
            false,
            KeyOptions {
                mode: UnlockMode::PasswordRequired,
                password: Some("correct horse battery staple".into()),
                encrypt_records: false,
            },
        )
        .await
        .expect("create locked space");
    let tok = first
        .stores()
        .get("esp_l")
        .unwrap()
        .issue_token("esp_l", "lia", SpaceRole::Admin, None)
        .unwrap()
        .raw;
    drop(first);
    // Reopen: a password_required space comes up locked.
    let manager = open_space_manager_with_kek(&root, kek()).unwrap();
    assert!(manager.key_ring().unwrap().is_locked("esp_l"));
    let app = memory_app(&state, manager.clone(), default_ctx(&tmp).await);
    let calls = || {
        [
            (
                "POST",
                "/memory/search",
                Some(serde_json::json!({"query": "x"})),
            ),
            (
                "POST",
                "/memory/add",
                Some(serde_json::json!({"content": "x"})),
            ),
            (
                "POST",
                "/mcp/tools/call",
                Some(serde_json::json!({"name": "mem_search", "arguments": {"query": "x"}})),
            ),
        ]
    };
    // A locked space's store cannot be opened, so its token cannot even be
    // verified: the request fails closed with 401 before any handler runs.
    for (m, p, body) in calls() {
        let (st, _) = send(&app, m, p, &tok, body).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED, "{m} {p}");
    }
    // The workspace resolver itself refuses a locked space (423 in the
    // middleware) and never falls back to another workspace.
    let err = xavier::workspace::space_workspace_context(&manager, "esp_l")
        .await
        .err()
        .expect("locked");
    assert_eq!(err, xavier::workspace::SpaceScopeError::Locked);
    // After unlock the same token works.
    manager
        .key_ring()
        .unwrap()
        .unlock_with_password("esp_l", "correct horse battery staple")
        .expect("unlock");
    for (m, p, body) in calls() {
        let (st, _) = send(&app, m, p, &tok, body).await;
        assert_eq!(st, StatusCode::OK, "{m} {p}");
    }
}

async fn mcp(
    f: &Fx,
    tok: &str,
    name: &str,
    args: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    send(
        &f.app,
        "POST",
        "/mcp/tools/call",
        tok,
        Some(serde_json::json!({"name": name, "arguments": args})),
    )
    .await
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn mcp_memory_is_isolated_and_limited_for_space_tokens() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    let reader = issue(&f, "esp_a", "rita", SpaceRole::Reader);

    let (st, body) = mcp(
        &f,
        &f.admin_a,
        "create_memory",
        serde_json::json!({"content": "okapi grazes quietly", "path": "m/a"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let (st, _) = mcp(
        &f,
        &f.admin_b,
        "create_memory",
        serde_json::json!({"content": "tapir sleeps deeply", "path": "m/b"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // The response echoes the query, so judge by the candidates only.
    let hit = |body: &serde_json::Value, needle: &str| {
        body["structuredContent"]["candidates"]
            .to_string()
            .contains(needle)
    };
    let (_, ra) = mcp(
        &f,
        &f.admin_a,
        "mem_search",
        serde_json::json!({"query": "okapi grazes quietly", "include_content": true}),
    )
    .await;
    assert!(hit(&ra, "okapi"), "{ra}");
    let (_, rb) = mcp(
        &f,
        &f.admin_b,
        "mem_search",
        serde_json::json!({"query": "okapi grazes quietly", "include_content": true}),
    )
    .await;
    assert!(!hit(&rb, "okapi"), "{rb}");
    let (_, ra2) = mcp(
        &f,
        &f.admin_a,
        "memory_search",
        serde_json::json!({"query": "tapir sleeps deeply", "include_content": true}),
    )
    .await;
    assert!(!hit(&ra2, "tapir"), "{ra2}");
    // Root (default workspace) sees neither.
    let (_, rr) = mcp(
        &f,
        ROOT,
        "mem_search",
        serde_json::json!({"query": "okapi grazes quietly", "include_content": true}),
    )
    .await;
    assert!(!hit(&rr, "okapi"), "{rr}");
    // The row is in the space's own store, not the default one.
    let p = f.tmp.path().join("state/spaces/esp_a/memory.sqlite");
    let n: i64 = rusqlite::Connection::open(p)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM memory_records", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);

    // Reader: search ok, add refused.
    let (st, _) = mcp(
        &f,
        &reader,
        "mem_search",
        serde_json::json!({"query": "okapi"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = mcp(
        &f,
        &reader,
        "create_memory",
        serde_json::json!({"content": "x"}),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // Everything outside search/add/get is refused for space tokens.
    for tool in [
        "memory_prune",
        "memoryfragment_delete",
        "stats",
        "list_projects",
        "xavier_run_command",
        "secret_lend",
        "espacio_channel_list",
        "espacio_channel_create",
        "xavier_context",
    ] {
        let (st, _) = mcp(
            &f,
            &f.admin_a,
            tool,
            serde_json::json!({"space_id": "esp_a", "name": "x"}),
        )
        .await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{tool}");
    }
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn mcp_espacio_tools_need_the_root_credential_not_an_admin_claim() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    let admin = xavier::security::auth::User::new(
        "admin@example.test".into(),
        "admin".into(),
        xavier::security::auth::UserRole::Admin,
    );
    let jwt = xavier::security::auth::generate_jwt(&admin, JWT_SECRET.as_bytes()).unwrap();
    let args = serde_json::json!({"space_id": "esp_a"});
    // Admin JWT: authenticated, but refused.
    let (st, body) = mcp(&f, &jwt, "espacio_channel_list", args.clone()).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("root credential"), "{body}");
    // Root: allowed.
    let (st, body) = mcp(&f, ROOT, "espacio_channel_list", args).await;
    assert_eq!(st, StatusCode::OK, "{body}");
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn deleted_and_recreated_space_does_not_inherit_the_cached_store() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    let (st, _) = add(
        &f,
        &f.admin_a,
        "/memory/add",
        "n/old",
        "ghost pangolin walks",
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        !search(&f, &f.admin_a, "/memory/search", "ghost pangolin walks")
            .await
            .is_empty()
    );
    let (st, body) = send(
        &f.app,
        "DELETE",
        "/api/v1/espacio/admin/spaces/esp_a?confirm=esp_a",
        ROOT,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    // The old token is dead.
    let (st, _) = send(
        &f.app,
        "POST",
        "/memory/search",
        &f.admin_a,
        Some(serde_json::json!({"query": "ghost pangolin walks"})),
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    // Same id again: a fresh, empty store, not the old open handle.
    let fresh = create_space(&f.manager, "esp_a", "alice", false).await;
    let leaked = search(&f, &fresh, "/memory/search", "ghost pangolin walks").await;
    assert!(leaked.is_empty(), "{} stale rows", leaked.len());
    let (st, _) = add(&f, &fresh, "/memory/add", "n/new", "fresh narwhal swims").await;
    assert_eq!(st, StatusCode::OK);
    assert!(!search(&f, &fresh, "/memory/search", "fresh narwhal swims")
        .await
        .is_empty());
}

fn files_in(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn encrypted_space_memory_fails_closed_and_writes_nothing() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    let tok = create_space(&f.manager, "esp_enc", "erin", true).await;
    let dir = f.tmp.path().join("state/spaces/esp_enc");
    let before = files_in(&dir);

    for (m, p, body) in [
        (
            "POST",
            "/memory/add",
            serde_json::json!({"content": "secret plaintext", "path": "n/s"}),
        ),
        (
            "POST",
            "/v1/memories",
            serde_json::json!({"content": "secret plaintext", "path": "n/s"}),
        ),
        (
            "POST",
            "/memory/search",
            serde_json::json!({"query": "secret plaintext"}),
        ),
        (
            "POST",
            "/v1/memories/search",
            serde_json::json!({"query": "secret plaintext"}),
        ),
        (
            "POST",
            "/mcp/tools/call",
            serde_json::json!({"name": "create_memory", "arguments": {"content": "secret plaintext"}}),
        ),
        (
            "POST",
            "/mcp/tools/call",
            serde_json::json!({"name": "mem_search", "arguments": {"query": "secret"}}),
        ),
    ] {
        let (st, resp) = send(&f.app, m, p, &tok, Some(body)).await;
        assert_eq!(st, StatusCode::NOT_IMPLEMENTED, "{m} {p}: {resp}");
        assert!(resp.to_string().contains("not available yet"), "{resp}");
    }
    let (st, _) = send(&f.app, "GET", "/memory/get?id=x", &tok, None).await;
    assert_eq!(st, StatusCode::NOT_IMPLEMENTED);

    // No memory store or file was built for the encrypted space.
    let after = files_in(&dir);
    for name in &after {
        assert!(
            !name.starts_with("memory.") && name != "workspace",
            "unexpected memory artefact {name}"
        );
    }
    assert_eq!(
        after.iter().filter(|n| n.starts_with("memory")).count(),
        0,
        "{before:?} -> {after:?}"
    );
    assert!(!xavier::workspace::space_workspace_is_cached("esp_enc").await);
    let err = xavier::workspace::space_workspace_context(&f.manager, "esp_enc")
        .await
        .err()
        .expect("refused");
    assert_eq!(err, xavier::workspace::SpaceScopeError::EncryptionPending);

    // A plaintext space on the same node keeps working.
    let (st, _) = add(&f, &f.admin_a, "/memory/add", "n/p", "plain okapi").await;
    assert_eq!(st, StatusCode::OK);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn trashing_a_space_drops_its_cached_workspace() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    let (st, _) = add(&f, &f.admin_a, "/memory/add", "n/c", "cached tapir").await;
    assert_eq!(st, StatusCode::OK);
    assert!(xavier::workspace::space_workspace_is_cached("esp_a").await);
    let (st, body) = send(
        &f.app,
        "DELETE",
        "/api/v1/espacio/admin/spaces/esp_a?confirm=esp_a",
        ROOT,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(!xavier::workspace::space_workspace_is_cached("esp_a").await);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn space_tokens_cannot_update_or_delete_whatever_the_role() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    let tokens = [
        ("admin", f.admin_a.clone()),
        ("moderator", issue(&f, "esp_a", "mod", SpaceRole::Moderator)),
        ("member", issue(&f, "esp_a", "mem", SpaceRole::Member)),
        ("reader", issue(&f, "esp_a", "rea", SpaceRole::Reader)),
    ];
    // Every allowlisted memory path with the mutating verbs it must refuse.
    let allowlisted = [
        "/memory/search",
        "/memory/get",
        "/memory/add",
        "/v1/memories",
        "/v1/memories/search",
        "/v1/memories/some-id",
        "/v1/memories/some-id/outline",
        "/mcp/tools/call",
    ];
    for (role, tok) in &tokens {
        for p in allowlisted {
            for m in ["DELETE", "PUT", "PATCH"] {
                let (st, _) = send(&f.app, m, p, tok, Some(serde_json::json!({}))).await;
                assert_eq!(st, StatusCode::FORBIDDEN, "{role} {m} {p}");
            }
        }
        // Denied routes (not on the allowlist at all).
        for (m, p) in [
            ("POST", "/memory/export-pack"),
            ("POST", "/memory/update"),
            ("POST", "/memory/delete"),
            ("DELETE", "/memory/evict"),
            ("DELETE", "/v1/memories/some-id"),
        ] {
            let (st, _) = send(&f.app, m, p, tok, Some(serde_json::json!({}))).await;
            assert_eq!(st, StatusCode::FORBIDDEN, "{role} {m} {p}");
        }
        // The outline route is allowlisted for reads: reaches the handler.
        let (st, _) = send(&f.app, "GET", "/v1/memories/some-id/outline", tok, None).await;
        assert_ne!(st, StatusCode::FORBIDDEN, "{role} outline");
        assert_ne!(st, StatusCode::UNAUTHORIZED, "{role} outline");
    }
}

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();
