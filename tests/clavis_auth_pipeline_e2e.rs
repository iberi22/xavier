//! Integration test verifying Clavis HTTP endpoint authorization passing through
//! the real production auth_middleware layer rather than manually injecting mock Claims.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::middleware::from_fn_with_state;
use axum::Router;
use http_body_util::BodyExt;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;

use xavier::agents::provider::router::{ProviderKind, ProviderRouter};
use xavier::agents::rate_limit::RateLimitManager;
use xavier::app::proxy_use_case::ProxyUseCase;
use xavier::app::qmd_memory_adapter::QmdMemoryAdapter;
use xavier::cli::http_setup::auth_middleware;
use xavier::cli::server::clavis_routes;
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
use xavier::secrets::vault::HardwareVault;
use xavier::security::auth::{generate_jwt, User, UserRole};
use xavier::tasks::store::{InMemoryTaskStore, TaskService};

const ROOT_TOKEN: &str = "clavis-pipeline-root-token";
const JWT_SECRET: &str = "clavis-pipeline-jwt-secret-0123456789";

fn setup_isolated_env() -> TempDir {
    let tmp_dir = tempfile::tempdir().expect("create tempdir");
    let isolated_dir = tmp_dir.path().to_path_buf();
    std::env::set_var("HOME", &isolated_dir);
    std::env::set_var("XAVIER_DATA_DIR", &isolated_dir);
    std::env::set_var("XAVIER_TOKEN", ROOT_TOKEN);
    std::env::set_var("XAVIER_JWT_SECRET", JWT_SECRET);
    tmp_dir
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

async fn build_app(state: CliState) -> Router {
    clavis_routes()
        .layer(from_fn_with_state(state.clone(), auth_middleware))
        .with_state(state)
}

fn build_request(
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> Request<Body> {
    let mut builder = Request::builder().uri(uri).method(method);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    if let Some(t) = token {
        builder = builder.header("X-Xavier-Token", t);
    }
    builder
        .body(match body {
            Some(b) => Body::from(b.to_string()),
            None => Body::empty(),
        })
        .expect("build request")
}

#[tokio::test]
#[serial_test::serial]
async fn test_clavis_auth_pipeline_unauthenticated_and_invalid_tokens() {
    let tmp = setup_isolated_env();
    let state = create_test_cli_state(&tmp).await;
    let app = build_app(state).await;

    // 1. Missing Token -> 401 Unauthorized
    let req_missing = build_request(Method::GET, "/v1/clavis/keys/api_key_test", None, None);
    let resp = app.clone().oneshot(req_missing).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 2. Malformed / Wrong Token -> 401 Unauthorized
    let req_invalid = build_request(
        Method::GET,
        "/v1/clavis/keys/api_key_test",
        Some("invalid-token-123"),
        None,
    );
    let resp = app.clone().oneshot(req_invalid).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[serial_test::serial]
async fn test_clavis_auth_pipeline_forbidden_for_non_root_tokens() {
    let tmp = setup_isolated_env();
    let state = create_test_cli_state(&tmp).await;
    let app = build_app(state).await;

    // Valid JWT Token with Admin or User role (not Root Token)
    let admin_user = User::new(
        "admin@swal.dev".to_string(),
        "Admin User".to_string(),
        UserRole::Admin,
    );
    let jwt_token = generate_jwt(&admin_user, JWT_SECRET.as_bytes()).unwrap();

    let req = build_request(
        Method::GET,
        "/v1/clavis/keys/api_key_test",
        Some(&jwt_token),
        None,
    );
    let resp = app.clone().oneshot(req).await.unwrap();

    // Since Clavis is in ROOT_ONLY_PREFIXES, non-root tokens must get 403 Forbidden
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
#[serial_test::serial]
async fn test_clavis_auth_pipeline_root_token_authorized_round_trip() {
    let tmp = setup_isolated_env();
    let state = create_test_cli_state(&tmp).await;
    let app = build_app(state).await;

    let secret_key = "api_key_pipeline_test";
    let secret_val = "sk-proj-pipeline-secret-val-12345";
    let _ = HardwareVault::new("xavier-clavis").delete_secret(secret_key);

    // 1. PUT key using Root Token -> 201 Created
    let put_req = build_request(
        Method::PUT,
        &format!("/v1/clavis/keys/{}", secret_key),
        Some(ROOT_TOKEN),
        Some(&format!(r#"{{"value":"{}"}}"#, secret_val)),
    );
    let put_resp = app.clone().oneshot(put_req).await.unwrap();
    assert_eq!(put_resp.status(), StatusCode::CREATED);

    let put_body = put_resp.into_body().collect().await.unwrap().to_bytes();
    let put_json: serde_json::Value = serde_json::from_slice(&put_body).unwrap();
    assert_eq!(put_json["status"], "ok");
    assert_eq!(put_json["created"], true);

    // 2. GET key using Root Token -> 200 OK
    let get_req = build_request(
        Method::GET,
        &format!("/v1/clavis/keys/{}", secret_key),
        Some(ROOT_TOKEN),
        None,
    );
    let get_resp = app.clone().oneshot(get_req).await.unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);

    let get_body = get_resp.into_body().collect().await.unwrap().to_bytes();
    let get_json: serde_json::Value = serde_json::from_slice(&get_body).unwrap();
    assert_eq!(get_json["value"], secret_val);

    let _ = HardwareVault::new("xavier-clavis").delete_secret(secret_key);
}

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();
