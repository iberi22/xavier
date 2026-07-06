//! Secret handlers for key lending and lease management.

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::Response,
    Json,
};

use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use crate::cli::types::*;

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
