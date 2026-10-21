//! Secret handlers for key lending and lease management.

use std::sync::Arc;

use axum::{
    extract::{rejection::JsonRejection, Extension, Path as AxumPath, State},
    http::StatusCode,
    response::Response,
    Json,
};

use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use crate::cli::types::*;
use xavier::coordination::secrets::lease_id_for_token;
use xavier::coordination::KeyLendingEngine;
use xavier::secrets::exec::{run_with_secret, ExecSecretError, SecretExecSpec};
use xavier::secrets::vault::HardwareVault;

/// Service name of the vault `POST /secrets/exec` resolves secrets from.
const EXEC_VAULT_SERVICE: &str = "xavier";

/// Environment variable the child is given when the request names none.
const EXEC_ENV_VAR_DEFAULT: &str = "XAVIER_SECRET";

/// Lease TTL and child watchdog applied when the request names none. Matches
/// the `secrets exec` CLI default.
const EXEC_TTL_DEFAULT: u64 = 3600;

/// Request body of `POST /secrets/exec`.
///
/// Carries a secret *name* only: there is no field through which a value can
/// enter the node, and `args` is an array that is forwarded verbatim, so the
/// command is never reassembled as a shell string. `ttl_seconds` is optional
/// and defaults to [`EXEC_TTL_DEFAULT`], the same value the CLI uses.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ExecSecretPayload {
    /// Vault entry whose value is injected into the child.
    pub secret_name: String,
    /// Agent identity recorded in the lease and in the audit rows.
    pub agent_id: String,
    /// Lease TTL and child watchdog, in seconds (default 3600).
    #[serde(default = "default_exec_ttl")]
    pub ttl_seconds: u64,
    /// Environment variable the child receives the value in.
    #[serde(default = "default_exec_env_var")]
    pub env_var: String,
    /// Program to run.
    pub command: String,
    /// Argument vector, forwarded as an array.
    #[serde(default)]
    pub args: Vec<String>,
}

/// Response body of `POST /secrets/exec`: the outcome of a child process whose
/// secret was injected by the engine. It has no field able to hold a value.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ExecSecretResponse {
    /// Exit status of the child, or `null` when it was killed by a signal.
    pub exit_code: Option<i32>,
    /// Always `null`: the lease is revoked inside the delegation and its token
    /// is never handed back, so no token can be replayed from this response.
    pub lease_token: Option<String>,
    /// Wall-clock duration of the child run, in milliseconds.
    pub duration_ms: u64,
    /// Always `true` here: the engine revokes the lease before returning.
    pub revoked: bool,
}

fn default_exec_env_var() -> String {
    EXEC_ENV_VAR_DEFAULT.to_string()
}

fn default_exec_ttl() -> u64 {
    EXEC_TTL_DEFAULT
}

/// `POST /secrets/exec` handler.
///
/// Delegates to [`run_with_secret`], which resolves the value from the vault,
/// lends it under the TTL watchdog, injects it into exactly one environment
/// variable of the child and revokes the lease on every exit path. The secret
/// is never read here, so no response field, error string or log line built
/// from this request can carry a value.
///
/// The vault is injected through an [`axum::Extension`] when the router
/// carries one (tests wire an isolated store); production routers do not, so
/// the real `xavier` vault is used.
pub async fn exec_handler(
    State(state): State<CliState>,
    maybe_vault: Option<Extension<Arc<HardwareVault>>>,
    result: Result<Json<ExecSecretPayload>, JsonRejection>,
) -> Response {
    let engine = Arc::clone(&state.secrets_engine);
    let vault = match maybe_vault {
        Some(Extension(vault)) => vault,
        None => Arc::new(HardwareVault::new(EXEC_VAULT_SERVICE)),
    };
    let payload = match result {
        Ok(Json(payload)) => payload,
        // A missing or malformed field must be a 4xx with a clear message,
        // never a panic: name the field the request got wrong.
        Err(rejection) => {
            return json_response(
                StatusCode::BAD_REQUEST,
                serde_json::json!({ "error": rejection.body_text() }),
            );
        }
    };
    exec_secret(&engine, payload, &vault).await
}

/// The body of [`exec_handler`], with the vault as an explicit parameter so
/// tests inject an isolated store instead of the real keyring or `~/.xavier`.
pub(crate) async fn exec_secret(
    engine: &Arc<KeyLendingEngine>,
    payload: ExecSecretPayload,
    vault: &HardwareVault,
) -> Response {
    let spec = SecretExecSpec::new(
        payload.secret_name.clone(),
        payload.env_var,
        payload.command,
        payload.args,
        payload.ttl_seconds,
        payload.agent_id,
    );

    match run_with_secret(&spec, Arc::clone(engine), vault).await {
        Ok(outcome) => json_response(
            StatusCode::OK,
            serde_json::to_value(ExecSecretResponse {
                exit_code: outcome.exit_code,
                lease_token: None,
                duration_ms: outcome.duration_ms,
                revoked: true,
            })
            .unwrap_or_default(),
        ),
        Err(e) => {
            let reason = exec_failure_reason(&e);
            tracing::warn!(
                secret_name = %payload.secret_name,
                reason = %reason,
                "secret exec refused"
            );
            json_response(
                exec_failure_status(&e),
                serde_json::json!({
                    "error": reason,
                    "secret_name": payload.secret_name,
                }),
            )
        }
    }
}

/// Failure reasons are fixed strings: the raw engine error can carry a
/// filesystem path, and none of these can ever carry a secret value.
fn exec_failure_reason(error: &ExecSecretError) -> &'static str {
    match error {
        ExecSecretError::Vault(_) => "secret is not available in the vault",
        ExecSecretError::Lease(_) => "lease could not be created",
        ExecSecretError::Io(_) => "child process could not be run",
        ExecSecretError::Timeout(_) => "execution timed out",
    }
}

fn exec_failure_status(error: &ExecSecretError) -> StatusCode {
    match error {
        ExecSecretError::Vault(_) => StatusCode::NOT_FOUND,
        ExecSecretError::Timeout(_) => StatusCode::GATEWAY_TIMEOUT,
        ExecSecretError::Lease(_) | ExecSecretError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// Lend handler.
pub async fn lend_handler(
    State(state): State<CliState>,
    axum::Extension(_session): axum::Extension<crate::cli::http_setup::SessionInfo>,
    Json(payload): Json<LendSecretPayload>,
) -> Response {
    let result = if payload.secret_value.as_deref().unwrap_or("").is_empty() {
        state
            .secrets_engine
            .lend_from_vault(
                &payload.secret_name,
                &payload.agent_id,
                payload.ttl_seconds,
                false, // Internal lend, we redact in the handler
            )
            .await
    } else {
        state
            .secrets_engine
            .lend(
                &payload.secret_name,
                payload.secret_value.as_deref(),
                &payload.agent_id,
                payload.ttl_seconds,
            )
            .await
    };

    match result {
        Ok(mut lease) => {
            // F2 - Redact secret_value from response (key never leaves Xavier)
            lease.secret_value = None;
            // The token is returned once, as the agent's bearer credential. The
            // opaque lease id is what the caller uses for status and revoke.
            let mut body = serde_json::to_value(&lease).unwrap_or_default();
            body["lease_id"] = serde_json::Value::String(lease_id_for_token(&lease.token));
            json_response(StatusCode::OK, body)
        }
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

/// Project id override for `history_handler` in tests or custom deployments.
#[derive(Clone, Debug)]
pub struct HistoryProject(pub String);

/// History handler.
pub async fn history_handler(maybe_project: Option<Extension<HistoryProject>>) -> Response {
    let project_id = match &maybe_project {
        Some(Extension(HistoryProject(p))) => p.as_str(),
        None => "metrics",
    };
    history_response(project_id).await
}

/// Helper that builds the history response for a given project id.
pub(crate) async fn history_response(project_id: &str) -> Response {
    match history_rows(project_id).await {
        Ok(logs) => json_response(
            StatusCode::OK,
            serde_json::to_value(logs).unwrap_or_default(),
        ),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

/// Newest audit rows of `project_id`. Each row names its lease by the same
/// opaque lease id the list uses; the raw session token column never leaves
/// the node.
pub(crate) async fn history_rows(project_id: &str) -> anyhow::Result<Vec<serde_json::Value>> {
    use xavier::codebase::connection_manager::ConnectionManager;
    ConnectionManager::global()
        .with_conn(project_id, |conn: &rusqlite::Connection| {
            let mut stmt = conn.prepare(
                "SELECT id, timestamp, event_type, agent_id, session_token, secret_id, reason
                 FROM secret_audit_logs
                 ORDER BY timestamp DESC
                 LIMIT 100",
            )?;
            let rows = stmt.query_map([], |row: &rusqlite::Row| {
                let session_token: String = row.get(4)?;
                Ok(serde_json::json!({
                    "id": row.get::<_, i64>(0)?,
                    "timestamp": row.get::<_, String>(1)?,
                    "event_type": row.get::<_, String>(2)?,
                    "agent_id": row.get::<_, String>(3)?,
                    "lease_id": lease_id_for_token(&session_token),
                    "secret_id": row.get::<_, Option<String>>(5)?,
                    "reason": row.get::<_, Option<String>>(6)?,
                }))
            })?;

            let mut logs = Vec::new();
            for row in rows {
                logs.push(row?);
            }
            Ok::<Vec<serde_json::Value>, anyhow::Error>(logs)
        })
        .await
}

/// Leases handler. Lists token-free views: opaque lease ids, never tokens.
pub async fn leases_handler(State(state): State<CliState>) -> Response {
    let leases = state.secrets_engine.list_lease_views().await;
    json_response(
        StatusCode::OK,
        serde_json::to_value(leases).unwrap_or_default(),
    )
}

/// Revoke handler. Targets a lease by its opaque lease id.
pub async fn revoke_handler(
    State(state): State<CliState>,
    Json(payload): Json<RevokeLeasePayload>,
) -> Response {
    revoke_by_id_response(&state, &payload.lease_id, "Manual API Call").await
}

/// Shared by `POST /secrets/revoke` (id in the body) and the proxy's
/// `POST /secrets/revoke/{lease_id}` (id in the path). Errors never echo the id.
pub(crate) async fn revoke_by_id_response(
    state: &CliState,
    lease_id: &str,
    reason: &str,
) -> Response {
    match state
        .secrets_engine
        .revoke_by_lease_id(lease_id, reason)
        .await
    {
        Ok(_) => json_response(StatusCode::OK, serde_json::json!({ "status": "revoked" })),
        Err(e) => json_response(
            StatusCode::NOT_FOUND,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

/// Status handler. Returns the token-free view of the lease with this id.
pub async fn status_handler(
    State(state): State<CliState>,
    AxumPath(lease_id): AxumPath<String>,
) -> Response {
    match state.secrets_engine.get_lease_view(&lease_id).await {
        Some(status) => json_response(
            StatusCode::OK,
            serde_json::to_value(status).unwrap_or_default(),
        ),
        None => json_response(
            StatusCode::NOT_FOUND,
            serde_json::json!({ "error": "Lease not found" }),
        ),
    }
}

/// Admin only. Lending a node secret to a process is an owner decision, so the
/// only principal that passes is the one `auth_middleware` gives the root token.
pub fn secrets_permission_check(role: &xavier::security::auth::UserRole) -> bool {
    use xavier::security::auth::Permission;
    role.can_manage_secrets()
}

/// The seven `/secrets/*` routes, gated as a unit.
///
/// Built here and merged by `src/cli/server.rs` so the tests run the wiring the
/// daemon serves instead of a copy of it — the gap that let `/code/index` ship
/// without a gate (#2793, #2800).
///
/// The gate is a `route_layer` over the whole sub-router rather than a
/// per-route layer, so a route added here is gated by construction: forgetting
/// the check is not expressible. `dead_code` is denied so the server cannot
/// stop merging this without failing the build.
#[deny(dead_code)]
pub(crate) fn secrets_routes() -> axum::Router<CliState> {
    use axum::middleware;
    use axum::routing::{get, post};
    use xavier::middleware::require_permission;

    axum::Router::new()
        .route("/secrets/lend", post(lend_handler))
        .route("/secrets/exec", post(exec_handler))
        .route("/secrets/leases", get(leases_handler))
        .route("/secrets/revoke", post(revoke_handler))
        .route("/secrets/history", get(history_handler))
        .route(
            "/secrets/revoke/{lease_id}",
            post(crate::cli::proxy::revoke_lease_by_path),
        )
        .route("/secrets/status/{lease_id}", get(status_handler))
        .route_layer(middleware::from_fn(require_permission(
            secrets_permission_check,
        )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use axum::routing::get;
    use axum::Router;
    use std::collections::BTreeSet;
    use tower::ServiceExt;

    use crate::cli::state::{CliState, CodeGraphState};
    use crate::codebase::connection_manager::ConnectionManager;
    use crate::secrets::audit::QmdAuditLogger;

    const TEST_CANARY_SECRET: &str = "canary-secret-val-98234-xav-w2b";

    struct TestFixture {
        engine: Arc<KeyLendingEngine>,
        vault: Arc<HardwareVault>,
        project_id: String,
        _dir: tempfile::TempDir,
    }

    async fn setup_fixture() -> TestFixture {
        let dir = tempfile::tempdir().expect("tempdir");
        let vault =
            Arc::new(HardwareVault::new("x").isolated(dir.path().join("secrets"), [9u8; 32]));
        let project_id = format!("test_secrets_handler_{}", uuid::Uuid::new_v4());
        ConnectionManager::global()
            .connect_with_path(&project_id, dir.path().join("metrics.db"))
            .expect("connect isolated metrics db");
        let audit = Box::new(QmdAuditLogger::for_project(&project_id));
        audit.init_schema_async().await.expect("audit schema");
        let engine =
            Arc::new(KeyLendingEngine::new(audit, None).with_leases_project(project_id.clone()));
        TestFixture {
            engine,
            vault,
            project_id,
            _dir: dir,
        }
    }

    async fn build_test_state(engine: Arc<KeyLendingEngine>, dir: &tempfile::TempDir) -> CliState {
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

        let store = Arc::new(
            VecSqliteMemoryStore::new(VecSqliteStoreConfig {
                path: dir.path().join("vec_store.db"),
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
            workspace_dir: dir.path().to_path_buf(),
            state_dir: dir.path().to_path_buf(),
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
            maloca: xavier::maloca::MalocaStore::open(&dir.path().join("maloca")),
        }
    }

    async fn response_json(response: Response) -> serde_json::Value {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        serde_json::from_slice(&bytes).expect("json body")
    }

    #[tokio::test]
    async fn test_get_secrets_leases_never_leaks_token_or_secret_value() {
        let f = setup_fixture().await;
        let state = build_test_state(Arc::clone(&f.engine), &f._dir).await;

        let secret_name = format!("K_{}", uuid::Uuid::new_v4());
        let agent_id = format!("agent_{}", uuid::Uuid::new_v4());

        f.vault
            .store_secret(&secret_name, TEST_CANARY_SECRET)
            .expect("store secret in isolated vault");
        let lease = f
            .engine
            .lend(&secret_name, Some(TEST_CANARY_SECRET), &agent_id, 3600)
            .await
            .expect("lend secret");
        let known_token = lease.token.clone();
        assert!(!known_token.is_empty(), "lease must have a token");

        // 1. Test via router GET /secrets/leases
        let app = Router::new()
            .route("/secrets/leases", get(leases_handler))
            .with_state(state.clone());

        let req = Request::builder()
            .method("GET")
            .uri("/secrets/leases")
            .body(Body::empty())
            .expect("build request");
        let res = app.oneshot(req).await.expect("oneshot request");
        assert_eq!(res.status(), StatusCode::OK);

        let body = response_json(res).await;
        let body_str = serde_json::to_string(&body).expect("serialize body");

        assert!(
            !body_str.contains(TEST_CANARY_SECRET),
            "leases response must NOT contain the canary secret value; got: {body_str}"
        );
        assert!(
            !body_str.contains(&known_token),
            "leases response must NOT contain the lease token; got: {body_str}"
        );

        let leases = body.as_array().expect("expected json array of leases");
        assert!(!leases.is_empty(), "expected at least one lease");
        let found = leases
            .iter()
            .find(|v| v.get("secret_name").and_then(|s| s.as_str()) == Some(&secret_name))
            .expect("created lease must be present");

        let obj = found.as_object().expect("lease must be a json object");
        let actual_keys: BTreeSet<&str> = obj.keys().map(String::as_str).collect();
        let expected_keys: BTreeSet<&str> = [
            "agent_id",
            "created_at",
            "expires_at",
            "lease_id",
            "secret_name",
        ]
        .into_iter()
        .collect();
        assert_eq!(
            actual_keys, expected_keys,
            "lease view keys must match exactly"
        );
        assert!(!obj.contains_key("token"), "must not serialize 'token'");
        assert!(
            !obj.contains_key("secret_value"),
            "must not serialize 'secret_value'"
        );
        assert!(!obj.contains_key("value"), "must not serialize 'value'");
        assert!(
            !obj.contains_key("session_token"),
            "must not serialize 'session_token'"
        );

        let expected_lease_id = lease_id_for_token(&known_token);
        assert_eq!(
            found.get("lease_id").and_then(|v| v.as_str()),
            Some(expected_lease_id.as_str())
        );

        // 2. Direct handler test: verify handler produces safe output
        let direct_res = leases_handler(State(state)).await;
        assert_eq!(direct_res.status(), StatusCode::OK);
        let direct_body = response_json(direct_res).await;
        let direct_body_str = serde_json::to_string(&direct_body).expect("serialize body");
        assert!(!direct_body_str.contains(TEST_CANARY_SECRET));
        assert!(!direct_body_str.contains(&known_token));
    }

    #[tokio::test]
    async fn test_get_secrets_history_never_leaks_token_or_secret_value() {
        let f = setup_fixture().await;
        let state = build_test_state(Arc::clone(&f.engine), &f._dir).await;

        let secret_name = format!("K_{}", uuid::Uuid::new_v4());
        let agent_id = format!("agent_{}", uuid::Uuid::new_v4());

        f.vault
            .store_secret(&secret_name, TEST_CANARY_SECRET)
            .expect("store secret in isolated vault");
        let lease = f
            .engine
            .lend(&secret_name, Some(TEST_CANARY_SECRET), &agent_id, 3600)
            .await
            .expect("lend secret");
        let known_token = lease.token.clone();
        assert!(!known_token.is_empty(), "lease must have a token");

        // Wait for async audit logger to commit row to sqlite
        let mut row_committed = false;
        for _ in 0..100 {
            let rows = history_rows(&f.project_id).await.unwrap_or_default();
            if rows
                .iter()
                .any(|r| r.get("secret_id").and_then(|s| s.as_str()) == Some(&secret_name))
            {
                row_committed = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(row_committed, "audit log was not committed in time");

        // 1. Test via router GET /secrets/history with isolated project extension
        let app = Router::new()
            .route("/secrets/history", get(history_handler))
            .layer(Extension(HistoryProject(f.project_id.clone())))
            .with_state(state.clone());

        let req = Request::builder()
            .method("GET")
            .uri("/secrets/history")
            .body(Body::empty())
            .expect("build request");
        let res = app.oneshot(req).await.expect("oneshot request");
        assert_eq!(res.status(), StatusCode::OK);

        let body = response_json(res).await;
        let body_str = serde_json::to_string(&body).expect("serialize body");

        assert!(
            !body_str.contains(TEST_CANARY_SECRET),
            "history response must NOT contain the canary secret value; got: {body_str}"
        );
        assert!(
            !body_str.contains(&known_token),
            "history response must NOT contain the lease token; got: {body_str}"
        );

        let history = body
            .as_array()
            .expect("expected json array of history logs");
        assert!(!history.is_empty(), "expected at least one history row");
        let found = history
            .iter()
            .find(|v| v.get("secret_id").and_then(|s| s.as_str()) == Some(&secret_name))
            .expect("audit row for created lease must be present");

        let obj = found
            .as_object()
            .expect("history row must be a json object");
        let actual_keys: BTreeSet<&str> = obj.keys().map(String::as_str).collect();
        let expected_keys: BTreeSet<&str> = [
            "agent_id",
            "event_type",
            "id",
            "lease_id",
            "reason",
            "secret_id",
            "timestamp",
        ]
        .into_iter()
        .collect();
        assert_eq!(
            actual_keys, expected_keys,
            "history row keys must match exactly"
        );
        assert!(!obj.contains_key("token"), "must not serialize 'token'");
        assert!(
            !obj.contains_key("session_token"),
            "must not serialize 'session_token'"
        );
        assert!(
            !obj.contains_key("secret_value"),
            "must not serialize 'secret_value'"
        );
        assert!(!obj.contains_key("value"), "must not serialize 'value'");

        let expected_lease_id = lease_id_for_token(&known_token);
        assert_eq!(
            found.get("lease_id").and_then(|v| v.as_str()),
            Some(expected_lease_id.as_str())
        );

        // 2. Direct handler test with HistoryProject extension
        let direct_res =
            history_handler(Some(Extension(HistoryProject(f.project_id.clone())))).await;
        assert_eq!(direct_res.status(), StatusCode::OK);
        let direct_body = response_json(direct_res).await;
        let direct_body_str = serde_json::to_string(&direct_body).expect("serialize body");
        assert!(!direct_body_str.contains(TEST_CANARY_SECRET));
        assert!(!direct_body_str.contains(&known_token));
    }

    #[tokio::test]
    async fn test_get_secrets_status_never_leaks_token_or_secret_value() {
        let f = setup_fixture().await;
        let state = build_test_state(Arc::clone(&f.engine), &f._dir).await;

        let secret_name = format!("K_{}", uuid::Uuid::new_v4());
        let agent_id = format!("agent_{}", uuid::Uuid::new_v4());

        let lease = f
            .engine
            .lend(&secret_name, Some(TEST_CANARY_SECRET), &agent_id, 3600)
            .await
            .expect("lend secret");
        let known_token = lease.token.clone();
        let opaque_lease_id = lease_id_for_token(&known_token);

        let app = Router::new()
            .route("/secrets/status/{lease_id}", get(status_handler))
            .with_state(state.clone());

        let req = Request::builder()
            .method("GET")
            .uri(format!("/secrets/status/{opaque_lease_id}"))
            .body(Body::empty())
            .expect("build request");
        let res = app.oneshot(req).await.expect("oneshot request");
        assert_eq!(res.status(), StatusCode::OK);

        let body = response_json(res).await;
        let body_str = serde_json::to_string(&body).expect("serialize body");

        assert!(!body_str.contains(TEST_CANARY_SECRET));
        assert!(!body_str.contains(&known_token));

        let obj = body.as_object().expect("object");
        let actual_keys: BTreeSet<&str> = obj.keys().map(String::as_str).collect();
        let expected_keys: BTreeSet<&str> = [
            "agent_id",
            "created_at",
            "expires_at",
            "lease_id",
            "secret_name",
        ]
        .into_iter()
        .collect();
        assert_eq!(actual_keys, expected_keys);
    }
}
