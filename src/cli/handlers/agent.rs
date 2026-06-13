//! Agent handlers for registration, heartbeat, and lifecycle management.

use axum::{
    extract::{Path as AxumPath, State},
    Json,
};

use crate::cli::security::secure_external_input;
use crate::cli::state::CliState;
use crate::cli::types::*;

use xavier::memory::schema::MemoryLevel;
use xavier::memory::store::MemoryRecord;

pub async fn agent_register_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<AgentRegisterPayload>,
) -> impl axum::response::IntoResponse {
    let metadata = xavier::coordination::agent_registry::AgentMetadata {
        name: payload.name,
        capabilities: payload.capabilities.unwrap_or_default(),
        role: payload.role,
        endpoint: payload.endpoint,
    };
    let session_id = payload
        .session_id
        .unwrap_or_else(|| payload.agent_id.clone());

    let success = state
        .agent_registry
        .register(payload.agent_id.clone(), session_id.clone(), metadata)
        .await;

    axum::Json(serde_json::json!({
        "status": if success { "ok" } else { "error" },
        "agent_id": payload.agent_id,
        "session_id": session_id,
        "message": if success { "Agent registered successfully" } else { "Registration failed" },
    }))
}

pub async fn agent_heartbeat_handler(
    State(state): State<CliState>,
    AxumPath(agent_id): AxumPath<String>,
) -> impl axum::response::IntoResponse {
    let success = state.agent_registry.heartbeat(&agent_id).await;

    axum::Json(serde_json::json!({
        "status": if success { "ok" } else { "error" },
        "agent_id": agent_id,
        "message": if success { "Heartbeat recorded" } else { "Agent not found" },
    }))
}

pub async fn agent_active_handler(
    State(state): State<CliState>,
) -> impl axum::response::IntoResponse {
    let active = state.agent_registry.get_active_agents().await;

    axum::Json(serde_json::json!({
        "status": "ok",
        "active_agents": active.len(),
        "agents": active.iter().map(|a| serde_json::json!({
            "agent_id": a.agent_id,
            "session_id": a.session_id,
            "last_heartbeat": a.last_heartbeat.to_rfc3339(),
            "name": a.metadata.name,
            "capabilities": a.metadata.capabilities,
            "role": a.metadata.role,
            "endpoint": a.metadata.endpoint,
        })).collect::<Vec<_>>(),
    }))
}

pub async fn agent_push_context_handler(
    State(state): State<CliState>,
    AxumPath(agent_id): AxumPath<String>,
    axum::Json(payload): axum::Json<AgentPushContextPayload>,
) -> impl axum::response::IntoResponse {
    let agent = state.agent_registry.get(&agent_id).await;
    if agent.is_none() {
        return axum::Json(serde_json::json!({
            "status": "error",
            "message": "Agent not registered",
        }));
    }

    let context =
        match secure_external_input(state.security.as_ref(), "agent context", &payload.context)
            .await
        {
            Ok(context) => context,
            Err(response) => return axum::Json(response),
        };

    let path = format!("agents/{}/context", agent_id);
    let mut metadata = payload.metadata.unwrap_or(serde_json::json!({}));
    if let Some(obj) = metadata.as_object_mut() {
        obj.insert("agent_id".to_string(), serde_json::json!(agent_id));
        obj.insert(
            "pushed_at".to_string(),
            serde_json::json!(chrono::Utc::now().to_rfc3339()),
        );
    }

    let record = MemoryRecord {
        id: String::new(),
        workspace_id: state.workspace_id.clone(),
        path: path.clone(),
        content: context,
        metadata,
        embedding: vec![],
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        revision: 1,
        primary: true,
        parent_id: None,
        cluster_id: None,
        level: MemoryLevel::Raw,
        relation: None,
        clearance: Default::default(),
        revisions: vec![],
        encrypted_dek: None,
        content_iv: None,
        metadata_iv: None,
    };
    match state.memory.add(record).await {
        Ok(doc_id) => axum::Json(serde_json::json!({
            "status": "ok",
            "path": path,
            "document_id": doc_id,
            "message": "Context stored successfully",
        })),
        Err(e) => axum::Json(serde_json::json!({
            "status": "error",
            "message": format!("Failed to store context: {}", e),
        })),
    }
}

pub async fn agent_unregister_handler(
    State(state): State<CliState>,
    AxumPath(agent_id): AxumPath<String>,
) -> impl axum::response::IntoResponse {
    let success = state.agent_registry.unregister(&agent_id).await;

    axum::Json(serde_json::json!({
        "status": if success { "ok" } else { "error" },
        "agent_id": agent_id,
        "message": if success { "Agent unregistered" } else { "Agent not found or already unregistered" },
    }))
}

pub async fn agent_list_handler(
    State(state): State<CliState>,
) -> impl axum::response::IntoResponse {
    let agents = state.agent_registry.get_active_agents().await;
    Json(serde_json::json!({
        "agents": agents.iter().map(|a| serde_json::json!({
            "id": a.agent_id,
            "session_id": a.session_id,
            "last_heartbeat": a.last_heartbeat,
        })).collect::<Vec<_>>(),
        "count": agents.len()
    }))
}
