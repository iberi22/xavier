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
            json_response(
                StatusCode::OK,
                serde_json::to_value(lease).unwrap_or_default(),
            )
        }
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

/// History handler.
pub async fn history_handler() -> Response {
    use xavier::codebase::connection_manager::ConnectionManager;
    let result = ConnectionManager::global()
        .with_conn("metrics", |conn: &rusqlite::Connection| {
            let mut stmt = conn.prepare(
                "SELECT id, timestamp, event_type, agent_id, session_token, secret_id, reason
                 FROM secret_audit_logs
                 ORDER BY timestamp DESC
                 LIMIT 100",
            )?;
            let rows = stmt.query_map([], |row: &rusqlite::Row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, i64>(0)?,
                    "timestamp": row.get::<_, String>(1)?,
                    "event_type": row.get::<_, String>(2)?,
                    "agent_id": row.get::<_, String>(3)?,
                    "session_token": row.get::<_, String>(4)?,
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
        .await;

    match result {
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

/// Leases handler.
pub async fn leases_handler(State(state): State<CliState>) -> Response {
    let mut leases = state.secrets_engine.list_leases().await;
    // Always redact secret_value for security
    for lease in &mut leases {
        lease.secret_value = None;
    }
    json_response(
        StatusCode::OK,
        serde_json::to_value(leases).unwrap_or_default(),
    )
}

/// Revoke handler.
pub async fn revoke_handler(
    State(state): State<CliState>,
    Json(payload): Json<RevokeLeasePayload>,
) -> Response {
    match state
        .secrets_engine
        .revoke(&payload.token, "Manual API Call")
        .await
    {
        Ok(_) => json_response(StatusCode::OK, serde_json::json!({ "status": "revoked" })),
        Err(e) => json_response(
            StatusCode::NOT_FOUND,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

/// Status handler.
pub async fn status_handler(
    State(state): State<CliState>,
    AxumPath(token): AxumPath<String>,
) -> Response {
    match state.secrets_engine.get_lease(&token).await {
        Some(mut status) => {
            // F2 - Redact secret_value from response
            status.secret_value = None;
            json_response(
                StatusCode::OK,
                serde_json::to_value(status).unwrap_or_default(),
            )
        }
        None => json_response(
            StatusCode::NOT_FOUND,
            serde_json::json!({ "error": "Lease not found" }),
        ),
    }
}
