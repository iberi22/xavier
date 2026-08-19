//! HTTP handlers for memory synchronisation endpoints.
//!
//! Exposes peer-to-peer memory sync over the REST API:
//!
//! - `POST /api/v1/memory/sync/push`                — push local changes to a remote peer
//! - `POST /api/v1/memory/sync/pull`                — pull changes from a remote peer
//! - `GET  /api/v1/memory/sync/status`              — current sync status / last session
//! - `POST /api/v1/memory/sync/resolve/{conflict_id}` — explicitly resolve a conflict
//!
//! These routes live on the same router as the rest of `routes.rs`, whose state
//! type is `Arc<dyn AgentLifecyclePort>`. Because that state does not carry a
//! `MemoryStore`, the handlers read the active `PeerMemorySync` from a
//! module-level singleton (mirroring the `TIME_STORE` / `HEALTH_PORT` pattern
//! already used in `routes.rs`). `init_memory_sync` wires the singleton at
//! startup; until then the handlers report `not_initialized` rather than panic.

use std::sync::{Arc, LazyLock, RwLock};
use std::time::{Duration, SystemTime};

use axum::{extract::Path, Json};
use serde::Deserialize;
use serde_json::json;

use crate::memory::sync::{merge::deserialise_chunk, ChunkDiff, PeerMemorySync, SyncSession};

/// Default workspace used when a request omits `workspace_id`.
const DEFAULT_WORKSPACE: &str = "default";

// ---------------------------------------------------------------------------
// Module-level singletons
// ---------------------------------------------------------------------------

/// The active sync service. Set once at startup via [`init_memory_sync`].
static MEMORY_SYNC: LazyLock<RwLock<Option<Arc<PeerMemorySync>>>> =
    LazyLock::new(|| RwLock::new(None));

/// The most recently completed sync session (set after each push/pull).
static LAST_SESSION: LazyLock<RwLock<Option<SyncSession>>> = LazyLock::new(|| RwLock::new(None));

/// Conflict IDs that have been explicitly resolved via the resolve endpoint.
static RESOLVED_CONFLICTS: LazyLock<RwLock<Vec<String>>> =
    LazyLock::new(|| RwLock::new(Vec::new()));

/// Wire the active `PeerMemorySync` into the module singleton.
///
/// Call once at startup. Safe to call again to replace the active service
/// (used by tests).
pub fn init_memory_sync(sync: Arc<PeerMemorySync>) {
    if let Ok(mut guard) = MEMORY_SYNC.write() {
        *guard = Some(sync);
    } else {
        tracing::error!("MEMORY_SYNC lock poisoned while initialising memory sync");
    }
}

/// Snapshot of the active sync service, if one has been initialised.
fn current_sync() -> Option<Arc<PeerMemorySync>> {
    MEMORY_SYNC.read().ok().and_then(|guard| guard.clone())
}

/// Record the outcome of a completed sync session.
fn set_last_session(session: SyncSession) {
    if let Ok(mut guard) = LAST_SESSION.write() {
        *guard = Some(session);
    }
}

// ---------------------------------------------------------------------------
// Request / response payloads
// ---------------------------------------------------------------------------

/// Body for push/pull requests.
#[derive(Debug, Deserialize)]
pub struct SyncPeerRequest {
    /// Base URL of the remote peer, e.g. `http://peer.local:8080`.
    pub peer_url: String,
    /// Workspace to sync. Defaults to [`DEFAULT_WORKSPACE`].
    #[serde(default)]
    pub workspace_id: Option<String>,
    /// Only sync records newer than this. Accepts epoch seconds or RFC 3339.
    /// Defaults to "all history" (UNIX_EPOCH) when omitted.
    #[serde(default)]
    pub since: Option<String>,
}

/// Body for conflict-resolution requests.
#[derive(Debug, Deserialize)]
pub struct SyncResolveRequest {
    /// Which side wins: `"local"` (keep ours) or `"remote"` (take theirs).
    pub resolution: String,
    /// The remote chunk to force-apply when `resolution == "remote"`.
    #[serde(default)]
    pub chunk: Option<ChunkDiff>,
}

use axum::response::IntoResponse;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SyncStatusResponse {
    pub status: &'static str,
    pub initialized: bool,
    pub node_id: String,
    pub sync_interval_secs: u64,
    pub last_session: Option<SyncSession>,
    pub resolved_conflicts: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SyncSuccessResponse {
    pub status: &'static str,
    pub session: SyncSession,
}

#[derive(Debug, Serialize)]
pub struct SyncErrorResponse {
    pub status: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SyncResolveResponse {
    pub status: &'static str,
    pub conflict_id: String,
    pub resolution: String,
    pub applied: bool,
}

#[derive(Debug, Serialize)]
pub struct SyncResolveErrorResponse {
    pub status: &'static str,
    pub message: &'static str,
    pub conflict_id: String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /api/v1/memory/sync/status` — report sync status.
pub async fn sync_status_handler() -> impl IntoResponse {
    let sync = current_sync();

    let (node_id, sync_interval_secs) = sync
        .as_ref()
        .map(|s| (s.node_id.clone(), s.sync_interval.as_secs()))
        .unwrap_or_else(|| (String::new(), 0));

    let last_session = LAST_SESSION.read().ok().and_then(|guard| guard.clone());

    let resolved_conflicts = RESOLVED_CONFLICTS
        .read()
        .ok()
        .map(|guard| guard.clone())
        .unwrap_or_default();

    Json(SyncStatusResponse {
        status: "ok",
        initialized: sync.is_some(),
        node_id,
        sync_interval_secs,
        last_session,
        resolved_conflicts,
    })
}

/// `POST /api/v1/memory/sync/push` — push local changes to a remote peer.
pub async fn sync_push_handler(Json(req): Json<SyncPeerRequest>) -> impl IntoResponse {
    let Some(sync) = current_sync() else {
        return Json(SyncErrorResponse {
            status: "error",
            message: "memory sync not initialized".to_string(),
            peer_url: None,
        })
        .into_response();
    };

    let workspace_id = req
        .workspace_id
        .unwrap_or_else(|| DEFAULT_WORKSPACE.to_string());
    let since = parse_since(&req.since);

    match sync.push_to(&req.peer_url, &workspace_id, since).await {
        Ok(session) => {
            set_last_session(session.clone());
            Json(SyncSuccessResponse {
                status: "ok",
                session,
            })
            .into_response()
        }
        Err(e) => Json(SyncErrorResponse {
            status: "error",
            message: e.to_string(),
            peer_url: Some(req.peer_url),
        })
        .into_response(),
    }
}

/// `POST /api/v1/memory/sync/pull` — pull changes from a remote peer.
pub async fn sync_pull_handler(Json(req): Json<SyncPeerRequest>) -> impl IntoResponse {
    let Some(sync) = current_sync() else {
        return Json(SyncErrorResponse {
            status: "error",
            message: "memory sync not initialized".to_string(),
            peer_url: None,
        })
        .into_response();
    };

    let workspace_id = req
        .workspace_id
        .unwrap_or_else(|| DEFAULT_WORKSPACE.to_string());
    let since = parse_since(&req.since);

    match sync.pull_from(&req.peer_url, &workspace_id, since).await {
        Ok(session) => {
            set_last_session(session.clone());
            Json(SyncSuccessResponse {
                status: "ok",
                session,
            })
            .into_response()
        }
        Err(e) => Json(SyncErrorResponse {
            status: "error",
            message: e.to_string(),
            peer_url: Some(req.peer_url),
        })
        .into_response(),
    }
}

/// `POST /api/v1/memory/sync/resolve/{conflict_id}` — resolve a conflict.
///
/// - `resolution: "local"`  → keep the local record (no-op beyond recording).
/// - `resolution: "remote"` → force-apply the provided `chunk` to the store,
///   bypassing LWW so the remote version wins unconditionally.
///
/// The resolution is recorded (idempotently) and surfaced by the status
/// endpoint.
pub async fn sync_resolve_handler(
    Path(conflict_id): Path<String>,
    Json(req): Json<SyncResolveRequest>,
) -> impl IntoResponse {
    let resolution = req.resolution.trim().to_lowercase();
    if resolution != "local" && resolution != "remote" {
        return Json(SyncResolveErrorResponse {
            status: "error",
            message: "resolution must be 'local' or 'remote'",
            conflict_id,
        })
        .into_response();
    }

    let mut applied = false;
    if resolution == "remote" {
        if let Some(sync) = current_sync() {
            if let Some(chunk) = req.chunk.as_ref() {
                if let Some(data) = chunk.data.as_ref() {
                    match deserialise_chunk(data) {
                        Ok(record) => match sync.store().put(record).await {
                            Ok(()) => applied = true,
                            Err(e) => {
                                tracing::warn!(
                                    "sync resolve {}: failed to apply remote chunk: {e}",
                                    conflict_id
                                );
                            }
                        },
                        Err(e) => {
                            tracing::warn!(
                                "sync resolve {}: cannot deserialise remote chunk: {e}",
                                conflict_id
                            );
                        }
                    }
                }
            }
        }
    }

    // Record the resolution idempotently.
    if let Ok(mut guard) = RESOLVED_CONFLICTS.write() {
        if !guard.iter().any(|id| id == &conflict_id) {
            guard.push(conflict_id.clone());
        }
    }

    Json(SyncResolveResponse {
        status: "resolved",
        conflict_id,
        resolution,
        applied,
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse an optional `since` cursor into a `SystemTime`.
///
/// Accepts:
/// - epoch seconds (`"0"`, `"1700000000"`)
/// - RFC 3339 timestamps (`"2026-01-01T00:00:00Z"`)
///
/// `None` / empty / unparseable values fall back to `UNIX_EPOCH` (i.e. sync
/// all history), which matches the one-shot push/pull "everything" semantics.
fn parse_since(since: &Option<String>) -> SystemTime {
    let Some(raw) = since else {
        return SystemTime::UNIX_EPOCH;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return SystemTime::UNIX_EPOCH;
    }
    if let Ok(secs) = trimmed.parse::<u64>() {
        return SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(trimmed) {
        let utc = dt.with_timezone(&chrono::Utc);
        return utc.into();
    }
    SystemTime::UNIX_EPOCH
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_since_none_is_epoch() {
        assert_eq!(parse_since(&None), SystemTime::UNIX_EPOCH);
    }

    #[test]
    fn parse_since_empty_is_epoch() {
        assert_eq!(parse_since(&Some(String::new())), SystemTime::UNIX_EPOCH);
        assert_eq!(
            parse_since(&Some("   ".to_string())),
            SystemTime::UNIX_EPOCH
        );
    }

    #[test]
    fn parse_since_epoch_seconds() {
        let st = parse_since(&Some("0".to_string()));
        assert_eq!(st, SystemTime::UNIX_EPOCH);

        let st = parse_since(&Some("100".to_string()));
        assert_eq!(st, SystemTime::UNIX_EPOCH + Duration::from_secs(100));
    }

    #[test]
    fn parse_since_rfc3339() {
        let st = parse_since(&Some("1970-01-01T00:00:30Z".to_string()));
        assert_eq!(st, SystemTime::UNIX_EPOCH + Duration::from_secs(30));
    }

    #[test]
    fn parse_since_garbage_is_epoch() {
        assert_eq!(
            parse_since(&Some("not-a-date".to_string())),
            SystemTime::UNIX_EPOCH
        );
    }
}
