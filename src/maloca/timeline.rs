//! Maloca Timeline Export API (MS-002)
//!
//! Exports the cognitive timeline (`timeline_events` + `timeline_sequence`
//! tables) as a flat event feed, grouped sessions, and per-event context.
//! Consumed by `@swal/maloca-client` / swal-backoffice TimelinePage.
//!
//! Contract (mirrors apps/swal-backoffice/src/lib/timelineClient.ts):
//!   GET /maloca/timeline            -> Vec<TimelineEvent>
//!   GET /maloca/timeline/sessions   -> Vec<TimelineSession>
//!   GET /maloca/timeline/{id}/context -> serde_json::Value

use axum::extract::Path;
use axum::response::IntoResponse;
use axum::{Extension, Json};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

use crate::server::events::RealtimeEvent;
use crate::workspace::WorkspaceContext;

/// Event shape returned by `/maloca/timeline`.
#[derive(Debug, Clone, Serialize)]
pub struct MalocaTimelineEvent {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub timestamp: String,
    pub agent: String,
    pub event_type: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_id: Option<String>,
}

/// Session shape returned by `/maloca/timeline/sessions`.
#[derive(Debug, Clone, Serialize)]
pub struct MalocaTimelineSession {
    pub id: String,
    pub start_time: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,
    pub agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    pub event_count: usize,
    #[serde(default)]
    pub key_decisions: Vec<String>,
}

/// Map a raw timeline operation to the frontend's event type taxonomy.
fn classify_operation(op: &str) -> &'static str {
    match op.to_ascii_lowercase().as_str() {
        "decision" | "decision_made" | "criterion_change" => "decision",
        "commit" | "git_commit" | "chronicle" => "commit",
        "error" | "failure" => "error",
        _ => "memory_created",
    }
}

/// Extract entity names from the event payload if present.
fn extract_entities(payload: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(arr) = payload.get("entities").and_then(|v| v.as_array()) {
        for e in arr {
            if let Some(s) = e.as_str() {
                out.push(s.to_string());
            } else if let Some(obj) = e.as_object() {
                if let Some(name) = obj.get("name").and_then(|v| v.as_str()) {
                    out.push(name.to_string());
                }
            }
        }
    }
    out
}

fn event_to_dto(ev: &RealtimeEvent) -> MalocaTimelineEvent {
    let summary = ev
        .payload
        .get("summary")
        .and_then(|s| s.as_str())
        .unwrap_or_default()
        .to_string();
    let operation = ev
        .payload
        .get("operation")
        .and_then(|s| s.as_str())
        .unwrap_or(&ev.event_type)
        .to_string();
    let memory_id = ev
        .payload
        .get("memory_id")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());
    let app_id = ev
        .payload
        .get("app_id")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
        .or_else(|| ev.project_id.clone());
    let context = ev.payload.get("details").cloned();
    MalocaTimelineEvent {
        id: ev.event_id.clone(),
        session_id: memory_id.clone(),
        timestamp: ev.timestamp.clone(),
        agent: ev.agent_id.clone(),
        event_type: classify_operation(&operation).to_string(),
        summary,
        entities: extract_entities(&ev.payload),
        context,
        app_id,
        memory_id,
    }
}

/// List all timeline events (optionally filtered by `since` RFC3339).
pub async fn timeline_export(Extension(ctx): Extension<WorkspaceContext>) -> impl IntoResponse {
    let since = "1970-01-01T00:00:00+00:00".to_string();
    match fetch_events(&ctx, &since).await {
        Ok(events) => Json(serde_json::json!({
            "ok": true,
            "count": events.len(),
            "results": events,
        })),
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

/// Group events into sessions (per agent + memory_id or 30-min windows).
pub async fn timeline_sessions(Extension(ctx): Extension<WorkspaceContext>) -> impl IntoResponse {
    let since = "1970-01-01T00:00:00+00:00".to_string();
    let events = match fetch_events(&ctx, &since).await {
        Ok(e) => e,
        Err(e) => return Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    };

    // Bucket by (agent, memory_id) — each memory_id is one session's stream.
    let mut buckets: HashMap<(String, String), Vec<&MalocaTimelineEvent>> = HashMap::new();
    for ev in &events {
        let key = (
            ev.agent.clone(),
            ev.memory_id
                .clone()
                .unwrap_or_else(|| ev.session_id.clone().unwrap_or_default()),
        );
        buckets.entry(key).or_default().push(ev);
    }

    let mut sessions: Vec<MalocaTimelineSession> = buckets
        .into_iter()
        .map(|((agent, mid), mut evs)| {
            evs.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
            let start = evs.first().map(|e| e.timestamp.clone()).unwrap_or_default();
            let end = evs.last().map(|e| e.timestamp.clone());
            let key_decisions: Vec<String> = evs
                .iter()
                .filter(|e| e.event_type == "decision")
                .filter_map(|e| {
                    if e.summary.is_empty() {
                        None
                    } else {
                        Some(e.summary.clone())
                    }
                })
                .collect();
            MalocaTimelineSession {
                id: if mid.is_empty() {
                    format!("sess-{}", agent)
                } else {
                    mid.clone()
                },
                start_time: start,
                end_time: end,
                agent: agent.clone(),
                app_id: evs.first().and_then(|e| e.app_id.clone()),
                event_count: evs.len(),
                key_decisions,
            }
        })
        .collect();

    sessions.sort_by(|a, b| b.start_time.cmp(&a.start_time));
    let total: usize = events.len();
    Json(serde_json::json!({ "ok": true, "count": total, "results": sessions }))
}

/// Full context (details JSON + memory ref) for a single event id.
pub async fn timeline_event_context(
    Extension(ctx): Extension<WorkspaceContext>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let since = "1970-01-01T00:00:00+00:00".to_string();
    let events = match fetch_events(&ctx, &since).await {
        Ok(e) => e,
        Err(e) => return Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    };
    match events.into_iter().find(|e| e.id == id) {
        Some(ev) => Json(serde_json::json!({
            "ok": true,
            "event": ev,
        })),
        None => Json(serde_json::json!({ "ok": false, "error": "event not found" })),
    }
}

/// Fetch raw events from the memory store (timeline_events table).
async fn fetch_events(
    ctx: &WorkspaceContext,
    since: &str,
) -> anyhow::Result<Vec<MalocaTimelineEvent>> {
    let memory = ctx.workspace.memory.clone();
    let ws = memory.workspace_id();
    let store = memory
        .store()
        .await
        .ok_or_else(|| anyhow::anyhow!("memory store unavailable for workspace {ws}"))?;
    let raw = store.list_timeline_events(ws, since).await?;
    Ok(raw.iter().map(event_to_dto).collect())
}

/// Parse an RFC3339 timestamp; used for validation helpers.
#[allow(dead_code)]
fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}
