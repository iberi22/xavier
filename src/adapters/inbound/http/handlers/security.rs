//! HTTP handler for security operations
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::adapters::inbound::http::AppState;
use axum::{extract::State, Json};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SecurityScanPayload {
    pub input: String,
}

use axum::response::IntoResponse;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SecurityScanDetection {
    pub is_injection: bool,
    pub confidence: f32,
    pub attack_type: String,
}

#[derive(Debug, Serialize)]
pub struct SecurityScanSuccessResponse {
    pub status: &'static str,
    pub allowed: bool,
    pub detection: SecurityScanDetection,
    pub sanitized_input: Option<String>,
    pub original_input: String,
}

#[derive(Debug, Serialize)]
pub struct SecurityScanErrorResponse {
    pub status: &'static str,
    pub message: String,
}

/// Security scan handler.
pub async fn security_scan_handler(
    State(state): State<AppState>,
    Json(payload): Json<SecurityScanPayload>,
) -> impl IntoResponse {
    let result = match state.security.process_input(&payload.input).await {
        Ok(res) => res,
        Err(e) => {
            return Json(SecurityScanErrorResponse {
                status: "error",
                message: format!("Security scan error: {}", e),
            }).into_response();
        }
    };

    Json(SecurityScanSuccessResponse {
        status: if result.allowed { "allowed" } else { "blocked" },
        allowed: result.allowed,
        detection: SecurityScanDetection {
            is_injection: result.is_injection,
            confidence: result.detection_confidence,
            attack_type: result.attack_type,
        },
        sanitized_input: result.sanitized_input,
        original_input: result.original_input,
    }).into_response()
}
