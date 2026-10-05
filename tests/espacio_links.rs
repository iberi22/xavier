//! WP13n: one-way space links and member-only linked search.
//!
//! Built on the production memory route set and the production espacio
//! routes, like `espacio_isolation_memory`. Tempdirs and a static KEK only.

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

const ROOT: &str = "wp13n-root-token";
const JWT_SECRET: &str = "wp13n-jwt-secret-0123456789abcdef";

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

#[allow(dead_code)]
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

const LINKS_B: &str = "/api/v1/espacio/spaces/esp_b/links";

async fn add_mem(f: &Fx, tok: &str, path: &str, text: &str) {
    let (st, body) = send(
        &f.app,
        "POST",
        "/memory/add",
        tok,
        Some(serde_json::json!({"content": text, "path": path})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
}

/// B (admin token) grants A read access; returns the link id.
async fn grant(f: &Fx, extra: serde_json::Value) -> String {
    let mut body = serde_json::json!({"target_space": "esp_a"});
    for (k, v) in extra.as_object().cloned().unwrap_or_default() {
        body[k] = v;
    }
    let (st, resp) = send(&f.app, "POST", LINKS_B, &f.admin_b, Some(body)).await;
    assert_eq!(st, StatusCode::CREATED, "{resp}");
    resp["link"]["id"].as_str().unwrap().to_string()
}

async fn search_linked(f: &Fx, tok: &str, q: &str) -> serde_json::Value {
    let (st, body) = send(
        &f.app,
        "POST",
        "/memory/search",
        tok,
        Some(serde_json::json!({"query": q, "limit": 20, "include_linked": true})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    body
}

fn results_text(body: &serde_json::Value) -> String {
    body["results"].to_string()
}

fn sources(body: &serde_json::Value) -> Vec<String> {
    body["results"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|r| r["source_space"].as_str().unwrap_or("").to_string())
        .collect()
}

async fn seed(f: &Fx) {
    add_mem(f, &f.admin_a, "notes/a", "alphaonly quokka").await;
    add_mem(f, &f.admin_b, "pub/b", "betaonly wombat").await;
    add_mem(f, &f.admin_b, "priv/b", "betaprivate wombat").await;
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn a_reads_b_through_link_and_results_are_tagged() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    seed(&f).await;

    // No link yet: nothing from B, even when asked.
    let r = search_linked(&f, &f.admin_a, "betaonly wombat").await;
    assert!(!results_text(&r).contains("betaonly"), "{r}");

    grant(&f, serde_json::json!({})).await;

    // Opt-in only: without the flag B stays invisible.
    let (_, plain) = send(
        &f.app,
        "POST",
        "/memory/search",
        &f.admin_a,
        Some(serde_json::json!({"query": "betaonly wombat"})),
    )
    .await;
    assert!(!results_text(&plain).contains("betaonly"), "{plain}");

    let r = search_linked(&f, &f.admin_a, "betaonly wombat").await;
    assert!(results_text(&r).contains("betaonly"), "{r}");
    assert!(sources(&r).iter().all(|s| s == "esp_b"), "{r}");
    let r = search_linked(&f, &f.admin_a, "alphaonly quokka").await;
    assert!(results_text(&r).contains("alphaonly"), "{r}");
    assert!(sources(&r).contains(&"esp_a".to_string()), "{r}");
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn link_is_one_way_b_cannot_read_a() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    seed(&f).await;
    grant(&f, serde_json::json!({})).await;
    let r = search_linked(&f, &f.admin_b, "alphaonly quokka").await;
    assert!(!results_text(&r).contains("alphaonly"), "{r}");
    assert!(r["linked_skipped"].as_array().unwrap().is_empty(), "{r}");
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn revoked_and_expired_links_are_excluded() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    seed(&f).await;
    let id = grant(&f, serde_json::json!({})).await;
    let r = search_linked(&f, &f.admin_a, "betaonly wombat").await;
    assert!(results_text(&r).contains("betaonly"), "{r}");

    let (st, _) = send(
        &f.app,
        "DELETE",
        &format!("{LINKS_B}/{id}"),
        &f.admin_b,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let r = search_linked(&f, &f.admin_a, "betaonly wombat").await;
    assert!(!results_text(&r).contains("betaonly"), "{r}");
    // The revoked row is kept and listed as revoked.
    let (_, list) = send(&f.app, "GET", LINKS_B, &f.admin_b, None).await;
    assert_eq!(list["links"][0]["revoked"], true, "{list}");

    // An expired link grants nothing.
    let past = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
    grant(&f, serde_json::json!({"expires_at": past})).await;
    let r = search_linked(&f, &f.admin_a, "betaonly wombat").await;
    assert!(!results_text(&r).contains("betaonly"), "{r}");

    // A future expiry still works.
    let future = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    grant(&f, serde_json::json!({"expires_at": future})).await;
    let r = search_linked(&f, &f.admin_a, "betaonly wombat").await;
    assert!(results_text(&r).contains("betaonly"), "{r}");
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn non_member_search_returns_zero() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    seed(&f).await;
    // B links to A only; a third space C has no link and no membership.
    grant(&f, serde_json::json!({})).await;
    let tok_c = create_space(&f.manager, "esp_c", "carol", false).await;
    for q in ["betaonly wombat", "alphaonly quokka"] {
        let r = search_linked(&f, &tok_c, q).await;
        assert_eq!(r["count"], 0, "{r}");
        assert!(r["results"].as_array().unwrap().is_empty(), "{r}");
    }
    // The link of B to A does not make C a reader through A.
    let (st, _) = send(
        &f.app,
        "POST",
        "/memory/search",
        "not-a-token",
        Some(serde_json::json!({"query": "betaonly"})),
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn link_never_lets_a_write_into_b() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    seed(&f).await;
    grant(&f, serde_json::json!({})).await;

    // Every write route with an explicit attempt to name space B.
    for (route, body) in [
        (
            "/memory/add",
            serde_json::json!({"content": "intruder emu", "path": "x/1", "space": "esp_b", "workspace_id": "esp_b"}),
        ),
        (
            "/v1/memories",
            serde_json::json!({"content": "intruder emu", "path": "x/2", "space": "esp_b"}),
        ),
    ] {
        let (st, resp) = send(&f.app, "POST", route, &f.admin_a, Some(body)).await;
        assert_eq!(st, StatusCode::OK, "{resp}");
    }
    let (st, _) = send(
        &f.app,
        "POST",
        "/mcp/tools/call",
        &f.admin_a,
        Some(serde_json::json!({"name": "create_memory",
            "arguments": {"content": "intruder emu", "path": "x/3", "space": "esp_b"}})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    // B's own store holds none of it; A's holds all of it.
    let rb = search_linked(&f, &f.admin_b, "intruder emu").await;
    assert!(!results_text(&rb).contains("intruder"), "{rb}");
    let n: i64 = rusqlite::Connection::open(f.tmp.path().join("state/spaces/esp_b/memory.sqlite"))
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM memory_records WHERE content LIKE '%intruder%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
    // Nor can A administer B's links (reverse grant), or delete B's data.
    let (st, _) = send(
        &f.app,
        "POST",
        LINKS_B,
        &f.admin_a,
        Some(serde_json::json!({"target_space": "esp_a"})),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = send(
        &f.app,
        "POST",
        "/api/v1/espacio/spaces/esp_a/links",
        &f.admin_a,
        Some(serde_json::json!({"target_space": "esp_a"})),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn only_admin_or_root_can_manage_links() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    let member = issue(&f, "esp_b", "mia", SpaceRole::Member);
    let moderator = issue(&f, "esp_b", "mo", SpaceRole::Moderator);
    let body = serde_json::json!({"target_space": "esp_a"});
    for tok in [&member, &moderator, &f.admin_a] {
        let (st, _) = send(&f.app, "POST", LINKS_B, tok, Some(body.clone())).await;
        assert_eq!(st, StatusCode::FORBIDDEN);
        let (st, _) = send(&f.app, "GET", LINKS_B, tok, None).await;
        assert_eq!(st, StatusCode::FORBIDDEN);
        let (st, _) = send(&f.app, "DELETE", &format!("{LINKS_B}/lnk_x"), tok, None).await;
        assert_eq!(st, StatusCode::FORBIDDEN);
    }
    // Nothing was created.
    let (st, list) = send(&f.app, "GET", LINKS_B, &f.admin_b, None).await;
    assert_eq!(st, StatusCode::OK);
    assert!(list["links"].as_array().unwrap().is_empty());
    // Root can.
    let (st, _) = send(&f.app, "POST", LINKS_B, ROOT, Some(body.clone())).await;
    assert_eq!(st, StatusCode::CREATED);
    // Unknown target space.
    let (st, _) = send(
        &f.app,
        "POST",
        LINKS_B,
        &f.admin_b,
        Some(serde_json::json!({"target_space": "esp_nope"})),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn link_filters_limit_what_is_visible() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    seed(&f).await;
    grant(&f, serde_json::json!({"path_prefix": "pub/"})).await;
    let r = search_linked(&f, &f.admin_a, "betaonly wombat").await;
    assert!(results_text(&r).contains("betaonly"), "{r}");
    let r = search_linked(&f, &f.admin_a, "betaprivate wombat").await;
    assert!(!results_text(&r).contains("betaprivate"), "{r}");
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn encrypted_linked_space_is_searched_server_side() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    seed(&f).await;
    let tok = create_space(&f.manager, "esp_enc", "erin", true).await;
    add_mem(&f, &tok, "notes/e", "encryptedonly kakapo").await;
    xavier::espacio::create_link(&f.manager, "esp_enc", "esp_a", None, None, None)
        .await
        .unwrap();
    grant(&f, serde_json::json!({})).await;
    let r = search_linked(&f, &f.admin_a, "encryptedonly kakapo").await;
    // WP13p: the encrypted space opens its own rows server-side.
    assert!(results_text(&r).contains("encryptedonly"), "{r}");
    assert!(sources(&r).iter().any(|s| s == "esp_enc"), "{r}");
    // A locked encrypted space is skipped with a note; the rest still works.
    f.manager.key_ring().unwrap().forget("esp_enc");
    let r = search_linked(&f, &f.admin_a, "betaonly wombat").await;
    assert!(results_text(&r).contains("betaonly"), "{r}");
    assert!(!results_text(&r).contains("encryptedonly"), "{r}");
    let skipped = r["linked_skipped"].as_array().unwrap();
    assert!(
        skipped
            .iter()
            .any(|s| s["space"] == "esp_enc" && s["reason"] == "Locked"),
        "{r}"
    );
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn mcp_mem_search_include_linked() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    seed(&f).await;
    let call = |tok: String, linked: bool| {
        let app = f.app.clone();
        async move {
            send(
                &app,
                "POST",
                "/mcp/tools/call",
                &tok,
                Some(serde_json::json!({"name": "mem_search",
                    "arguments": {"query": "betaonly wombat", "include_content": true, "include_linked": linked}})),
            )
            .await
        }
    };
    grant(&f, serde_json::json!({})).await;
    let (st, with) = call(f.admin_a.clone(), true).await;
    assert_eq!(st, StatusCode::OK, "{with}");
    let c = with["structuredContent"]["candidates"].to_string();
    assert!(c.contains("betaonly") && c.contains("esp_b"), "{with}");
    let (_, without) = call(f.admin_a.clone(), false).await;
    assert!(!without["structuredContent"]["candidates"]
        .to_string()
        .contains("betaonly"));
    // One way: B asking for A's data sees nothing of A.
    let (_, rb) = send(
        &f.app,
        "POST",
        "/mcp/tools/call",
        &f.admin_b,
        Some(serde_json::json!({"name": "mem_search",
            "arguments": {"query": "alphaonly quokka", "include_content": true, "include_linked": true}})),
    )
    .await;
    assert!(
        !rb["structuredContent"]["candidates"]
            .to_string()
            .contains("alphaonly"),
        "{rb}"
    );
}
