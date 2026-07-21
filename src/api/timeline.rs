//! Timeline API endpoints
//!
//! Provides HTTP endpoints for cognitive calendar navigation (Time Travel).

use axum::{extract::Extension, response::IntoResponse, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::context::timeline::{TimelineEngine, TimelineQuery};
use crate::workspace::WorkspaceContext;

#[derive(Debug, Deserialize)]
pub struct TimeSliceRequest {
    pub query: String,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub agent_id: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    50
}

#[derive(Debug, Serialize)]
pub struct TimeSliceResponse {
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub memories: Vec<crate::api::skills::MemoryRefResponse>,
    pub timeline_events: Vec<TimelineEventResponse>,
}

#[derive(Debug, Serialize)]
pub struct TimelineEventResponse {
    pub timestamp: DateTime<Utc>,
    pub operation: String,
    pub summary: String,
    pub agent_id: String,
}

/// Get time slice.
pub async fn get_time_slice(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(request): Json<TimeSliceRequest>,
) -> impl IntoResponse {
    let engine = TimelineEngine::new(workspace.workspace.memory.clone());

    let query = TimelineQuery {
        query: request.query,
        start_date: request.start_date,
        end_date: request.end_date,
        agent_id: request.agent_id,
        limit: request.limit,
    };

    match engine.get_time_slice(&query).await {
        Ok(slice) => {
            let response = TimeSliceResponse {
                period_start: slice.period_start,
                period_end: slice.period_end,
                memories: slice
                    .memories
                    .into_iter()
                    .map(|m| crate::api::skills::MemoryRefResponse {
                        id: m.id,
                        path: m.path,
                        summary: m.summary,
                        keywords: m.keywords,
                    })
                    .collect(),
                timeline_events: slice
                    .timeline_events
                    .into_iter()
                    .map(|e| TimelineEventResponse {
                        timestamp: e.timestamp,
                        operation: e.operation,
                        summary: e.summary,
                        agent_id: e.agent_id,
                    })
                    .collect(),
            };
            Json(serde_json::json!({ "ok": true, "data": response }))
        }
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

// ---------------------------------------------------------------------------
// GET /timeline
// ---------------------------------------------------------------------------

/// Timeline summary.
pub async fn timeline_summary(
    Extension(workspace): Extension<WorkspaceContext>,
) -> impl IntoResponse {
    let engine = TimelineEngine::new(workspace.workspace.memory.clone());
    let query = TimelineQuery {
        query: String::new(),
        start_date: None,
        end_date: None,
        agent_id: None,
        limit: default_limit(),
    };

    match engine.get_time_slice(&query).await {
        Ok(slice) => Json(serde_json::json!({
            "ok": true,
            "data": {
                "period_start": slice.period_start,
                "period_end": slice.period_end,
                "memories_count": slice.memories.len(),
                "timeline_events_count": slice.timeline_events.len(),
                "timeline_events": slice.timeline_events.into_iter().map(|e| serde_json::json!({
                    "timestamp": e.timestamp,
                    "operation": e.operation,
                    "summary": e.summary,
                    "agent_id": e.agent_id,
                })).collect::<Vec<_>>()
            }
        })),
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}
