//! WP13p: memory stores of encrypting spaces seal every record with THAT
//! space's data key (never the default-space or node key).
//!
//! Production composition (`memory_routes()`, real auth and clearance layers,
//! `install_space_manager`), tempdirs and static KEKs only. The node record
//! key env is pinned to a test key and the data dir to a tempdir so nothing
//! touches a real data directory.

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
use xavier::memory::sqlite_vec_store::at_rest::SpaceCrypto;
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use xavier::memory::store::MemoryStore;

use xavier::memory::store::MemoryBackend;
use xavier::ports::inbound::AgentLifecyclePort;
use xavier::secrets::audit::QmdAuditLogger;
use xavier::tasks::store::{InMemoryTaskStore, TaskService};
use xavier::workspace::{
    EmbeddingProviderMode, PlanTier, SyncPolicy, WorkspaceConfig, WorkspaceContext, WorkspaceState,
};

const ROOT: &str = "wp13p-root-token";
const JWT_SECRET: &str = "wp13p-jwt-secret-0123456789abcdef";

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

const CANARY: &str = "ZXQ-canary-7f3a91-quokka";
const CANARY2: &str = "ZXQ-canary-rev-55b2c4-wombat";
const NODE_KEY_HEX: &str = "0707070707070707070707070707070707070707070707070707070707070707";

struct Fx {
    app: Router,
    manager: Arc<SpaceManager>,
    tmp: TempDir,
    admin_a: String,
    admin_b: String,
    admin_p: String,
}

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
    let tmp = TempDir::new().unwrap();
    std::env::set_var("XAVIER_TOKEN", ROOT);
    std::env::set_var("XAVIER_JWT_SECRET", JWT_SECRET);
    std::env::set_var("XAVIER_EMBEDDING_PROVIDER_MODE", "disabled");
    std::env::set_var("XAVIER_RECORD_KEY", NODE_KEY_HEX);
    std::env::set_var("XAVIER_DATA_DIR", tmp.path().join("node-data"));
    std::env::remove_var("XAVIER_SPACES");
    let state = create_test_cli_state(&tmp).await;
    let manager = open_space_manager_with_kek(&tmp.path().join("state"), kek()).expect("manager");
    let default = default_ctx(&tmp).await;
    let app = memory_app(&state, manager.clone(), default);
    let admin_a = create_space(&manager, "esp_a", "alice", true).await;
    let admin_b = create_space(&manager, "esp_b", "bob", true).await;
    let admin_p = create_space(&manager, "esp_p", "pat", false).await;
    Fx {
        app,
        manager,
        tmp,
        admin_a,
        admin_b,
        admin_p,
    }
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

async fn search(f: &Fx, tok: &str, route: &str, q: &str) -> (StatusCode, serde_json::Value) {
    send(
        &f.app,
        "POST",
        route,
        tok,
        Some(serde_json::json!({"query": q, "limit": 20})),
    )
    .await
}

async fn space_dir(f: &Fx, id: &str) -> std::path::PathBuf {
    f.manager.get(id).await.unwrap().storage_path
}

fn all_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                all_files(&p, out);
            } else {
                out.push(p);
            }
        }
    }
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Files under `dir` whose raw bytes contain `needle`.
fn files_containing(dir: &std::path::Path, needle: &str) -> Vec<String> {
    let mut files = Vec::new();
    all_files(dir, &mut files);
    files
        .into_iter()
        .filter(|p| {
            std::fs::read(p)
                .map(|b| contains(&b, needle.as_bytes()))
                .unwrap_or(false)
        })
        .map(|p| p.display().to_string())
        .collect()
}

async fn checkpoint(db: &std::path::Path) {
    let id = xavier::memory::sqlite_vec_store::project_id_for_path(db);
    xavier::codebase::connection_manager::ConnectionManager::global()
        .with_conn(&id, |c| {
            c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
            Ok(())
        })
        .await
        .unwrap();
}

fn mem_record(
    ws: &str,
    id: &str,
    path: &str,
    content: &str,
) -> xavier::memory::store::MemoryRecord {
    xavier::memory::store::MemoryRecord {
        id: id.to_string(),
        workspace_id: ws.to_string(),
        path: path.to_string(),
        content: content.to_string(),
        ..Default::default()
    }
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn encrypted_space_writes_no_plaintext_and_reads_back() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    let canary_text = format!("note {CANARY} for the owner");
    let (st, id1) = add(&f, &f.admin_a, "/memory/add", "notes/one", &canary_text).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = add(&f, &f.admin_a, "/v1/memories", "notes/two", &canary_text).await;
    assert_eq!(st, StatusCode::OK);
    assert!(!id1.is_empty());

    let dir = space_dir(&f, "esp_a").await;
    // Before and after a WAL checkpoint, every file under the space dir.
    assert_eq!(files_containing(&dir, CANARY), Vec::<String>::new());
    checkpoint(&dir.join("memory.sqlite")).await;
    assert_eq!(files_containing(&dir, CANARY), Vec::<String>::new());
    // No persistent conversations database for an encrypting space.
    assert!(!dir.join("conversations.db").exists());

    // The owner's token still reads and searches plaintext.
    for route in ["/memory/search", "/v1/memories/search"] {
        let (st, body) = search(&f, &f.admin_a, route, "owner").await;
        assert_eq!(st, StatusCode::OK, "{body}");
        assert!(
            body["results"].to_string().contains(CANARY),
            "{route}: {body}"
        );
    }
    let (st, body) = send(
        &f.app,
        "GET",
        &format!("/memory/get?id={id1}"),
        &f.admin_a,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(body.to_string().contains(CANARY), "{body}");

    // The row is sealed under the space marker, not the default/node key.
    let db = dir.join("memory.sqlite");
    let conn = rusqlite::Connection::open(&db).unwrap();
    let (content, dek): (String, Vec<u8>) = conn
        .query_row(
            "SELECT content, encrypted_dek FROM memory_records LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(content.starts_with("xr1:"), "{content}");
    assert!(dek.starts_with(b"XSK1"), "{dek:?}");
    let plain_rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_records WHERE encrypted_dek IS NULL OR length(encrypted_dek) = 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        plain_rows, 0,
        "every record of an encrypting space is sealed"
    );
    // Content search index is path-only; no extracted entities for sealed rows.
    let fts: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_fts WHERE content != ''",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(fts, 0);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn revisions_checkpoints_and_snapshots_are_sealed() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    let ring = f.manager.key_ring().expect("ring");
    let dir = space_dir(&f, "esp_a").await;
    let db = dir.join("side.sqlite");
    let store = VecSqliteMemoryStore::new(VecSqliteStoreConfig {
        path: db.clone(),
        embedding_dimensions: 1536,
    })
    .await
    .unwrap()
    .with_space_crypto(SpaceCrypto::new("esp_a", ring.clone()));
    store
        .put(mem_record(
            "esp_a",
            "rec1",
            "n/rev",
            &format!("first {CANARY}"),
        ))
        .await
        .unwrap();
    // The update path pushes the previous content into `revisions`.
    store
        .update(mem_record(
            "esp_a",
            "rec1",
            "n/rev",
            &format!("second {CANARY2}"),
        ))
        .await
        .unwrap();
    store
        .save_checkpoint(
            "esp_a",
            xavier::checkpoint::Checkpoint {
                task_id: "t".into(),
                name: "c".into(),
                data: serde_json::json!({"note": "ckpt-canary-91c0"}),
            },
        )
        .await
        .unwrap();
    store
        .save_entity_graph_snapshot("esp_a", "{\"snap\":\"snap-canary-44d1\"}")
        .await
        .unwrap();
    checkpoint(&db).await;
    for c in [CANARY, CANARY2, "ckpt-canary-91c0", "snap-canary-44d1"] {
        assert_eq!(files_containing(&dir, c), Vec::<String>::new(), "{c}");
    }
    // Round trip through the store, revisions included.
    let rec = store.get("esp_a", "rec1").await.unwrap().unwrap();
    assert!(rec.content.contains(CANARY2), "{}", rec.content);
    assert!(
        serde_json::to_string(&rec.revisions)
            .unwrap()
            .contains(CANARY2),
        "revisions lost: {:?}",
        rec.revisions
    );
    let ck = store
        .load_checkpoint("esp_a", "t", "c")
        .await
        .unwrap()
        .unwrap();
    assert!(ck.data.to_string().contains("ckpt-canary-91c0"));
    let snap = store
        .load_entity_graph_snapshot("esp_a")
        .await
        .unwrap()
        .unwrap();
    assert!(snap.contains("snap-canary-44d1"));
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn other_keys_cannot_open_a_spaces_rows_even_if_the_file_is_copied() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    let (st, id) = add(
        &f,
        &f.admin_a,
        "/memory/add",
        "notes/x",
        &format!("x {CANARY}"),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let dir_a = space_dir(&f, "esp_a").await;
    checkpoint(&dir_a.join("memory.sqlite")).await;
    let copy = f.tmp.path().join("copy.sqlite");
    std::fs::copy(dir_a.join("memory.sqlite"), &copy).unwrap();
    let cfg = |p: &std::path::Path| VecSqliteStoreConfig {
        path: p.to_path_buf(),
        embedding_dimensions: 1536,
    };
    let ring = f.manager.key_ring().unwrap();

    // Positive control: A's own key opens the copy.
    let a = VecSqliteMemoryStore::new(cfg(&copy))
        .await
        .unwrap()
        .with_space_crypto(SpaceCrypto::new("esp_a", ring.clone()));
    let rec = a.get("esp_a", &id).await.unwrap().unwrap();
    assert!(rec.content.contains(CANARY));

    // B's own (different) key cannot, and the row is never served as content.
    let copy_b = f.tmp.path().join("copy_b.sqlite");
    std::fs::copy(&copy, &copy_b).unwrap();
    let b = VecSqliteMemoryStore::new(cfg(&copy_b))
        .await
        .unwrap()
        .with_space_crypto(SpaceCrypto::new("esp_b", ring.clone()));
    assert!(b.get("esp_a", &id).await.is_err());
    for r in b.list("esp_a").await.unwrap() {
        assert!(!r.content.contains(CANARY) && !r.content.starts_with("xr1:"));
        assert!(xavier::memory::sqlite_vec_store::at_rest::is_locked_placeholder(&r));
    }
    // Same key, wrong space id in the AD.
    let copy_c = f.tmp.path().join("copy_c.sqlite");
    std::fs::copy(&copy, &copy_c).unwrap();
    let c = VecSqliteMemoryStore::new(cfg(&copy_c))
        .await
        .unwrap()
        .with_space_crypto(SpaceCrypto::new("esp_p", ring.clone()));
    assert!(c.get("esp_a", &id).await.is_err());

    // The default (node-level) store cannot open it either.
    let copy_d = f.tmp.path().join("copy_d.sqlite");
    std::fs::copy(&copy, &copy_d).unwrap();
    let d = VecSqliteMemoryStore::new(cfg(&copy_d)).await.unwrap();
    assert!(d.get("esp_a", &id).await.is_err());
    for r in d.list("esp_a").await.unwrap() {
        assert!(!r.content.contains(CANARY) && !r.content.starts_with("xr1:"));
    }
    // A default-keyed store never writes under a space key: its rows are not
    // XSK1 and a space store refuses them.
    let mut plain = mem_record("esp_a", "dflt", "n/d", "default row");
    d.put(plain.clone()).await.unwrap();
    plain.content = String::new();
    let wrong = a.get("esp_a", "dflt").await;
    match wrong {
        Err(_) => {}
        Ok(Some(r)) => assert!(!r.content.contains("default row")),
        Ok(None) => {}
    }
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn locked_space_is_423_and_leaks_nothing() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    let (st, id) = add(
        &f,
        &f.admin_a,
        "/memory/add",
        "notes/l",
        &format!("l {CANARY}"),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let ring = f.manager.key_ring().unwrap();
    ring.forget("esp_a");
    assert!(ring.is_locked("esp_a"));

    // A locked space's own token cannot even authenticate (the token store is
    // sealed with the same key): 401 at the door, or 423 if it got further.
    // Never any plaintext.
    let locked_status = |st: StatusCode| st == StatusCode::LOCKED || st == StatusCode::UNAUTHORIZED;
    let (st, body) = add_raw(&f, "/memory/add", "notes/new", &format!("n {CANARY2}")).await;
    assert!(locked_status(st), "{st} {body}");
    let (st, body) = search(&f, &f.admin_a, "/memory/search", CANARY).await;
    assert!(locked_status(st), "{st} {body}");
    assert!(!body.to_string().contains(CANARY));
    let (st, body) = send(
        &f.app,
        "GET",
        &format!("/memory/get?id={id}"),
        &f.admin_a,
        None,
    )
    .await;
    assert!(locked_status(st), "{st} {body}");
    assert!(!body.to_string().contains(CANARY));
    // The workspace resolver itself reports Locked (the 423 source).
    let err = xavier::workspace::space_workspace_context(&f.manager, "esp_a")
        .await
        .err()
        .expect("locked");
    assert_eq!(err, xavier::workspace::SpaceScopeError::Locked);

    // Store level: a locked key means writes fail and reads are placeholders.
    let dir = space_dir(&f, "esp_a").await;
    checkpoint(&dir.join("memory.sqlite")).await;
    let copy = f.tmp.path().join("locked.sqlite");
    std::fs::copy(dir.join("memory.sqlite"), &copy).unwrap();
    let s = VecSqliteMemoryStore::new(VecSqliteStoreConfig {
        path: copy.clone(),
        embedding_dimensions: 1536,
    })
    .await
    .unwrap()
    .with_space_crypto(SpaceCrypto::new("esp_a", ring.clone()));
    assert!(s
        .put(mem_record("esp_a", "w1", "n/w", "must not be stored"))
        .await
        .is_err());
    assert!(s.get("esp_a", &id).await.is_err());
    for r in s.list("esp_a").await.unwrap() {
        assert!(!r.content.contains(CANARY) && !r.content.starts_with("xr1:"));
    }
    assert_eq!(
        files_containing(f.tmp.path(), "must not be stored"),
        Vec::<String>::new()
    );

    // Unlock: served again.
    ring.unlock_with_node("esp_a").unwrap();
    let (st, body) = search(&f, &f.admin_a, "/memory/search", CANARY).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(body["results"].to_string().contains(CANARY));
}

async fn add_raw(f: &Fx, route: &str, path: &str, text: &str) -> (StatusCode, serde_json::Value) {
    send(
        &f.app,
        "POST",
        route,
        &f.admin_a,
        Some(serde_json::json!({"content": text, "path": path})),
    )
    .await
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn unencrypted_space_is_unchanged() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    let (st, id) = add(&f, &f.admin_p, "/memory/add", "n/p", "plain okapi record").await;
    assert_eq!(st, StatusCode::OK);
    let (st, body) = search(&f, &f.admin_p, "/memory/search", "plain okapi").await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(
        body["results"].to_string().contains("plain okapi"),
        "{body}"
    );
    let (st, _) = send(
        &f.app,
        "GET",
        &format!("/memory/get?id={id}"),
        &f.admin_p,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    // Space B (encrypted) sees nothing of it.
    let (_, body) = search(&f, &f.admin_b, "/memory/search", "plain okapi").await;
    assert!(!body["results"].to_string().contains("okapi"), "{body}");
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn linked_search_decrypts_server_side_and_skips_locked() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    let (st, _) = add(
        &f,
        &f.admin_a,
        "/memory/add",
        "notes/l",
        &format!("linked {CANARY}"),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    // A grants B read access (grantor = esp_a, grantee = esp_b).
    let (st, resp) = send(
        &f.app,
        "POST",
        "/api/v1/espacio/spaces/esp_a/links",
        &f.admin_a,
        Some(serde_json::json!({"target_space": "esp_b"})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{resp}");
    let linked = |tok: String| {
        let app = f.app.clone();
        async move {
            send(
                &app,
                "POST",
                "/memory/search",
                &tok,
                Some(serde_json::json!({"query": CANARY, "limit": 20, "include_linked": true})),
            )
            .await
        }
    };
    let (st, body) = linked(f.admin_b.clone()).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(body["results"].to_string().contains(CANARY), "{body}");
    assert!(
        body["results"].to_string().contains("esp_a"),
        "result must be tagged with its source space: {body}"
    );
    // B's own files never hold A's content.
    let dir_b = space_dir(&f, "esp_b").await;
    assert_eq!(files_containing(&dir_b, CANARY), Vec::<String>::new());

    // A locked: the link is skipped, nothing of A is returned.
    f.manager.key_ring().unwrap().forget("esp_a");
    let (st, body) = linked(f.admin_b.clone()).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(!body["results"].to_string().contains(CANARY), "{body}");
    assert!(
        body["linked_skipped"]
            .as_array()
            .map(|a| a
                .iter()
                .any(|s| s["space"] == "esp_a" && s["reason"] == "Locked"))
            .unwrap_or(false),
        "{body}"
    );
}

// ---- review fixes: symbols, beliefs/tokens, unmarked rows, embeddings ----

async fn side_store(f: &Fx, name: &str, space: &str) -> (VecSqliteMemoryStore, std::path::PathBuf) {
    let dir = space_dir(f, space).await;
    let db = dir.join(name);
    let store = VecSqliteMemoryStore::new(VecSqliteStoreConfig {
        path: db.clone(),
        embedding_dimensions: 1536,
    })
    .await
    .unwrap()
    .with_space_crypto(SpaceCrypto::new(space, f.manager.key_ring().unwrap()));
    (store, db)
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn symbols_path_never_writes_sealed_words() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    let (store, db) = side_store(&f, "sym.sqlite", "esp_a").await;
    store
        .put(mem_record(
            "esp_a",
            "s1",
            "n/s",
            "Zxqcanaryword Qwvcanaryother tokens",
        ))
        .await
        .unwrap();
    assert!(store.symbols_for_memory("s1").await.unwrap().is_empty());
    checkpoint(&db).await;
    let conn = rusqlite::Connection::open(&db).unwrap();
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM memory_symbol_links", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
    drop(conn);
    assert_eq!(
        files_containing(&space_dir(&f, "esp_a").await, "Zxqcanaryword"),
        Vec::<String>::new()
    );

    // Default store, node-key sealed private row: same guarantee.
    let dflt = f.tmp.path().join("dflt-sym.sqlite");
    let d = VecSqliteMemoryStore::new(VecSqliteStoreConfig {
        path: dflt.clone(),
        embedding_dimensions: 1536,
    })
    .await
    .unwrap();
    d.put(mem_record(
        "ws",
        "d1",
        "n/d",
        "Zxqdefaultword Qwvdefaultother",
    ))
    .await
    .unwrap();
    assert!(d.symbols_for_memory("d1").await.unwrap().is_empty());
    let conn = rusqlite::Connection::open(&dflt).unwrap();
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM memory_symbol_links", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn beliefs_and_session_tokens_are_not_stored_in_clear() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    let (store, db) = side_store(&f, "bel.sqlite", "esp_a").await;
    let mut edge = xavier::domain::memory::belief::BeliefEdge::new(
        "BeliefSrcCanary".to_string(),
        "BeliefDstCanary".to_string(),
        "relcanary_rel".to_string(),
        0.5,
        "prov-canary".to_string(),
    );
    edge.id = "edge1".to_string();
    store
        .save_beliefs("esp_a", vec![edge.clone()])
        .await
        .unwrap();
    let now = chrono::Utc::now();
    store
        .save_session_token(
            "esp_a",
            xavier::memory::store::SessionTokenRecord {
                token: "SessionTokCanary".to_string(),
                created_at: now,
                expires_at: now + chrono::Duration::hours(1),
            },
        )
        .await
        .unwrap();
    checkpoint(&db).await;
    let dir = space_dir(&f, "esp_a").await;
    for c in [
        "BeliefSrcCanary",
        "BeliefDstCanary",
        "relcanary_rel",
        "prov-canary",
        "SessionTokCanary",
    ] {
        assert_eq!(files_containing(&dir, c), Vec::<String>::new(), "{c}");
    }
    // Round trip and token validation still work.
    let st = store.load_workspace_state("esp_a").await.unwrap();
    assert_eq!(st.beliefs.len(), 1);
    assert_eq!(st.beliefs[0].source, "BeliefSrcCanary");
    assert_eq!(st.beliefs[0].target, "BeliefDstCanary");
    assert_eq!(st.beliefs[0].relation_type, "relcanary_rel");
    assert!(store
        .is_session_token_valid("esp_a", "SessionTokCanary")
        .await
        .unwrap());
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn unmarked_and_foreign_rows_are_never_served() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    let (store, db) = side_store(&f, "inj.sqlite", "esp_a").await;
    store
        .put(mem_record("esp_a", "r1", "n/1", "real sealed"))
        .await
        .unwrap();
    store
        .put(mem_record("esp_a", "r2", "n/2", "real sealed two"))
        .await
        .unwrap();
    store
        .save_checkpoint(
            "esp_a",
            xavier::checkpoint::Checkpoint {
                task_id: "t".into(),
                name: "c".into(),
                data: serde_json::json!({"k": "v"}),
            },
        )
        .await
        .unwrap();
    store
        .save_entity_graph_snapshot("esp_a", "{\"s\":1}")
        .await
        .unwrap();
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        // Unmarked plaintext row.
        conn.execute(
            "UPDATE memory_records SET encrypted_dek = NULL, content = 'INJECTED-PLAIN', metadata = '{}' WHERE id = 'r1'",
            [],
        )
        .unwrap();
        // A default-space (XDK2) marker on a space row.
        conn.execute(
            "UPDATE memory_records SET encrypted_dek = X'58444b32' WHERE id = 'r2'",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE checkpoint_records SET data = '{\"k\":\"INJECTED-CK\"}'",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE entity_graph_snapshots SET data = 'INJECTED-SNAP'",
            [],
        )
        .unwrap();
    }
    for id in ["r1", "r2"] {
        match store.get("esp_a", id).await {
            Err(_) => {}
            Ok(Some(r)) => {
                assert!(!r.content.contains("INJECTED") && !r.content.contains("real sealed"))
            }
            Ok(None) => {}
        }
    }
    for r in store.list("esp_a").await.unwrap() {
        assert!(!r.content.contains("INJECTED") && !r.content.contains("real sealed"));
        assert!(xavier::memory::sqlite_vec_store::at_rest::is_locked_placeholder(&r));
    }
    assert!(store.load_checkpoint("esp_a", "t", "c").await.is_err());
    assert!(store.load_entity_graph_snapshot("esp_a").await.is_err());
}

#[tokio::test]
async fn disabled_conversations_refuse_everything_and_create_no_file() {
    let db = xavier::codebase::conversations_db::ConversationsDb::open_disabled("esp_a").unwrap();
    assert!(db.is_disabled());
    assert!(db.create_thread(Some("t"), None, None).await.is_err());
    assert!(db.create_schema().await.is_err());
    assert!(db.list_threads(10).await.is_err());
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn private_text_is_not_embedded_by_a_remote_provider() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    let saved: Vec<(&str, Option<String>)> = [
        "XAVIER_EMBEDDING_PROVIDER_MODE",
        "XAVIER_EMBEDDER",
        "XAVIER_EMBEDDING_URL",
    ]
    .iter()
    .map(|k| (*k, std::env::var(k).ok()))
    .collect();
    std::env::remove_var("XAVIER_EMBEDDING_PROVIDER_MODE");
    std::env::set_var("XAVIER_EMBEDDER", "openrouter");
    std::env::set_var("XAVIER_EMBEDDING_URL", "https://embeddings.example.invalid");
    assert!(
        !xavier::embedding::embedder_is_local_only(),
        "a cloud provider must not count as local"
    );
    let (store, _db) = side_store(&f, "emb.sqlite", "esp_a").await;
    let t0 = std::time::Instant::now();
    store
        .put(mem_record("esp_a", "e1", "n/e", "private embed canary"))
        .await
        .unwrap();
    let elapsed = t0.elapsed();
    let rec = store.get("esp_a", "e1").await.unwrap().unwrap();
    for (k, v) in saved {
        match v {
            Some(v) => std::env::set_var(k, v),
            None => std::env::remove_var(k),
        }
    }
    assert!(rec.embedding.is_empty());
    assert_eq!(rec.embedding_status, "pending");
    assert_eq!(
        rec.embedding_attempts, 0,
        "no request may even be attempted"
    );
    assert!(elapsed < std::time::Duration::from_secs(2), "{elapsed:?}");
}
