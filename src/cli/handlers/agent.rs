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

/// Agent register handler.
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

/// Agent heartbeat handler.
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

/// Agent active handler.
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

/// Agent push context handler.
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
        score: 0.0,
        deleted_at: None,
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

/// Agent unregister handler.
pub async fn agent_unregister_handler(
    State(state): State<CliState>,
    AxumPath(agent_id): AxumPath<String>,
) -> impl axum::response::IntoResponse {
    let success = state.agent_registry.unregister(&agent_id).await;

    if success {
        state
            .secrets_engine
            .revoke_for_agent(&agent_id, "Agent Unregistered")
            .await;
    }

    axum::Json(serde_json::json!({
        "status": if success { "ok" } else { "error" },
        "agent_id": agent_id,
        "message": if success { "Agent unregistered" } else { "Agent not found or already unregistered" },
    }))
}

/// Agent task complete handler.
pub async fn agent_task_complete_handler(
    State(state): State<CliState>,
    AxumPath(agent_id): AxumPath<String>,
) -> impl axum::response::IntoResponse {
    state
        .agent_registry
        .on_task_complete(
            &agent_id,
            "default",
            &Ok(xavier::agents::runtime::AgentResponse {
                session_id: "default".to_string(),
                query: "manual-complete".to_string(),
                response: "Task completed manually".to_string(),
                confidence: 1.0,
                system_timings: xavier::agents::runtime::SystemTimings {
                    system1_ms: 0,
                    system2_ms: 0,
                    system3_ms: 0,
                    total_ms: 0,
                },
            }),
        )
        .await;

    axum::Json(serde_json::json!({
        "status": "ok",
        "agent_id": agent_id,
        "message": "Task completion processed",
    }))
}

/// Agent task failed handler.
pub async fn agent_task_failed_handler(
    State(state): State<CliState>,
    AxumPath(agent_id): AxumPath<String>,
    axum::Json(payload): axum::Json<serde_json::Value>,
) -> impl axum::response::IntoResponse {
    let reason = payload["reason"]
        .as_str()
        .unwrap_or("Unknown reason")
        .to_string();
    state
        .agent_registry
        .on_task_complete(&agent_id, "default", &Err(reason))
        .await;

    axum::Json(serde_json::json!({
        "status": "ok",
        "agent_id": agent_id,
        "message": "Task failure processed",
    }))
}

/// Agent list handler.
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

/// Agent scan handler.
pub async fn agent_scan_handler(
    State(state): State<CliState>,
) -> impl axum::response::IntoResponse {
    match state.agent_indexer.scanner().scan_all().await {
        Ok(sessions) => axum::Json(serde_json::json!({
            "status": "ok",
            "count": sessions.len(),
            "sessions": sessions,
        })),
        Err(e) => axum::Json(serde_json::json!({
            "status": "error",
            "message": format!("Failed to scan agents: {}", e),
        })),
    }
}

/// Agent index handler.
pub async fn agent_index_handler(
    State(state): State<CliState>,
) -> impl axum::response::IntoResponse {
    match state.agent_indexer.index_agents().await {
        Ok(indexed_files) => {
            let mut count = 0;
            for file in indexed_files {
                let record = MemoryRecord {
                    id: uuid::Uuid::new_v4().to_string(),
                    workspace_id: state.workspace_id.clone(),
                    path: file.path,
                    content: file.content,
                    metadata: serde_json::json!({
                        "source": "agent_scanner",
                        "last_modified": file.last_modified,
                        "size": file.size,
                    }),
                    embedding: vec![],
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    revision: 1,
                    primary: true,
                    score: 0.0,
                    deleted_at: None,
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
                if state.memory.add(record).await.is_ok() {
                    count += 1;
                }
            }
            axum::Json(serde_json::json!({
                "status": "ok",
                "indexed_count": count,
            }))
        }
        Err(e) => axum::Json(serde_json::json!({
            "status": "error",
            "message": format!("Failed to index agents: {}", e),
        })),
    }
}

/// Openclaw scan handler.
pub async fn openclaw_scan_handler() -> impl axum::response::IntoResponse {
    let scanner = crate::memory::openclaw_scanner::OpenClawAgentScanner::new();
    match scanner.scan_all_agents().await {
        Ok(agents) => axum::Json(serde_json::json!({
            "status": "ok",
            "count": agents.len(),
            "agents": agents,
        })),
        Err(e) => axum::Json(serde_json::json!({
            "status": "error",
            "message": format!("Failed to scan OpenClaw agents: {}", e),
        })),
    }
}

/// Openclaw index handler.
pub async fn openclaw_index_handler(
    State(state): State<CliState>,
) -> impl axum::response::IntoResponse {
    let scanner = crate::memory::openclaw_scanner::OpenClawAgentScanner::new();
    match state
        .openclaw_indexer
        .index_all_agents(&scanner, state.store.as_ref())
        .await
    {
        Ok(records) => axum::Json(serde_json::json!({
            "status": "ok",
            "indexed_count": records.len(),
        })),
        Err(e) => axum::Json(serde_json::json!({
            "status": "error",
            "message": format!("Failed to index OpenClaw agents: {}", e),
        })),
    }
}

/// Agent sync handler.
pub async fn agent_sync_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<serde_json::Value>,
) -> impl axum::response::IntoResponse {
    // Reuse tasks sync logic for now or implement direct cloud sync trigger
    let _mode = payload["mode"].as_str().unwrap_or("bidirectional");

    match state.store.sync_all(&state.workspace_id).await {
        Ok(stats) => axum::Json(serde_json::json!({
            "status": "ok",
            "message": "Agent memory synchronization completed",
            "stats": stats,
        })),
        Err(e) => axum::Json(serde_json::json!({
            "status": "error",
            "message": format!("Synchronization failed: {}", e),
        })),
    }
}
