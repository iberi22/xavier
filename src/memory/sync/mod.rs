//! Memory Sync — chunk-based LWW sync between Xavier peers.
//!
//! This module provides peer-to-peer memory synchronization using
//! chunk-based diffing and Last-Writer-Wins (LWW) conflict resolution.
//!
//! ## Architecture
//!
//! ```text
//! PeerMemorySync
//!   ├── sync_with(peer)    — full sync (manifest → diff → push/pull)
//!   ├── push_to(peer)      — one-shot push
//!   ├── pull_from(peer)    — one-shot pull
//!   └── sync_loop(peers)   — background loop
//!         ├── diff.rs      — diff two store snapshots
//!         ├── merge.rs     — LWW merge resolution
//!         ├── push_pull.rs — HTTP push/pull transport
//!         └── manifest.rs  — manifest building/reconciliation
//! ```

pub mod adapter;
pub mod diff;
pub mod manifest;
pub mod merge;
pub mod push_pull;

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::memory::store::MemoryStore;

// ---------------------------------------------------------------------------
// SyncError — structured error for peer sync operations
// ---------------------------------------------------------------------------

/// Structured error type for memory sync HTTP operations.
///
/// Unlike a generic `anyhow::Error`, this preserves the HTTP status code and
/// response body so callers can distinguish between transport failures and
/// application-level rejections (e.g. 422 Unprocessable Entity).
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// The peer returned a non-success HTTP status.
    #[error("HTTP {status} from {url}: {body}")]
    Http {
        status: u16,
        url: String,
        body: String,
    },

    /// A reqwest transport / connection error (timeout, DNS, TLS, etc.).
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),

    /// Any other error (deserialization, store, etc.).
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl SyncError {
    /// Returns the HTTP status code, if this error was caused by a non-success
    /// response from a peer. Returns `None` for transport or other errors.
    pub fn http_status(&self) -> Option<u16> {
        match self {
            SyncError::Http { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// Returns the target peer URL, if this error is HTTP-related.
    pub fn peer_url(&self) -> Option<&str> {
        match self {
            SyncError::Http { url, .. } => Some(url),
            _ => None,
        }
    }
}

/// Convenience type alias for sync results.
pub type SyncResult<T> = std::result::Result<T, SyncError>;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Action that a diff describes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DiffAction {
    Add,
    Update,
    Delete,
}

/// A single chunk-level difference between two stores.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkDiff {
    /// SHA-256 hex hash of the chunk content
    pub chunk_hash: String,
    /// Namespace / workspace-id for the chunk
    pub namespace: String,
    /// What to do with this chunk
    pub action: DiffAction,
    /// Serialised chunk payload (present for Add/Update)
    pub data: Option<Vec<u8>>,
    /// Timestamp of the chunk on the source side
    pub timestamp: SystemTime,
    /// The actual record path/id (required for Delete to identify which record to remove).
    #[serde(default)]
    pub record_path: Option<String>,
}

/// Metrics and metadata for a completed sync session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncSession {
    pub peer_id: String,
    pub chunks_sent: u64,
    pub chunks_received: u64,
    pub conflicts: u64,
    pub duration_ms: u64,
    pub success: bool,
}

/// A manifest entry — a compact summary of one chunk's state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub chunk_hash: String,
    pub namespace: String,
    pub revision: u64,
    pub updated_at: DateTime<Utc>,
    pub size_bytes: u64,
    /// The record's logical path (needed so peers can delete by path).
    #[serde(default)]
    pub record_path: Option<String>,
}

/// Full store manifest (list of all entries).
pub type Manifest = Vec<ManifestEntry>;

// ---------------------------------------------------------------------------
// PeerMemorySync
// ---------------------------------------------------------------------------

/// Main API for synchronising memory stores between Xavier peers.
pub struct PeerMemorySync {
    store: Arc<dyn MemoryStore>,
    http_client: reqwest::Client,
    /// How often the background sync loop runs (default 300s).
    pub sync_interval: Duration,
    /// This node's unique identifier (used for LWW tie-breaking).
    pub node_id: String,
    /// Shared mesh credential sent to peers as `X-Xavier-Token`.
    /// Peers in the same SWAL mesh authenticate with the same token.
    peer_token: Option<String>,
    /// Last successful sync timestamp per peer URL.
    last_sync_map: tokio::sync::RwLock<std::collections::HashMap<String, DateTime<Utc>>>,
    /// Endpoint adapter — controls whether this sync client talks to peers
    /// via the old `/v1/memory/*` data-plane or the new `/api/v1/memory/sync/*`
    /// control-plane endpoints.
    endpoint_adapter: adapter::SyncEndpointAdapter,
}

impl PeerMemorySync {
    /// Create a new PeerMemorySync attached to the given store.
    pub fn new(store: Arc<dyn MemoryStore>, node_id: String) -> Self {
        Self::with_peer_token(store, node_id, None)
    }

    /// Create a new PeerMemorySync with an optional shared mesh token.
    pub fn with_peer_token(
        store: Arc<dyn MemoryStore>,
        node_id: String,
        peer_token: Option<String>,
    ) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest::Client::new()");
        Self {
            store,
            endpoint_adapter: adapter::SyncEndpointAdapter::legacy(
                http_client.clone(),
                peer_token.clone(),
            ),
            http_client,
            sync_interval: Duration::from_secs(300),
            node_id,
            peer_token,
            last_sync_map: tokio::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// Create a new PeerMemorySync with an explicit endpoint adapter.
    pub fn with_adapter(
        store: Arc<dyn MemoryStore>,
        node_id: String,
        peer_token: Option<String>,
        endpoint_adapter: adapter::SyncEndpointAdapter,
    ) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest::Client::new()");
        Self {
            store,
            endpoint_adapter,
            http_client,
            sync_interval: Duration::from_secs(300),
            node_id,
            peer_token,
            last_sync_map: tokio::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// Access the endpoint adapter (for testing / reconfiguration).
    pub fn adapter(&self) -> &adapter::SyncEndpointAdapter {
        &self.endpoint_adapter
    }

    /// Retrieve the last successful sync timestamp for a given peer URL.
    pub async fn last_sync_at(&self, peer_url: &str) -> Option<DateTime<Utc>> {
        let map = self.last_sync_map.read().await;
        map.get(peer_url).cloned()
    }

    /// Borrow the underlying memory store.
    ///
    /// Used by the HTTP sync handlers (e.g. conflict resolution that must
    /// force-apply a remote chunk, bypassing the normal LWW transport).
    pub fn store(&self) -> &Arc<dyn MemoryStore> {
        &self.store
    }

    /// Full two-way sync against a remote peer.
    ///
    /// 1. Pull the peer's manifest
    /// 2. Diff local vs remote manifest → determine what to send/receive
    /// 3. Push local changes newer than the peer's last sync
    /// 4. Pull remote changes newer than our last sync
    pub async fn sync_with(&self, peer_url: &str) -> SyncResult<SyncSession> {
        let start = std::time::Instant::now();

        // 1. Build local manifest
        let local_manifest = crate::memory::sync::manifest::build_manifest(&*self.store)
            .await
            .map_err(SyncError::Other)?;

        // 2. Pull remote manifest
        let remote_manifest = self.pull_manifest(peer_url).await?;

        // 3. Diff: determine what to push and pull
        let (to_push, to_pull) =
            crate::memory::sync::diff::diff_manifests(&local_manifest, &remote_manifest)
                .map_err(SyncError::Other)?;

        // 4. Push local changes — collect full chunk data from store
        let push_diffs =
            crate::memory::sync::push_pull::entries_as_push_diffs(&*self.store, &to_push)
                .await
                .map_err(SyncError::Other)?;
        let chunks_sent = self.push_diffs_raw(peer_url, &push_diffs).await?;

        // 5. Pull remote changes
        let received = self.pull_diffs(peer_url, &to_pull).await?;
        let chunks_received = received.len() as u64;

        // 6. Apply received diffs with LWW merge
        let mut conflicts = 0u64;
        crate::memory::sync::merge::apply_changes_received(&*self.store, &received, &mut conflicts)
            .await
            .map_err(SyncError::Other)?;

        let duration_ms = start.elapsed().as_millis() as u64;
        let session = SyncSession {
            peer_id: peer_url.to_string(),
            chunks_sent,
            chunks_received,
            conflicts,
            duration_ms,
            success: true,
        };
        self.last_sync_map
            .write()
            .await
            .insert(peer_url.to_string(), Utc::now());
        Ok(session)
    }

    /// One-shot push: send local chunks newer than `since` to peer.
    pub async fn push_to(
        &self,
        peer_url: &str,
        workspace_id: &str,
        since: SystemTime,
    ) -> SyncResult<SyncSession> {
        let start = std::time::Instant::now();
        let diffs = crate::memory::sync::push_pull::collect_changes_since(
            &*self.store,
            workspace_id,
            since,
        )
        .await
        .map_err(SyncError::Other)?;
        let chunks_sent = self.push_diffs_raw(peer_url, &diffs).await?;
        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(SyncSession {
            peer_id: peer_url.to_string(),
            chunks_sent,
            chunks_received: 0,
            conflicts: 0,
            duration_ms,
            success: true,
        })
    }

    /// One-shot pull: fetch peer's chunks newer than `since`.
    pub async fn pull_from(
        &self,
        peer_url: &str,
        workspace_id: &str,
        since: SystemTime,
    ) -> SyncResult<SyncSession> {
        let start = std::time::Instant::now();
        let received = self.pull_diffs_since(peer_url, workspace_id, since).await?;
        let chunks_received = received.len() as u64;
        let mut conflicts = 0u64;
        crate::memory::sync::merge::apply_changes_received(&*self.store, &received, &mut conflicts)
            .await
            .map_err(SyncError::Other)?;
        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(SyncSession {
            peer_id: peer_url.to_string(),
            chunks_sent: 0,
            chunks_received,
            conflicts,
            duration_ms,
            success: true,
        })
    }

    /// Background sync loop (runs every `sync_interval`).
    ///
    /// Calls `sync_with` on each peer in order. Runs until `stop` is true.
    pub async fn sync_loop(
        &self,
        peers: Vec<String>,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) {
        tracing::info!(
            "sync_loop started: {} peers, interval={:?}",
            peers.len(),
            self.sync_interval
        );
        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            for peer in &peers {
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                match self.sync_with(peer).await {
                    Ok(session) => {
                        tracing::info!(
                            "sync_with {peer}: sent={} recv={} conflicts={} dur={}ms ok={}",
                            session.chunks_sent,
                            session.chunks_received,
                            session.conflicts,
                            session.duration_ms,
                            session.success,
                        );
                    }
                    Err(e) => {
                        tracing::warn!("sync_with {peer} failed: {e:#}");
                    }
                }
            }
            // Sleep for the interval (check stop periodically)
            let interval = self.sync_interval;
            for _ in 0..(interval.as_secs() / 5).max(1) {
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }

    /// Spawn a background sync loop task that periodically syncs with all peers.
    ///
    /// Reads peer URLs from `peers` (or, when empty, from the `XAVIER_PEERS`
    /// environment variable — comma-separated URLs). Returns a
    /// [`JoinHandle`] and a stop-flag that can be flipped to terminate the loop.
    pub fn spawn_background_sync(
        self: &Arc<Self>,
        peers: Vec<String>,
    ) -> (
        tokio::task::JoinHandle<()>,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) {
        // Resolve peers: explicit list wins, then env var fallback.
        let resolved_peers = if !peers.is_empty() {
            peers
        } else {
            Self::peers_from_env()
        };

        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_clone = stop.clone();
        let sync = Arc::clone(self);

        let handle = tokio::spawn(async move {
            if resolved_peers.is_empty() {
                tracing::info!(
                    "sync_loop: no peers configured (set XAVIER_PEERS or \
                     register peers in the mesh registry); background sync idle"
                );
                return;
            }
            tracing::info!(
                "sync_loop: spawning with {} peers (interval={:?})",
                resolved_peers.len(),
                sync.sync_interval
            );
            sync.sync_loop(resolved_peers, stop_clone).await;
        });

        (handle, stop)
    }

    /// Parse peer URLs from the `XAVIER_PEERS` environment variable.
    ///
    /// Expected format: comma-separated URLs, e.g.
    /// `http://peer1:8080, http://peer2:8080`
    pub fn peers_from_env() -> Vec<String> {
        std::env::var("XAVIER_PEERS")
            .ok()
            .map(|val| {
                val.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Ping a peer to check if it's alive.
    pub async fn ping(&self, peer_url: &str) -> bool {
        let url = format!("{}/health", peer_url.trim_end_matches('/'));
        let mut req = self.http_client.get(&url).timeout(Duration::from_secs(5));
        if let Some(token) = &self.peer_token {
            req = req.header("X-Xavier-Token", token);
        }
        req.send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Pull the entire manifest from a peer.
    async fn pull_manifest(&self, peer_url: &str) -> SyncResult<Manifest> {
        self.endpoint_adapter.pull_manifest(peer_url).await
    }

    /// Push a batch of diffs to a peer and return how many were sent.
    async fn push_diffs_raw(&self, peer_url: &str, diffs: &[ChunkDiff]) -> SyncResult<u64> {
        self.endpoint_adapter.push_diffs(peer_url, diffs).await
    }

    /// Pull diffs (full manifest comparison) from a peer.
    async fn pull_diffs(
        &self,
        peer_url: &str,
        want: &[ManifestEntry],
    ) -> SyncResult<Vec<ChunkDiff>> {
        self.endpoint_adapter.pull_diffs(peer_url, want).await
    }

    /// Pull diffs newer than a timestamp (incremental).
    async fn pull_diffs_since(
        &self,
        peer_url: &str,
        workspace_id: &str,
        since: SystemTime,
    ) -> SyncResult<Vec<ChunkDiff>> {
        let since_epoch = since
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.endpoint_adapter
            .pull_diffs_since(peer_url, workspace_id, since_epoch)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // SyncError tests
    // -----------------------------------------------------------------------

    #[test]
    fn sync_error_http_captures_status_url_body() {
        let err = SyncError::Http {
            status: 422,
            url: "http://peer:8080/v1/memory/manifest".to_string(),
            body: "invalid manifest".to_string(),
        };
        assert_eq!(err.http_status(), Some(422));
        assert_eq!(err.peer_url(), Some("http://peer:8080/v1/memory/manifest"));
        assert_eq!(
            err.to_string(),
            "HTTP 422 from http://peer:8080/v1/memory/manifest: invalid manifest"
        );
    }

    #[test]
    fn sync_error_transport_has_no_http_status() {
        // Test that Other variant returns None for http_status and peer_url
        let sync_err = SyncError::Other(anyhow::anyhow!("test error"));
        assert_eq!(sync_err.http_status(), None);
        assert_eq!(sync_err.peer_url(), None);
    }

    #[test]
    fn sync_error_other_wraps_anyhow() {
        let inner = anyhow::anyhow!("store read failed");
        let err = SyncError::Other(inner);
        assert_eq!(err.http_status(), None);
        assert_eq!(err.peer_url(), None);
        assert!(err.to_string().contains("store read failed"));
    }

    #[test]
    fn sync_error_is_send_sync() {
        // Ensure SyncError can be used across async boundaries.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SyncError>();
    }

    // -----------------------------------------------------------------------
    // peers_from_env tests
    // -----------------------------------------------------------------------

    #[test]
    fn peers_from_env_empty_when_unset() {
        // Clear any existing value to be safe.
        std::env::remove_var("XAVIER_PEERS");
        assert!(PeerMemorySync::peers_from_env().is_empty());
    }

    #[test]
    fn peers_from_env_parses_comma_separated() {
        std::env::set_var("XAVIER_PEERS", "http://a:8080, http://b:9090");
        let peers = PeerMemorySync::peers_from_env();
        assert_eq!(peers, vec!["http://a:8080", "http://b:9090"]);
        std::env::remove_var("XAVIER_PEERS");
    }

    #[test]
    fn peers_from_env_skips_empty_segments() {
        std::env::set_var("XAVIER_PEERS", "http://a:8080,,, http://b:9090, ");
        let peers = PeerMemorySync::peers_from_env();
        assert_eq!(peers, vec!["http://a:8080", "http://b:9090"]);
        std::env::remove_var("XAVIER_PEERS");
    }
}
