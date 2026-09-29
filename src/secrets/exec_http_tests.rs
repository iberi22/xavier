use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::extract::Extension;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use axum::routing::post;
use axum::Router;
use tower::ServiceExt;

use crate::cli::handlers::secrets::{exec_handler, exec_secret, ExecSecretPayload};
use crate::cli::state::{CliState, CodeGraphState};
use crate::codebase::connection_manager::ConnectionManager;
use crate::coordination::KeyLendingEngine;
use crate::secrets::audit::QmdAuditLogger;
use crate::secrets::vault::HardwareVault;

const CANARY: &str = "canary-51c7ae30-exec-handler";
const CANARY_VAR: &str = "XAVIER_EXEC_HANDLER_CANARY";

struct Fixture {
    engine: Arc<KeyLendingEngine>,
    vault: Arc<HardwareVault>,
    secret_name: String,
    project_id: String,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn payload(&self) -> ExecSecretPayload {
        ExecSecretPayload {
            secret_name: self.secret_name.clone(),
            agent_id: self.secret_name.clone(),
            ttl_seconds: 30,
            env_var: CANARY_VAR.to_string(),
            command: "true".to_string(),
            args: Vec::new(),
        }
    }

    async fn exec_route(&self, body: serde_json::Value) -> (StatusCode, serde_json::Value) {
        let app = Router::new()
            .route("/secrets/exec", post(exec_handler))
            .layer(Extension(Arc::clone(&self.vault)))
            .with_state(test_state(Arc::clone(&self.engine)).await);
        let request = Request::builder()
            .method("POST")
            .uri("/secrets/exec")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).expect("json")))
            .expect("build request");
        let response = app.oneshot(request).await.expect("oneshot");
        let status = response.status();
        (status, body_of(response).await)
    }
}

async fn setup() -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let vault = Arc::new(HardwareVault::new("x").isolated(dir.path().join("secrets"), [9u8; 32]));
    let secret_name = format!("K_{}", uuid::Uuid::new_v4());
    vault
        .store_secret(&secret_name, CANARY)
        .expect("store canary");
    let project_id = format!("test_exec_http_secrets_{}", uuid::Uuid::new_v4());
    ConnectionManager::global()
        .connect_with_path(&project_id, dir.path().join("metrics.db"))
        .expect("connect isolated metrics db");
    let audit = Box::new(QmdAuditLogger::for_project(&project_id));
    audit.init_schema_async().await.expect("audit schema");
    let engine =
        Arc::new(KeyLendingEngine::new(audit, None).with_leases_project(project_id.clone()));
    Fixture {
        engine,
        vault,
        secret_name,
        project_id,
        _dir: dir,
    }
}

async fn test_state(engine: Arc<KeyLendingEngine>) -> CliState {
    use parking_lot::Mutex;
    use tokio::sync::RwLock as AsyncRwLock;
    use xavier::agents::provider::router::{ProviderKind, ProviderRouter};
    use xavier::agents::rate_limit::RateLimitManager;
    use xavier::app::proxy_use_case::ProxyUseCase;
    use xavier::app::qmd_memory_adapter::QmdMemoryAdapter;
    use xavier::app::security_service::SecurityService;
    use xavier::codebase::conversations_db::ConversationsDb;
    use xavier::coordination::{SimpleAgentRegistry, XavierEventBus};
    use xavier::embedding::NoopEmbedder;
    use xavier::memory::agent_indexer::AgentIndexer;
    use xavier::memory::file_indexer::{FileIndexer, FileIndexerConfig};
    use xavier::memory::openclaw_indexer::OpenClawAgentIndexer;
    use xavier::memory::qmd_memory::QmdMemory;
    use xavier::memory::sqlite_vec_store::{
        VecSqliteMemoryStore, VecSqliteStoreConfig, DEFAULT_EMBEDDING_DIMENSIONS,
    };
    use xavier::observability::UsageCounters;
    use xavier::security::sessions::SessionManager;
    use xavier::tasks::store::{InMemoryTaskStore, TaskService};

    let docs = Arc::new(AsyncRwLock::new(Vec::new()));
    let qmd_memory = Arc::new(QmdMemory::new_with_workspace(docs, "test-ws"));
    let memory_port = Arc::new(QmdMemoryAdapter::new(Arc::clone(&qmd_memory)));

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(
        VecSqliteMemoryStore::new(VecSqliteStoreConfig {
            path: temp_dir.path().join("vec_store.db"),
            embedding_dimensions: DEFAULT_EMBEDDING_DIMENSIONS,
        })
        .await
        .expect("vec store"),
    );

    let cg_db = Arc::new(::code_graph::db::CodeGraphDB::in_memory().expect("code graph db"));
    let cg_state = Arc::new(AsyncRwLock::new(CodeGraphState {
        db: cg_db.clone(),
        indexer: Arc::new(::code_graph::indexer::Indexer::new(cg_db.clone())),
        query: Arc::new(::code_graph::query::QueryEngine::new(cg_db)),
    }));

    CliState {
        memory: memory_port,
        qmd_memory,
        store,
        workspace_id: "test-ws".to_string(),
        workspace_dir: std::env::current_dir().expect("cwd"),
        state_dir: std::env::current_dir().expect("cwd"),
        auth_db: None,
        code_graph: cg_state,
        security: Arc::new(SecurityService::new()),
        security_scan: Arc::new(SecurityService::new()),
        _time_store: None,
        agent_registry: SimpleAgentRegistry::new(None),
        panel_store: Arc::new(
            ConversationsDb::open_in_memory("test-project")
                .await
                .expect("panel store"),
        ),
        secrets_engine: engine,
        event_bus: XavierEventBus::new(10),
        tasks: Arc::new(TaskService::new(Arc::new(InMemoryTaskStore::new()))),
        rate_manager: Arc::new(RateLimitManager::new()),
        prompt_cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
        http_client: reqwest::Client::new(),
        proxy_use_case: Arc::new(ProxyUseCase::new(
            Arc::new(RateLimitManager::new()),
            Arc::new(Mutex::new(std::collections::HashMap::new())),
        )),
        usage_counters: Arc::new(UsageCounters::new()),
        session_manager: Arc::new(SessionManager::new(60)),
        provider_router: Arc::new(AsyncRwLock::new(ProviderRouter::new(ProviderKind::OpenAI))),
        embedder: Arc::new(NoopEmbedder),
        agent_indexer: Arc::new(AgentIndexer::new(FileIndexer::new(
            FileIndexerConfig::default(),
            None,
        ))),
        auth_store: None,
        openclaw_indexer: Arc::new(OpenClawAgentIndexer::new(Arc::new(NoopEmbedder))),
        multi_db: xavier::storage::multi_db::MultiDbManager::new(),
        system_scan_cache: Arc::new(AsyncRwLock::new(None)),
        maloca: xavier::maloca::MalocaStore::open(&temp_dir.path().join("maloca")),
    }
}

async fn body_of(response: Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("json body")
}

async fn audit_rows(project_id: &str, agent_id: &str, event: &str) -> i64 {
    let project_id = project_id.to_string();
    let agent = agent_id.to_string();
    let kind = event.to_string();
    ConnectionManager::global()
        .with_conn(&project_id, move |conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM secret_audit_logs WHERE agent_id = ?1 AND event_type = ?2",
                rusqlite::params![agent, kind],
                |row| row.get(0),
            )
            .map_err(Into::into)
        })
        .await
        .expect("metrics audit query")
}

async fn wait_for_audit_pair(project_id: &str, agent_id: &str) -> (i64, i64) {
    let mut counts = (0, 0);
    for _ in 0..100 {
        counts = (
            audit_rows(project_id, agent_id, "LEND").await,
            audit_rows(project_id, agent_id, "REVOKE").await,
        );
        if counts == (1, 1) {
            return counts;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    counts
}

#[tokio::test]
async fn test_exec_handler_response_has_no_secret_value() {
    let f = setup().await;

    let response = exec_secret(&f.engine, f.payload(), &f.vault).await;
    let body = body_of(response).await;
    let rendered = serde_json::to_string(&body).expect("serialise body");

    assert!(
        !rendered.contains(CANARY),
        "exec response must NOT contain the canary value; got: {rendered}"
    );
    assert!(
        !rendered.contains("secret_value"),
        "exec response must NOT carry a secret_value key; got: {rendered}"
    );
    assert_eq!(body["exit_code"], serde_json::json!(0));
    assert_eq!(body["revoked"], serde_json::json!(true));
    assert!(body["lease_token"].is_null());
    assert!(body["duration_ms"].is_number());
    assert!(f.engine.list_leases().await.is_empty());
}

#[tokio::test]
async fn test_exec_handler_records_lend_and_revoke() {
    let f = setup().await;

    let _ = exec_secret(&f.engine, f.payload(), &f.vault).await;

    assert_eq!(
        wait_for_audit_pair(&f.project_id, &f.secret_name).await,
        (1, 1)
    );
    assert!(f.engine.list_leases().await.is_empty());
}

#[tokio::test]
async fn test_exec_route_via_axum_oneshot_with_isolated_vault() {
    let f = setup().await;
    let (status, body) = f
        .exec_route(serde_json::json!({
            "secret_name": f.secret_name,
            "agent_id": f.secret_name,
            "command": "true",
            "args": [],
        }))
        .await;
    assert_eq!(status, StatusCode::OK);

    let rendered = serde_json::to_string(&body).expect("serialise body");
    assert!(
        !rendered.contains(CANARY),
        "exec route response must NOT contain the canary value; got: {rendered}"
    );
    assert_eq!(body["exit_code"], serde_json::json!(0));
    assert_eq!(body["revoked"], serde_json::json!(true));
    assert!(body["lease_token"].is_null());
    assert!(f.engine.list_leases().await.is_empty());
}

#[tokio::test]
async fn test_exec_route_rejects_missing_required_fields() {
    let f = setup().await;
    let (status, body) = f
        .exec_route(serde_json::json!({
            "command": "true",
            "args": [],
        }))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let error = body["error"].as_str().expect("error message");
    assert!(
        error.contains("secret_name"),
        "error must name the missing field; got: {error}"
    );
    assert!(f.engine.list_leases().await.is_empty());
}
