xavier::isolate_test_process!();

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
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
use xavier::memory::file_indexer::{FileIndexer, FileIndexerConfig};
use xavier::memory::qmd_memory::QmdMemory;
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use xavier::ports::inbound::AgentLifecyclePort;
use xavier::secrets::audit::QmdAuditLogger;
use xavier::tasks::store::{InMemoryTaskStore, TaskService};
use xavier::workspace::{WorkspaceContext, WorkspaceState};

/// Minimal `CliState` fixture, matching `tests/production_router_smoke.rs` and `tests/clavis_auth_pipeline_e2e.rs`.
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

/// Puts env vars back when the test ends, including on assertion failure.
struct RestoreCwd(std::path::PathBuf);

impl Drop for RestoreCwd {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).unwrap();
    }
}

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

use serde_json::{json, Value};
use xavier::adapters::inbound::http::handlers::{ivn, marketplace};
use xavier::data_commons::marketplace::DataMarketplace;
use xavier::security::auth::{generate_jwt, User, UserRole};

const JWT_SECRET: &str = "ivn-router-test-signing-secret";

fn token(subject: &str, role: UserRole) -> String {
    let mut user = User::new(format!("{subject}@example.invalid"), subject.into(), role);
    user.id = subject.into();
    generate_jwt(&user, JWT_SECRET.as_bytes()).unwrap()
}

async fn app(temp: &TempDir) -> Router {
    let state = create_test_cli_state(temp).await;
    let workspace = Arc::new(
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
            workspace,
        },
        space_manager: None,
        maloca_router: Router::new(),
        event_bus_for_ws: xavier::coordination::XavierEventBus::new(10),
        no_ui: true,
    };
    build_app(state, deps)
}

async fn call(
    app: &Router,
    method: Method,
    path: &str,
    bearer: &str,
    body: Value,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("Authorization", format!("Bearer {bearer}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|e| panic!("Expected JSON for {path}, got {status}: {e}"));
    (status, value)
}

async fn forbidden(app: &Router, method: Method, path: &str, bearer: &str, body: Value) {
    let (status, response) = call(app, method, path, bearer, body).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {response}");
    assert_eq!(response["status"], "error", "{response}");
    assert!(response["message"].as_str().is_some(), "{response}");
}

fn dataset(publisher: &str, name: &str) -> Value {
    json!({"name": name, "description": "Router authorization fixture", "category": "test",
        "publisher": publisher, "rows": [{"value": 7}]})
}

fn identity_request() -> Value {
    let candidates: Vec<Value> = (1..=6)
        .map(|n| {
            json!({
                "node_id": format!("validator-{n}"), "wallet": format!("wallet-{n}"),
                "karma": 1000, "seed": format!("seed-{n}"),
                "operator_id": format!("operator-{n}"), "ip_address": format!("192.168.{n}.1")
            })
        })
        .collect();
    json!({"applicant": "applicant", "proof_hashes": ["proof"], "candidate_pool": candidates})
}

#[tokio::test]
async fn productive_router_enforces_ivn_marketplace_roles_and_preserves_state() {
    let temp = tempfile::tempdir().unwrap();
    let _cwd = RestoreCwd(std::env::current_dir().unwrap());
    std::env::set_current_dir(temp.path()).unwrap();
    let _env = RestoreEnv::set(&[("HOME", temp.path()), ("XAVIER_DATA_DIR", temp.path())]);
    let _auth = RestoreEnv::set_str(&[
        ("XAVIER_TOKEN", "ivn-router-root-test"),
        ("XAVIER_JWT_SECRET", JWT_SECRET),
    ]);
    ivn::init_ivn_engine(ivn::IvnEngineStore::load().unwrap());
    marketplace::init_marketplace(DataMarketplace::load().unwrap());
    let router = app(&temp).await;
    let viewer = token("viewer", UserRole::Readonly);
    let owner = token("owner", UserRole::User);
    let stranger = token("stranger", UserRole::User);
    let admin = token("admin", UserRole::Admin);

    forbidden(
        &router,
        Method::POST,
        "/v1/marketplace/datasets",
        &viewer,
        dataset("viewer", "Denied"),
    )
    .await;
    forbidden(
        &router,
        Method::POST,
        "/v1/identity/request",
        &viewer,
        identity_request(),
    )
    .await;
    forbidden(
        &router,
        Method::POST,
        "/v1/identity/missing/vote",
        &viewer,
        json!({"validator_node_id": "viewer", "vote": "Abstain"}),
    )
    .await;
    forbidden(
        &router,
        Method::POST,
        "/v1/marketplace/datasets",
        &stranger,
        dataset("owner", "Spoofed"),
    )
    .await;

    let (status, created) = call(
        &router,
        Method::POST,
        "/v1/marketplace/datasets",
        &owner,
        dataset("owner", "Owned"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let dataset_id = created["dataset_id"].as_str().unwrap();
    let revoke = format!("/v1/marketplace/datasets/{dataset_id}");
    forbidden(&router, Method::DELETE, &revoke, &viewer, Value::Null).await;
    forbidden(&router, Method::DELETE, &revoke, &stranger, Value::Null).await;

    let (status, created) = call(
        &router,
        Method::POST,
        "/v1/identity/request",
        &owner,
        identity_request(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let request = &created["request"];
    let id = request["id"].as_str().unwrap();
    let assigned = request["assigned_validators"].as_array().unwrap();
    let first = assigned[0]["node_id"].as_str().unwrap();
    let second = assigned[1]["node_id"].as_str().unwrap();
    let wallet = assigned[0]["wallet"].as_str().unwrap();
    let validator = token(first, UserRole::User);
    let vote_path = format!("/v1/identity/{id}/vote");
    forbidden(
        &router,
        Method::POST,
        &vote_path,
        &stranger,
        json!({"validator_node_id": first, "vote": "Abstain"}),
    )
    .await;
    forbidden(
        &router,
        Method::POST,
        &vote_path,
        &validator,
        json!({"validator_node_id": second, "vote": "Abstain"}),
    )
    .await;
    let vote = json!({"validator_node_id": first, "vote": "Abstain"});
    let (status, response) =
        call(&router, Method::POST, &vote_path, &validator, vote.clone()).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    forbidden(&router, Method::POST, &vote_path, &validator, vote.clone()).await;
    forbidden(
        &router,
        Method::POST,
        &vote_path,
        &validator,
        json!({"validator_node_id": wallet, "vote": "Check"}),
    )
    .await;

    // Discard process-global stores, reload from disk, then build a fresh productive router.
    drop(router);
    ivn::init_ivn_engine(ivn::IvnEngineStore::default());
    marketplace::init_marketplace(DataMarketplace::new());
    ivn::init_ivn_engine(ivn::IvnEngineStore::load().unwrap());
    marketplace::init_marketplace(DataMarketplace::load().unwrap());
    let router = app(&temp).await;
    let (status, response) = call(
        &router,
        Method::GET,
        &format!("/v1/identity/request/{id}"),
        &viewer,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["request"]["votes"].as_object().unwrap().len(), 1);
    forbidden(&router, Method::POST, &vote_path, &validator, vote).await;
    let (status, response) = call(
        &router,
        Method::GET,
        "/v1/marketplace/datasets",
        &viewer,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert!(response["datasets"]
        .as_array()
        .unwrap()
        .iter()
        .any(|dataset| dataset["id"] == dataset_id));
    let (status, response) = call(&router, Method::DELETE, &revoke, &owner, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{response}");

    let (status, created) = call(
        &router,
        Method::POST,
        "/v1/marketplace/datasets",
        &admin,
        dataset("owner", "Admin managed"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let path = format!(
        "/v1/marketplace/datasets/{}",
        created["dataset_id"].as_str().unwrap()
    );
    let (status, response) = call(&router, Method::DELETE, &path, &admin, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    drop(router);
    marketplace::init_marketplace(DataMarketplace::load().unwrap());
    let router = app(&temp).await;
    let (_, response) = call(
        &router,
        Method::GET,
        "/v1/marketplace/datasets",
        &viewer,
        Value::Null,
    )
    .await;
    assert_eq!(
        response["count"], 0,
        "Revocations must survive reload: {response}"
    );
}
