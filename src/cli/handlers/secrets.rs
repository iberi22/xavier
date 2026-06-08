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
    axum::Extension(session): axum::Extension<crate::cli::http_setup::SessionInfo>,
    Json(payload): Json<LendSecretPayload>,
) -> Response {
    let result = if payload.secret_value.is_empty() {
        state
            .secrets_engine
            .lend_from_vault(
                &payload.secret_name,
                &payload.agent_id,
                payload.ttl_seconds,
                session.is_ephemeral,
            )
            .await
    } else {
        match state
            .secrets_engine
            .lend(
                &payload.secret_name,
                Some(&payload.secret_value),
                &payload.agent_id,
                payload.ttl_seconds,
            )
            .await
        {
            Ok(mut lease) => {
                if session.is_ephemeral {
                    lease.secret_value = None;
                }
                Ok(lease)
            }
            Err(e) => Err(e),
        }
    };

    match result {
        Ok(lease) => json_response(
            StatusCode::OK,
            serde_json::to_value(lease).unwrap_or_default(),
        ),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

pub async fn leases_handler(State(state): State<CliState>) -> Response {
    let leases = state.secrets_engine.list_leases().await;
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
