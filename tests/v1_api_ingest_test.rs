//! Regression coverage for secret screening on the memory ingest surface.
//!
//! The deployed `POST /v1/memories` is `cli::server::memory_routes()` ->
//! `cli::handlers::memory::add_handler`, which screens `content` before
//! persisting. A focused handler test also covers `server::v1_api::v1_memories_add`,
//! which is not part of the deployed HTTP router and is exercised in isolation.
//!
//! Mutations (each must turn a test red):
//!   - Delete Step 5 of `SecurityService::process_input` -> the production
//!     block/redaction test fails.
//!   - Delete the screening block of `v1_memories_add` -> the handler test fails.
//!
//! Hermetic: tempdirs, a static test root token, no ambient workspace/daemon state.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware, Extension, Router,
};
use http_body_util::BodyExt;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;

use xavier::adapters::inbound::http::middleware::clearance::clearance_session_middleware;
use xavier::agents::provider::router::{ProviderKind, ProviderRouter};
use xavier::agents::rate_limit::RateLimitManager;
use xavier::agents::RuntimeConfig;
use xavier::app::proxy_use_case::ProxyUseCase;
use xavier::app::qmd_memory_adapter::QmdMemoryAdapter;
use xavier::cli::http_setup::auth_middleware;
use xavier::cli::state::{CliState, CodeGraphState};
use xavier::codebase::conversations_db::ConversationsDb;
use xavier::coordination::KeyLendingEngine;
use xavier::coordination::SimpleAgentRegistry;
use xavier::embedding::NoopEmbedder;
use xavier::memory::agent_indexer::AgentIndexer;
use xavier::memory::file_indexer::{FileIndexer, FileIndexerConfig};
use xavier::memory::qmd_memory::QmdMemory;
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use xavier::memory::store::MemoryBackend;
use xavier::ports::inbound::AgentLifecyclePort;
use xavier::secrets::audit::QmdAuditLogger;
use xavier::server::v1_api::v1_memories_add;
use xavier::settings::types::DedupSettings;
use xavier::tasks::store::{InMemoryTaskStore, TaskService};
use xavier::workspace::{
    EmbeddingProviderMode, PlanTier, SyncPolicy, WorkspaceConfig, WorkspaceContext, WorkspaceState,
};

/// Fake value only, shaped to match the existing "Generic API Key" detector
/// pattern (16+ alphanumerics after `API_KEY=`). Not a real credential.
const FAKE_API_KEY: &str = "FAKEkey0123456789ABCDEFGH";
const ROOT_TOKEN: &str = "wp-ingest-root-token";

async fn create_test_cli_state(temp_dir: &TempDir) -> CliState {
    let docs = Arc::new(tokio::sync::RwLock::new(Vec::new()));
    let qmd_memory = Arc::new(QmdMemory::new_with_workspace(docs, "test-ws"));
    let memory_port = Arc::new(QmdMemoryAdapter::new(Arc::clone(&qmd_memory)));
    let store = Arc::new(
        VecSqliteMemoryStore::new(VecSqliteStoreConfig {
            path: temp_dir.path().join("test_store.db"),
            embedding_dimensions: 1536,
        })
        .await
        .unwrap(),
    );
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

/// Same layer order as the production memory surface: auth (outer) then
/// clearance, over the real `memory_routes()`.
async fn production_fixture() -> (Router, CliState, TempDir) {
    std::env::set_var("XAVIER_TOKEN", ROOT_TOKEN);
    std::env::set_var("XAVIER_JWT_SECRET", "wp-ingest-jwt-secret-0123456789abcdef");
    std::env::set_var("XAVIER_EMBEDDING_PROVIDER_MODE", "disabled");
    std::env::remove_var("XAVIER_SPACES");
    let tmp = TempDir::new().unwrap();
    let state = create_test_cli_state(&tmp).await;
    let protected = xavier::cli::server::memory_routes()
        .layer(middleware::from_fn(clearance_session_middleware))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    let app = Router::new().merge(protected).with_state(state.clone());
    (app, state, tmp)
}

async fn send(
    app: &Router,
    path: &str,
    token: Option<&str>,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder().method("POST").uri(path);
    if let Some(token) = token {
        builder = builder.header("X-Xavier-Token", token);
    }
    let req = builder
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn production_v1_memories_blocks_and_drops_secret() {
    let (app, state, _tmp) = production_fixture().await;
    let path = "security/ingest-blocked";

    let (status, body) = send(
        &app,
        "/v1/memories",
        Some(ROOT_TOKEN),
        serde_json::json!({"content": format!("API_KEY={FAKE_API_KEY}"), "path": path}),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["status"].as_str(),
        Some("blocked"),
        "a payload carrying a bare credential must be blocked: {body}"
    );
    let docs = state.qmd_memory.all_documents().await;
    assert!(
        !docs.iter().any(|d| d.path == path),
        "a blocked payload must not be persisted"
    );
    assert!(
        !docs.iter().any(|d| d.content.contains(FAKE_API_KEY)),
        "the raw credential must never be stored"
    );

    // A write without the root token, or with a forged one, never succeeds.
    let write = serde_json::json!({"content": "unauthorized write", "path": "security/noauth"});
    let (status, _) = send(&app, "/v1/memories", None, write.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(&app, "/v1/memories", Some("forged-token"), write).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn e3_secret_value_never_returned_by_memory_search() {
    let (app, state, _tmp) = production_fixture().await;
    let marker = "e3markerquaysidebramble9271";

    // A credential-bearing write alongside the marker must be screened out
    // (same fake-secret shape as the block test above).
    let secret_path = "security/e3-secret-marker";
    let (status, body) = send(
        &app,
        "/v1/memories",
        Some(ROOT_TOKEN),
        serde_json::json!({"content": format!("API_KEY={FAKE_API_KEY} note {marker}"), "path": secret_path}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"].as_str(), Some("blocked"), "{body}");

    // A benign memory carrying the same marker, so search is not trivially empty.
    let benign_path = "security/e3-benign-marker";
    let benign_content = format!("benign harbour lighthouse note {marker}");
    let (status, body) = send(
        &app,
        "/v1/memories",
        Some(ROOT_TOKEN),
        serde_json::json!({"content": benign_content, "path": benign_path}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"].as_str(), Some("ok"), "{body}");

    // Search via the production router the fixture serves (`memory_routes`).
    let (status, body) = send(
        &app,
        "/memory/search",
        Some(ROOT_TOKEN),
        serde_json::json!({"query": marker}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let raw = serde_json::to_string(&body).unwrap();
    assert!(
        !raw.contains(FAKE_API_KEY),
        "the raw credential value must never come back from search"
    );
    assert!(
        !raw.contains(secret_path),
        "the blocked secret memory must not appear in search results"
    );
    let results = body["results"].as_array().cloned().unwrap_or_default();
    assert!(
        results
            .iter()
            .any(|r| r["path"].as_str() == Some(benign_path)),
        "the benign memory must be found, otherwise the search is trivially empty: {raw}"
    );

    // Store-level belt: the raw secret was never persisted.
    let docs = state.qmd_memory.all_documents().await;
    assert!(
        !docs.iter().any(|d| d.content.contains(FAKE_API_KEY)),
        "the raw credential must never be stored"
    );
}

#[tokio::test]
async fn production_v1_memories_persists_plain_text() {
    let (app, state, _tmp) = production_fixture().await;
    let path = "security/ingest-plain";

    let (status, body) = send(
        &app,
        "/v1/memories",
        Some(ROOT_TOKEN),
        serde_json::json!({"content": "plain note without any credential", "path": path}),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"].as_str(), Some("ok"), "{body}");
    assert!(
        body["id"].as_str().is_some_and(|id| !id.is_empty()),
        "a successful write must return an id: {body}"
    );
    let docs = state.qmd_memory.all_documents().await;
    assert!(
        docs.iter()
            .any(|d| d.path == path && d.content == "plain note without any credential"),
        "plain content must be persisted"
    );
}

/// `v1_memories_add` is not part of the deployed HTTP router; covered here as
/// an isolated handler unit test only.
async fn handler_app(tmp: &TempDir) -> Router {
    let config = WorkspaceConfig {
        id: "ingest-handler".to_string(),
        token: "ingest-handler-token".to_string(),
        plan: PlanTier::Personal,
        memory_backend: MemoryBackend::Memory,
        storage_limit_bytes: None,
        request_limit: None,
        request_unit_limit: None,
        embedding_provider_mode: EmbeddingProviderMode::BringYourOwn,
        managed_google_embeddings: false,
        sync_policy: SyncPolicy::LocalOnly,
        dedup: DedupSettings::default(),
        protocol: Default::default(),
    };
    let workspace = WorkspaceState::new(config, RuntimeConfig::default(), tmp.path().to_path_buf())
        .await
        .expect("workspace state");
    let ctx = WorkspaceContext {
        workspace_id: "ingest-handler".to_string(),
        workspace: Arc::new(workspace),
    };
    Router::new()
        .route("/v1/memories", axum::routing::post(v1_memories_add))
        .layer(Extension(ctx))
}

#[tokio::test]
async fn v1_memories_add_handler_blocks_secret() {
    let tmp = TempDir::new().unwrap();
    let app = handler_app(&tmp).await;
    let (_, body) = send(
        &app,
        "/v1/memories",
        None,
        serde_json::json!({"text": format!("API_KEY={FAKE_API_KEY}"), "path": "security/ingest-handler"}),
    )
    .await;
    assert_eq!(
        body["status"].as_str(),
        Some("blocked"),
        "the handler must refuse a payload carrying a credential: {body}"
    );
}

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();
