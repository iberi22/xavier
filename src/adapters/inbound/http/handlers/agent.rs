//! HTTP handler for agent lifecycle operations
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::adapters::inbound::http::AppState;
use crate::coordination::agent_registry::AgentMetadata;
use axum::{extract::State, Json, response::IntoResponse};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct AgentRegisterPayload {
    pub agent_id: String,
    pub session_id: String,
    pub name: Option<String>,
    pub capabilities: Option<Vec<String>>,
    pub role: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AgentRegisterResponse {
    pub status: &'static str,
    pub agent_id: String,
    pub session_id: String,
    pub message: &'static str,
}

/// Agent register handler.
pub async fn agent_register_handler(
    State(state): State<AppState>,
    Json(payload): Json<AgentRegisterPayload>,
) -> impl IntoResponse {
    let metadata = AgentMetadata {
        name: payload.name,
        capabilities: payload.capabilities.unwrap_or_default(),
        role: payload.role,
        endpoint: None,
    };

    let success = state
        .agent_lifecycle
        .register(
            payload.agent_id.clone(),
            payload.session_id.clone(),
            metadata,
        )
        .await;

    Json(AgentRegisterResponse {
        status: if success { "ok" } else { "error" },
        agent_id: payload.agent_id,
        session_id: payload.session_id,
        message: if success { "Agent registered successfully" } else { "Registration failed" },
    })
}

#[derive(Debug, Serialize)]
pub struct AgentHeartbeatResponse {
    pub status: &'static str,
    pub agent_id: String,
    pub message: &'static str,
}

/// Agent heartbeat handler.
pub async fn agent_heartbeat_handler(
    State(state): State<AppState>,
    axum::extract::Path(agent_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let success = state.agent_lifecycle.heartbeat(&agent_id).await;

    Json(AgentHeartbeatResponse {
        status: if success { "ok" } else { "error" },
        agent_id,
        message: if success { "Heartbeat recorded" } else { "Agent not found" },
    })
}

#[derive(Debug, Serialize)]
pub struct AgentActiveResponse {
    pub status: &'static str,
    pub active_agents: usize,
    pub agents: Vec<crate::coordination::agent_registry::AgentEntry>,
}

/// Agent active handler.
pub async fn agent_active_handler(State(state): State<AppState>) -> impl IntoResponse {
    let active = state.agent_lifecycle.get_active_agents().await;

    Json(AgentActiveResponse {
        status: "ok",
        active_agents: active.len(),
        agents: active,
    })
}

#[derive(Debug, Serialize)]
pub struct AgentUnregisterResponse {
    pub status: &'static str,
    pub agent_id: String,
    pub message: &'static str,
}

/// Agent unregister handler.
pub async fn agent_unregister_handler(
    State(state): State<AppState>,
    axum::extract::Path(agent_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let success = state.agent_lifecycle.unregister(&agent_id).await;

    Json(AgentUnregisterResponse {
        status: if success { "ok" } else { "error" },
        agent_id,
        message: if success { "Agent unregistered" } else { "Agent not found" },
    })
}

#[derive(Debug, Deserialize)]
pub struct AgentPushContextPayload {
    pub content: String,
    pub importance: Option<f32>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct AgentPushContextResponse {
    pub status: &'static str,
    pub agent_id: String,
    pub message: &'static str,
    pub importance: f32,
}

/// Agent push context handler.
pub async fn agent_push_context_handler(
    State(_state): State<AppState>,
    axum::extract::Path(agent_id): axum::extract::Path<String>,
    Json(payload): Json<AgentPushContextPayload>,
) -> impl IntoResponse {
    // In a real implementation, this would use a port to add context to the memory store
    // tied to the agent's session.
    let importance = payload.importance.unwrap_or(0.5);

    // Placeholder logic for now, similar to what might be in cli.rs
    Json(AgentPushContextResponse {
        status: "ok",
        agent_id,
        message: "Context pushed (placeholder)",
        importance,
    })
}
