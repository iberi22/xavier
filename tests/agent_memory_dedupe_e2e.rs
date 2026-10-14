xavier::isolate_test_process!();

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
use chrono::{Duration, TimeZone, Utc};
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
use xavier::memory::dedupe::{
    dedupe_agent_sessions, is_record_forgotten, DedupeAgentSessionsResult,
};
use xavier::memory::file_indexer::{FileChunk, FileIndexer, FileIndexerConfig, IndexedFile};
use xavier::memory::qmd_memory::QmdMemory;
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use xavier::memory::store::{MemoryRecord, MemoryStore};
use xavier::ports::inbound::AgentLifecyclePort;
use xavier::secrets::audit::QmdAuditLogger;
use xavier::tasks::store::{InMemoryTaskStore, TaskService};
use xavier::workspace::{WorkspaceContext, WorkspaceState};

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

async fn create_test_state(temp_dir: &TempDir) -> CliState {
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

fn make_record(
    id: &str,
    path: &str,
    content: &str,
    updated_at: chrono::DateTime<Utc>,
    last_modified: &str,
) -> MemoryRecord {
    MemoryRecord {
        id: id.to_string(),
        workspace_id: "test-ws".to_string(),
        path: path.to_string(),
        content: content.to_string(),
        metadata: serde_json::json!({
            "source": "agent_scanner",
            "last_modified": last_modified,
            "size": 1,
        }),
        embedding: vec![],
        created_at: updated_at,
        updated_at,
        revision: 1,
        primary: true,
        deleted_at: None,
        ..Default::default()
    }
}

#[tokio::test]
async fn test_e2e_agent_memory_dedupe_lifecycle_points_a_to_d() {
    let temp = TempDir::new().unwrap();
    let _env = RestoreEnv::set(&[("HOME", temp.path()), ("XAVIER_DATA_DIR", temp.path())]);
    let state = create_test_state(&temp).await;
    let store = &*state.store;

    let path_alpha = "agent_memory://cursor/session-alpha";
    let path_beta = "agent_memory://cursor/session-beta";

    let t1 = Utc.with_ymd_and_hms(2026, 10, 1, 10, 0, 0).unwrap();
    let t2 = t1 + Duration::hours(1);
    let t3 = t1 + Duration::hours(2);

    let mtime_1 = "2026-10-01T10:00:00Z";
    let mtime_2 = "2026-10-01T11:00:00Z";
    let mtime_3 = "2026-10-01T12:00:00Z";

    // ─────────────────────────────────────────────────────────────────
    // Point 4a: Crear 3 filas para la misma ruta y 1 ruta única;
    //           dry-run no cambia nada; apply deja 1 vigente por ruta.
    // ─────────────────────────────────────────────────────────────────
    store
        .put(make_record("alpha-1", path_alpha, "Content 1", t1, mtime_1))
        .await
        .unwrap();
    store
        .put(make_record("alpha-2", path_alpha, "Content 2", t2, mtime_2))
        .await
        .unwrap();
    store
        .put(make_record("alpha-3", path_alpha, "Content 3", t3, mtime_3))
        .await
        .unwrap();
    store
        .put(make_record(
            "beta-1",
            path_beta,
            "Beta content",
            t2,
            mtime_2,
        ))
        .await
        .unwrap();

    let all_init = store.list("test-ws").await.unwrap();
    assert_eq!(all_init.len(), 4, "Initially 4 rows in store");

    // Dry-run: does not change anything
    let dry_run_res = dedupe_agent_sessions(store, "test-ws", true).await.unwrap();
    assert_eq!(
        dry_run_res,
        DedupeAgentSessionsResult {
            routes: 2,
            to_forget: 2,
            to_keep: 2,
        }
    );

    let all_after_dry = store.list("test-ws").await.unwrap();
    assert_eq!(all_after_dry.len(), 4);
    for r in &all_after_dry {
        assert!(
            !is_record_forgotten(r),
            "Dry-run must not mutate any deleted_at in store"
        );
    }

    // Apply: leaves 1 vigente per route
    let apply_res = dedupe_agent_sessions(store, "test-ws", false)
        .await
        .unwrap();
    assert_eq!(
        apply_res,
        DedupeAgentSessionsResult {
            routes: 2,
            to_forget: 2,
            to_keep: 2,
        }
    );

    let all_after_apply = store.list("test-ws").await.unwrap();
    assert_eq!(all_after_apply.len(), 4, "No physical deletions allowed");

    let alpha_active: Vec<_> = all_after_apply
        .iter()
        .filter(|r| r.path == path_alpha && !is_record_forgotten(r))
        .collect();
    let alpha_forgotten: Vec<_> = all_after_apply
        .iter()
        .filter(|r| r.path == path_alpha && is_record_forgotten(r))
        .collect();

    assert_eq!(alpha_active.len(), 1, "Exactly 1 vigente row for alpha");
    assert_eq!(alpha_active[0].id, "alpha-3", "Most recent updated_at kept");
    assert_eq!(alpha_forgotten.len(), 2, "2 older rows marked forgotten");

    let beta_active: Vec<_> = all_after_apply
        .iter()
        .filter(|r| r.path == path_beta && !is_record_forgotten(r))
        .collect();
    assert_eq!(beta_active.len(), 1, "Exactly 1 vigente row for beta");
    assert_eq!(beta_active[0].id, "beta-1");

    // ─────────────────────────────────────────────────────────────────
    // Point 4b: Re-escaneo de fuente sin cambios: no crea filas nuevas
    //           y no resucita las olvidadas.
    // ─────────────────────────────────────────────────────────────────
    let file_alpha_unchanged = IndexedFile {
        path: path_alpha.to_string(),
        content: "Content 3".to_string(),
        chunks: vec![],
        last_modified: mtime_3.to_string(),
        size: 1,
    };

    let rescan_res = state
        .agent_indexer
        .ingest_file(store, "test-ws", &file_alpha_unchanged)
        .await
        .unwrap();
    assert_eq!(rescan_res, Some("alpha-3".to_string()));

    let all_after_rescan = store.list("test-ws").await.unwrap();
    assert_eq!(
        all_after_rescan.len(),
        4,
        "Re-scan must not create new rows"
    );

    let alpha_forgotten_after: Vec<_> = all_after_rescan
        .iter()
        .filter(|r| r.path == path_alpha && is_record_forgotten(r))
        .collect();
    assert_eq!(
        alpha_forgotten_after.len(),
        2,
        "Forgotten rows must not be resurrected"
    );

    // ─────────────────────────────────────────────────────────────────
    // Point 4c: Fuente cambiada: actualiza la vigente en sitio (mismo
    //           path), no crea otra.
    // ─────────────────────────────────────────────────────────────────
    let mtime_4 = "2026-10-01T14:00:00Z";
    let file_alpha_changed = IndexedFile {
        path: path_alpha.to_string(),
        content: "Content 3 updated with new messages".to_string(),
        chunks: vec![],
        last_modified: mtime_4.to_string(),
        size: 2,
    };

    let update_res = state
        .agent_indexer
        .ingest_file(store, "test-ws", &file_alpha_changed)
        .await
        .unwrap();
    assert_eq!(
        update_res,
        Some("alpha-3".to_string()),
        "Must update existing vigente ID in place"
    );

    let all_after_update = store.list("test-ws").await.unwrap();
    assert_eq!(
        all_after_update.len(),
        4,
        "Source change must not create a new row"
    );

    let alpha_active_updated: Vec<_> = all_after_update
        .iter()
        .filter(|r| r.path == path_alpha && !is_record_forgotten(r))
        .collect();
    assert_eq!(alpha_active_updated.len(), 1);
    assert_eq!(alpha_active_updated[0].id, "alpha-3");
    assert_eq!(
        alpha_active_updated[0].content,
        "Content 3 updated with new messages"
    );

    // ─────────────────────────────────────────────────────────────────
    // Point 4d: Olvido explícito de una ruta: no vuelve a aparecer
    //           tras re-escaneo.
    // ─────────────────────────────────────────────────────────────────
    // Explicitly forget route beta
    let mut beta_record = store.get("test-ws", path_beta).await.unwrap().unwrap();
    let now = Utc::now();
    beta_record.deleted_at = Some(now);
    if let Some(obj) = beta_record.metadata.as_object_mut() {
        obj.insert(
            "deleted_at".to_string(),
            serde_json::json!(now.to_rfc3339()),
        );
    }
    store.update(beta_record).await.unwrap();

    // Verify beta is now forgotten
    let beta_check = store.get("test-ws", path_beta).await.unwrap().unwrap();
    assert!(is_record_forgotten(&beta_check));

    // Re-scan source for beta without change in source
    let file_beta_unchanged = IndexedFile {
        path: path_beta.to_string(),
        content: "Beta content".to_string(),
        chunks: vec![],
        last_modified: mtime_2.to_string(),
        size: 1,
    };

    let forget_rescan = state
        .agent_indexer
        .ingest_file(store, "test-ws", &file_beta_unchanged)
        .await
        .unwrap();
    assert_eq!(
        forget_rescan, None,
        "Explicitly forgotten route must NOT be resurrected"
    );

    let all_after_forget_rescan = store.list("test-ws").await.unwrap();
    let beta_active_after: Vec<_> = all_after_forget_rescan
        .iter()
        .filter(|r| r.path == path_beta && !is_record_forgotten(r))
        .collect();
    assert_eq!(
        beta_active_after.len(),
        0,
        "Route beta must remain forgotten, not reappear"
    );
}

#[tokio::test]
async fn test_e2e_agent_memory_dedupe_admin_endpoint_auth_point_e() {
    let temp = TempDir::new().unwrap();
    let _env = RestoreEnv::set(&[("HOME", temp.path()), ("XAVIER_DATA_DIR", temp.path())]);
    let root_secret = "test-root-admin-secret-2026";
    let jwt_secret = "jwt-signing-secret-for-tests";
    let _env_str = RestoreEnv::set_str(&[
        ("XAVIER_TOKEN", root_secret),
        ("XAVIER_JWT_SECRET", jwt_secret),
    ]);

    let state = create_test_state(&temp).await;

    // Create a scoped token to test non-admin refusal
    let token_store = xavier::security::tokens::TokenStore::new();
    token_store.init_schema_async().await.unwrap();
    let (xav_token, _) = token_store
        .create_token(
            "scoped_token".into(),
            vec!["read".into(), "write".into()],
            None,
        )
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
    let uri = "/v1/admin/memory/dedupe-agent-sessions";

    // 1. Without token: must be rejected with 401 or 403
    let req_no_auth = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(Body::from(r#"{"dry_run": true}"#))
        .unwrap();
    let res_no_auth = app.clone().oneshot(req_no_auth).await.unwrap();
    assert_eq!(
        res_no_auth.status(),
        StatusCode::UNAUTHORIZED,
        "Request without token must be rejected"
    );

    // 2. With scoped non-admin token: must be rejected with 403 Forbidden
    let req_scoped = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("Authorization", format!("Bearer {}", xav_token))
        .header("Content-Type", "application/json")
        .body(Body::from(r#"{"dry_run": true}"#))
        .unwrap();
    let res_scoped = app.clone().oneshot(req_scoped).await.unwrap();
    assert_eq!(
        res_scoped.status(),
        StatusCode::FORBIDDEN,
        "Scoped non-admin token must be rejected on admin dedupe endpoint"
    );

    // 3. With admin token: dry_run defaults to true
    let req_admin_dry = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("X-Xavier-Token", root_secret)
        .header("Content-Type", "application/json")
        .body(Body::empty())
        .unwrap();
    let res_admin_dry = app.clone().oneshot(req_admin_dry).await.unwrap();
    assert_eq!(
        res_admin_dry.status(),
        StatusCode::OK,
        "Admin token must be accepted"
    );

    let body_bytes = axum::body::to_bytes(res_admin_dry.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body_json["status"], "ok");
    assert_eq!(body_json["dry_run"], true);
    assert!(body_json.get("routes").is_some());
    assert!(body_json.get("to_forget").is_some());
    assert!(body_json.get("to_keep").is_some());

    // 4. With admin token and explicit {"dry_run": false}: accepts and applies
    let req_admin_apply = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("X-Xavier-Token", root_secret)
        .header("Content-Type", "application/json")
        .body(Body::from(r#"{"dry_run": false}"#))
        .unwrap();
    let res_admin_apply = app.clone().oneshot(req_admin_apply).await.unwrap();
    assert_eq!(res_admin_apply.status(), StatusCode::OK);

    let apply_bytes = axum::body::to_bytes(res_admin_apply.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let apply_json: serde_json::Value = serde_json::from_slice(&apply_bytes).unwrap();
    assert_eq!(apply_json["status"], "ok");
    assert_eq!(apply_json["dry_run"], false);
}
