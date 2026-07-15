//! Observability log handlers — expose the `ServiceLogStore` to the Panel UI.
//!
//! These endpoints back the "Logs" tab in the administration panel, letting
//! operators browse recent entries, filter by level/source, and read aggregate
//! error/warning statistics without leaving the browser.

use axum::{
    extract::{Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use xavier::observability::service_log::{LogEntry, ObservabilityStats, ServiceLogStore};

/// Query parameters accepted by `GET /api/logs`.
#[derive(Debug, Clone, Deserialize)]
pub struct LogsQuery {
    /// Optional severity filter (`error`, `warn`, `info`, `debug`, `trace`).
    pub level: Option<String>,
    /// Optional source filter (`http_server`, `agent_runtime`, `ui`, ...).
    pub source: Option<String>,
    /// Full-text search across message + metadata. When present this takes
    /// precedence over the level/source filters (uses the FTS5 index).
    pub q: Option<String>,
    /// Maximum number of entries to return (default 200, cap 1000).
    pub limit: Option<u32>,
}

/// `GET /api/logs` — recent log entries, newest first.
///
/// Supports three modes, applied in priority order:
/// 1. `?q=<text>` → FTS5 full-text search via `search_logs`.
/// 2. `?level=error` / `?source=http_server` → filtered recent entries.
/// 3. (no filters) → most recent entries of any level.
pub async fn list_logs(
    State(_state): State<CliState>,
    Query(params): Query<LogsQuery>,
) -> axum::response::Response {
    let limit = params.limit.unwrap_or(200).min(1000);

    let store = match ServiceLogStore::new().await {
        Ok(store) => store,
        Err(error) => {
            return json_response(
                StatusCode::SERVICE_UNAVAILABLE,
                serde_json::json!({
                    "error": "log store unavailable",
                    "detail": error.to_string()
                }),
            )
        }
    };

    let entries: Result<Vec<LogEntry>, _> =
        if let Some(query) = params.q.as_ref().filter(|q| !q.trim().is_empty()) {
            store.search_logs(query, limit).await
        } else {
            let level = params
                .level
                .as_deref()
                .map(|l| l.trim().to_ascii_lowercase());
            let source = params
                .source
                .as_deref()
                .map(|s| s.trim().to_ascii_lowercase());
            store
                .query_recent(level.as_deref(), source.as_deref(), limit)
                .await
        };

    match entries {
        Ok(rows) => json_response(StatusCode::OK, serde_json::json!(rows)),
        Err(error) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": error.to_string() }),
        ),
    }
}

/// `GET /api/logs/stats` — aggregate counts for the dashboard header.
pub async fn log_stats(State(_state): State<CliState>) -> axum::response::Response {
    let stats: Result<ObservabilityStats, _> = async {
        let store = ServiceLogStore::new().await?;
        store.get_stats().await
    }
    .await;

    match stats {
        Ok(stats) => json_response(StatusCode::OK, serde_json::json!(stats)),
        Err(error) => json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            serde_json::json!({
                "error": "log store unavailable",
                "detail": error.to_string(),
                // Return zeroed defaults so the UI can render an empty state
                // instead of a hard failure when the DB is not initialized.
                "total_entries": 0,
                "errors_last_hour": 0,
                "errors_today": 0,
                "warnings_today": 0,
                "active_patterns": 0,
            }),
        ),
    }
}
