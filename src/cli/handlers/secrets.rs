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
    Json(payload): Json<LendSecretPayload>,
) -> Response {
    match state
        .secrets_engine
        .lend(
            &payload.secret_name,
            &payload.secret_value,
            &payload.agent_id,
            payload.ttl_seconds,
        )
        .await
    {
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
