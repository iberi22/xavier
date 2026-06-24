//! Shared application state for HTTP handlers
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::Value;

use crate::ports::inbound::{
    AgentLifecyclePort, HealthPort, InputSecurityPort, MemoryQueryPort, SecurityScanPort,
    SessionPort, SessionSyncPort, TimeMetricsPort, VerificationPort,
};
use crate::coordination::events::XavierEventBus;
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
    pub workspace_id: String,
    pub auth_token: String,

    // Code graph components (to be moved to ports in a future phase)
    pub code_db: Arc<code_graph::db::CodeGraphDB>,
    pub code_indexer: Arc<code_graph::indexer::Indexer>,
    pub code_query: Arc<code_graph::query::QueryEngine>,
    pub event_bus: XavierEventBus,
}

/// Check that the `X-Xavier-Token` or `Authorization: Bearer <token>` header matches.
/// Supports XAVIER_BENCHMARK_MODE=true to skip auth (for benchmarking).
pub fn check_auth(headers: &HeaderMap, state: &AppState) -> Result<(), (StatusCode, Json<Value>)> {
    // Benchmark mode: skip auth entirely when env var is set
    if std::env::var("XAVIER_BENCHMARK_MODE").as_deref() == Ok("true") {
        return Ok(());
    }

    // Try X-Xavier-Token first (xavier compatible)
    if let Some(token) = headers.get("X-Xavier-Token").and_then(|v| v.to_str().ok()) {
        if token == state.auth_token {
            return Ok(());
        }
    }
    // Fallback to Authorization: Bearer <token>
    if let Some(auth) = headers.get("Authorization").and_then(|v| v.to_str().ok()) {
        if auth.starts_with("Bearer ") {
            let token = auth.trim_start_matches("Bearer ").trim();
            if token == state.auth_token {
                return Ok(());
            }
        }
    }
    Err((
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({
            "status": "error",
            "message": "Unauthorized",
        })),
    ))
}
