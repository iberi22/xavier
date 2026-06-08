//! Security handlers for input scanning and threat detection.

use axum::{extract::State, http::StatusCode, response::Response, Json};

use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use crate::cli::types::*;

pub async fn security_scan_handler(
    State(_state): State<CliState>,
    axum::Json(_payload): axum::Json<SecurityScanPayload>,
) -> impl axum::response::IntoResponse {
    Json(serde_json::json!({"status":"todo"}))
}

pub async fn session_create_handler(State(state): State<CliState>) -> Response {
    // This handler is protected by auth_middleware which ensures the root XAVIER_TOKEN was used.
    let session = state.session_manager.create_session();

    json_response(
        StatusCode::OK,
        serde_json::json!({
            "status": "ok",
            "session_id": session.id,
            "expires_at": session.expires_at,
        }),
    )
}

#[derive(serde::Deserialize)]
pub struct SecurityApprovePayload {
    pub action: String,
    pub target: String,
}

pub async fn security_approve_handler(
    State(_state): State<CliState>,
    Json(payload): Json<SecurityApprovePayload>,
) -> Response {
    // Use ApprovalStore instead of thread-unsafe environment variables
    xavier::security::APPROVAL_STORE.approve(&payload.action, &payload.target);

    json_response(
        StatusCode::OK,
        serde_json::json!({
            "status": "approved",
            "action": payload.action,
            "target": payload.target,
        }),
    )
}
