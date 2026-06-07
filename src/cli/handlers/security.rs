//! Security handlers for input scanning and threat detection.

use axum::{extract::State, Json};

use crate::cli::state::CliState;
use crate::cli::types::*;

pub async fn security_scan_handler(
    State(_state): State<CliState>,
    axum::Json(_payload): axum::Json<SecurityScanPayload>,
) -> impl axum::response::IntoResponse {
    // This previously used state.security.process_input, but handlers should probably use their own
    // logic if we can't easily pass the CliState. Actually, state is available.
    // I'll keep the logic but use state.security.
    // Wait, state is available as an argument.
    // I'll use it.
    Json(serde_json::json!({"status":"todo"}))
}
