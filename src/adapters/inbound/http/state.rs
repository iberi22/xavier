//! Shared application state for HTTP handlers
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::Value;
use subtle::ConstantTimeEq;

use crate::ports::inbound::{
    AgentLifecyclePort, HealthPort, InputSecurityPort, MemoryQueryPort, SecurityScanPort,
    SessionPort, SessionSyncPort, TimeMetricsPort, VerificationPort,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub memory: Arc<dyn MemoryQueryPort>,
    pub security: Arc<dyn InputSecurityPort>,
    pub security_scan: Arc<dyn SecurityScanPort>,
    pub time_metrics: Arc<dyn TimeMetricsPort>,
    pub agent_lifecycle: Arc<dyn AgentLifecyclePort>,
    pub health: Arc<dyn HealthPort>,
    pub verification: Arc<dyn VerificationPort>,
    pub session_sync: Arc<dyn SessionSyncPort>,
    pub session: Arc<dyn SessionPort>,
    pub espacio: Arc<crate::espacio::SpaceManager>,
    pub workspace_id: String,
    pub auth_token: String,
    pub secrets_engine: Option<Arc<crate::coordination::KeyLendingEngine>>,
}

/// Check that the `X-Xavier-Token` or `Authorization: Bearer <token>` header matches.
pub fn check_auth(headers: &HeaderMap, state: &AppState) -> Result<(), (StatusCode, Json<Value>)> {
    let mut is_authorized = false;
    let expected_bytes = state.auth_token.as_bytes();

    // Helper closure for constant-time comparison that resists timing attacks
    // even for strings of different lengths.
    let check_token = |provided: &str| -> bool {
        let provided_bytes = provided.as_bytes();
        if provided_bytes.len() != expected_bytes.len() {
            // Dummy check to mitigate timing side channels
            let _ = expected_bytes.ct_eq(expected_bytes);
            return false;
        }
        bool::from(provided_bytes.ct_eq(expected_bytes))
    };

    // Try X-Xavier-Token first (xavier compatible)
    if let Some(token) = headers.get("X-Xavier-Token").and_then(|v| v.to_str().ok()) {
        is_authorized = is_authorized || check_token(token);
    }
    // Fallback to Authorization: Bearer <token>
    if let Some(auth) = headers.get("Authorization").and_then(|v| v.to_str().ok()) {
        if auth.starts_with("Bearer ") {
            let token = auth.trim_start_matches("Bearer ").trim();
            is_authorized = is_authorized || check_token(token);
        }
    }

    if is_authorized {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "status": "error",
                "message": "Unauthorized",
            })),
        ))
    }
}
