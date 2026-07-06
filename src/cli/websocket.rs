//! Handlers for session events and memory compaction.
//!
//! This module manages session-related events, including indexing chat entries
//! into memory and performing periodic compaction to optimize context storage.

use crate::cli::security::secure_external_input;
use crate::cli::state::CliState;
use crate::cli::types::SessionCompactPayload;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use tracing::info;
use xavier::memory::schema::MemoryLevel;
use xavier::memory::store::MemoryRecord;
use xavier::session::event_mapper::PanelThreadEntry;
use xavier::session::types::SessionEvent;

pub async fn session_event_handler(
    State(state): State<CliState>,
    axum::Json(event): axum::Json<SessionEvent>,
) -> impl axum::response::IntoResponse {
    if matches!(
        event.event_type,
        xavier::session::types::SessionEventType::SessionEnd
    ) {
        info!(
            "Session {} ended. Revoking associated agent leases...",
            event.session_id
        );

        // Find agent associated with this session
        let active_agents = state.agent_registry.get_active_agents().await;
        if let Some(agent) = active_agents
            .iter()
            .find(|a| a.session_id == event.session_id)
        {
            state
                .secrets_engine
                .revoke_for_agent(&agent.agent_id, "Session Ended")
                .await;
        }

        return axum::Json(serde_json::json!({
            "status": "ok",
            "message": "session_end_processed",
            "session_id": event.session_id,
        }));
    }

    let entry = match PanelThreadEntry::from_session_event(&event) {
        Some(e) => e,
        None => {
            return axum::Json(serde_json::json!({
                "status": "skipped",
                "reason": "no_content",
                "session_id": event.session_id,
            }))
        }
    };

    let entry_content = match secure_external_input(
        state.security.as_ref(),
        "session event content",
        &entry.content,
    )
    .await
    {
        Ok(content) => content,
        Err(response) => return axum::Json(response),
    };

    let content = format!(
        "[{}] {}: {}",
        entry.timestamp.format("%Y-%m-%d %H:%M:%S"),
        entry.role,
        entry_content
    );

    let record_path = format!("sessions/{}/thread", event.session_id);
    let record = MemoryRecord {
        id: String::new(),
        workspace_id: state.workspace_id.clone(),
        path: record_path.clone(),
        content,
        metadata: serde_json::json!({"kind": "Context", "namespace": "Session"}),
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
        score: 0.0,
    };
    match state.memory.add(record).await {
        Ok(id) => {
            info!("Session event indexed: {} -> {}", event.session_id, id);
            axum::Json(serde_json::json!({
                "status": "ok",
                "session_id": event.session_id,
                "path": record_path,
                "id": id,
            }))
        }
        Err(e) => {
            info!("Failed to index session event: {}", e);
            axum::Json(serde_json::json!({
                "status": "error",
                "error": e.to_string(),
            }))
        }
    }
}

pub async fn session_compact_handler(
    State(state): State<CliState>,
    axum::Json(payload): axum::Json<SessionCompactPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let session_id = &payload.session_id;
    let threshold = payload.threshold_percent.clamp(1.0, 100.0);

    let current_tokens = match payload.current_tokens {
        Some(t) => t,
        None => {
            match state
                .memory
                .search(&format!("session {} compact", session_id), 5, None)
                .await
            {
                Ok(docs) => {
                    let total_chars: usize = docs.iter().map(|d| d.content.len()).sum();
                    total_chars / 4
                }
                Err(_) => 0,
            }
        }
    };

    let estimated_max_tokens = 200_000;
    let usage_percent = (current_tokens as f64 / estimated_max_tokens as f64) * 100.0;

    let triggered = usage_percent >= threshold;

    if !triggered {
        return Ok(axum::Json(serde_json::json!({
            "status": "ok",
            "triggered": false,
            "session_id": session_id,
            "usage_percent": usage_percent,
            "threshold_percent": threshold,
            "message": format!(
                "Compaction not needed: {:.1}% < {:.1}%",
                usage_percent,
                threshold
            ),
        })));
    }

    let search_path = format!("sessions/{}/thread", session_id);
    let all_docs = match state.memory.get(&search_path).await {
        Ok(Some(doc)) => vec![doc],
        Ok(None) => state
            .memory
            .search(&search_path, 10, None)
            .await
            .unwrap_or_default(),
        Err(_) => vec![],
    };

    let total_docs = all_docs.len();
    let keep_count = (total_docs as f64 * 0.20).ceil() as usize;
    let compact_docs: Vec<_> = all_docs.iter().rev().take(keep_count).collect();

    let mut compacted_content = String::new();
    compacted_content.push_str(&format!(
        "[COMPACTED] Session {} - Original {} entries, kept {}\n",
        session_id,
        total_docs,
        compact_docs.len()
    ));

    if let Some(oldest) = all_docs.first() {
        compacted_content.push_str(&format!(
            "[EARLIEST] {}\n",
            &oldest.content[..oldest.content.len().min(200)]
        ));
    }

    compacted_content.push_str("\n=== KEPT RECENT ENTRIES ===\n");
    for doc in &compact_docs {
        let truncate_content = if doc.content.len() > 500 {
            format!("{}... [truncated]", &doc.content[..500])
        } else {
            doc.content.clone()
        };
        compacted_content.push_str(&format!("[ENTRY] {}\n\n", truncate_content));
    }

    let compact_path = format!("context/{}/compact", session_id);
    let record = MemoryRecord {
        id: String::new(),
        workspace_id: state.workspace_id.clone(),
        path: compact_path.clone(),
        content: compacted_content.clone(),
        metadata: serde_json::json!({"kind": "Context", "namespace": "Session"}),
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
        score: 0.0,
    };
    match state.memory.add(record).await {
        Ok(id) => {
            info!(
                "Session {} compacted: {} -> {} entries, saved to {}",
                session_id,
                total_docs,
                compact_docs.len(),
                id
            );
            Ok(axum::Json(serde_json::json!({
                "status": "ok",
                "triggered": true,
                "session_id": session_id,
                "usage_percent": usage_percent,
                "threshold_percent": threshold,
                "original_entries": total_docs,
                "kept_entries": compact_docs.len(),
                "compacted_path": compact_path,
                "compacted_id": id,
                "message": format!(
                    "Compacted session {}: {} -> {} entries (kept last 20%)",
                    session_id,
                    total_docs,
                    compact_docs.len()
                ),
            })))
        }
        Err(e) => {
            info!("Session compaction error: {}", e);
            Ok(axum::Json(serde_json::json!({
                "status": "error",
                "triggered": true,
                "session_id": session_id,
                "error": e.to_string(),
            })))
        }
    }
}
