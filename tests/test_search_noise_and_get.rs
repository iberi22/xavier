//! Regression tests for two production issues found in the F0 context-quality
//! audit (2026-09-24):
//!
//! 1. General search was dominated by auto-generated telemetry/noise records
//!    (`activity/*`, `gestalt/thinking/*`, `[activity] ...` event-bus lines)
//!    for almost any query, drowning out actually relevant decisions/plans.
//! 2. `GET /memory/get?path=...` 404'd for every path, including paths that
//!    `POST /memory/add` had just confirmed with `status: "ok"` — the route
//!    was never registered.
//!
//! These tests build a small in-process dataset (noise + relevant docs) and
//! drive the real HTTP handlers end to end, the same way the fixes were
//! verified against production.

use axum::{body::Body, http::Request, routing::get, routing::post, Router};
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;

use xavier::agents::provider::router::{ProviderKind, ProviderRouter};
use xavier::agents::rate_limit::RateLimitManager;
use xavier::app::proxy_use_case::ProxyUseCase;
use xavier::app::qmd_memory_adapter::QmdMemoryAdapter;
use xavier::cli::handlers::memory::{add_handler, get_handler, search_handler};
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

/// Builds a standalone `CliState` (same shape as `test_store_path_hierarchy.rs`)
/// backed by a temp dir + in-memory sqlite-vec store, no embedder configured
/// (so search exercises the deterministic lexical/BM25 path).
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

async fn add_memory(
    app: &Router,
    path: &str,
    content: &str,
    metadata: serde_json::Value,
) -> serde_json::Value {
    let payload = serde_json::json!({
        "content": content,
        "path": path,
        "metadata": metadata,
    });
    let req = Request::builder()
        .method("POST")
        .uri("/memory/add")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["status"], "ok", "add failed: {json}");
    json
}

async fn search(app: &Router, body: serde_json::Value) -> serde_json::Value {
    let req = Request::builder()
        .method("POST")
        .uri("/memory/search")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::OK);
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&body_bytes).unwrap()
}

/// Seeds a small dataset that mirrors the production shape reported in the
/// audit: a handful of "noisy" auto-generated telemetry records that mention
/// the exact same domain vocabulary as the one genuinely relevant document,
/// so a naive search has every incentive to surface the noise instead.
async fn seed_noise_and_relevant_docs(app: &Router) {
    // The one relevant, hand-authored decision a real query should surface.
    add_memory(
        app,
        "decisions/swal-network/2026-09-24-auditoria-gaps",
        "Auditoria SWAL.network 2026-09-24: Maloca y Atlas necesitan tareas \
         de mesh pendientes antes del despliegue de nodos.",
        serde_json::json!({"type": "decision"}),
    )
    .await;

    // Noise 1: path-based exclusion (`gestalt/thinking/*`), long and
    // repeats the query vocabulary many times so raw term-frequency scoring
    // would otherwise rank it above the short relevant doc.
    let verbose_thinking_loop = "Maloca Atlas mesh tareas ".repeat(80);
    add_memory(
        app,
        "gestalt/thinking/2026-09-24_120000",
        &format!("# Xavier Thinking Loop Insight — 2026-09-24\n{verbose_thinking_loop}"),
        serde_json::json!({"kind": "document"}),
    )
    .await;

    // Noise 2: path-based exclusion (`activity/*`).
    add_memory(
        app,
        "activity/2026-09-24/some-event",
        "Maloca Atlas mesh tareas activity event log entry",
        serde_json::json!({}),
    )
    .await;

    // Noise 3: metadata-based exclusion (`event_type: "activity"`), path
    // does not match either noise prefix.
    add_memory(
        app,
        "gestalt/bus/executions/2026-09-24T00-00-00Z-ses_test",
        "[activity] opencode ses_test — opencode session.updated: Maloca Atlas mesh tareas",
        serde_json::json!({"event_type": "activity"}),
    )
    .await;

    // Noise 4: content-prefix-based exclusion (`[activity]`), no matching
    // path prefix and no `event_type`/`type` metadata tag either — this is
    // the defense-in-depth case for producers that didn't tag their record.
    add_memory(
        app,
        "misc/untagged-activity-line",
        "[activity] some producer forgot to set event_type: Maloca Atlas mesh tareas",
        serde_json::json!({}),
    )
    .await;
}

/// Requirement F0.1: a query for real ecosystem content returns the relevant
/// document, not the telemetry/noise namespaces, by default.
#[tokio::test]
async fn search_excludes_noise_namespaces_by_default() {
    let temp = tempfile::tempdir().unwrap();
    let state = create_test_cli_state(&temp).await;
    let app = Router::new()
        .route("/memory/add", post(add_handler))
        .route("/memory/search", post(search_handler))
        .with_state(state.clone());

    seed_noise_and_relevant_docs(&app).await;

    let res = search(
        &app,
        serde_json::json!({"query": "maloca atlas mesh tareas", "limit": 5}),
    )
    .await;

    let results = res["results"].as_array().expect("results array");
    assert!(
        !results.is_empty(),
        "expected at least the relevant decision doc, got none: {res}"
    );

    let paths: Vec<&str> = results
        .iter()
        .map(|r| r["path"].as_str().unwrap_or_default())
        .collect();

    assert_eq!(
        paths[0], "decisions/swal-network/2026-09-24-auditoria-gaps",
        "relevant decision doc should rank first; got order: {paths:?}"
    );

    for path in &paths {
        assert!(
            !path.starts_with("activity/") && !path.starts_with("gestalt/thinking/"),
            "noise namespace leaked into default search results: {path}"
        );
    }
    assert!(
        !paths.contains(&"gestalt/bus/executions/2026-09-24T00-00-00Z-ses_test"),
        "event_type=activity record should be excluded by default: {paths:?}"
    );
    assert!(
        !paths.contains(&"misc/untagged-activity-line"),
        "[activity]-prefixed content should be excluded by default: {paths:?}"
    );
}

/// The `include_activity` opt-in must bring the noise back when explicitly
/// requested (e.g. an activity dashboard), proving this is a filter, not a
/// silent data loss.
#[tokio::test]
async fn search_include_activity_opts_back_in() {
    let temp = tempfile::tempdir().unwrap();
    let state = create_test_cli_state(&temp).await;
    let app = Router::new()
        .route("/memory/add", post(add_handler))
        .route("/memory/search", post(search_handler))
        .with_state(state.clone());

    seed_noise_and_relevant_docs(&app).await;

    let res = search(
        &app,
        serde_json::json!({
            "query": "maloca atlas mesh tareas",
            "limit": 10,
            "include_activity": true,
        }),
    )
    .await;

    let results = res["results"].as_array().expect("results array");
    let paths: Vec<&str> = results
        .iter()
        .map(|r| r["path"].as_str().unwrap_or_default())
        .collect();

    assert!(
        paths.iter().any(|p| p.starts_with("gestalt/thinking/")),
        "include_activity=true should surface gestalt/thinking/* again: {paths:?}"
    );
    assert!(
        paths.iter().any(|p| p.starts_with("activity/")),
        "include_activity=true should surface activity/* again: {paths:?}"
    );
}

/// Requirement F0.2: `POST /memory/add` followed by `GET /memory/get?path=`
/// returns 200 with the same content that was just saved (previously 404'd
/// unconditionally — the route was never registered).
#[tokio::test]
async fn add_then_get_by_path_round_trips() {
    let temp = tempfile::tempdir().unwrap();
    let state = create_test_cli_state(&temp).await;
    let app = Router::new()
        .route("/memory/add", post(add_handler))
        .route("/memory/get", get(get_handler))
        .with_state(state.clone());

    let path = "decisions/swal-network/2026-09-24-auditoria-gaps-test";
    let content = "Auditoria SWAL.network 2026-09-24 (test fixture).";
    let add_res = add_memory(&app, path, content, serde_json::json!({"type": "decision"})).await;
    let saved_id = add_res["id"].as_str().unwrap().to_string();
    let saved_path = add_res["path"].as_str().unwrap().to_string();

    // Lookup by path.
    let req = Request::builder()
        .method("GET")
        .uri(format!(
            "/memory/get?path={}",
            urlencoding_encode(&saved_path)
        ))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        axum::http::StatusCode::OK,
        "GET /memory/get?path= must succeed right after a successful POST /memory/add"
    );
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let get_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(get_json["status"], "ok");
    assert_eq!(get_json["path"], saved_path);
    assert_eq!(get_json["content"], content);

    // Lookup by id is also supported.
    let req = Request::builder()
        .method("GET")
        .uri(format!("/memory/get?id={saved_id}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::OK);

    // A path that was never saved must 404, not silently succeed.
    let req = Request::builder()
        .method("GET")
        .uri("/memory/get?path=never/saved/this/path")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::NOT_FOUND);
}

/// Minimal percent-encoding for query values used by these tests (paths use
/// `/` which must stay literal for axum's `Query` extractor to parse the
/// rest of the query string correctly when combined with other params; here
/// we only need to escape characters that are actually unsafe/ambiguous).
fn urlencoding_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
