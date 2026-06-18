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

pub mod diff;
pub mod manifest;
pub mod merge;
pub mod push_pull;

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::memory::store::MemoryStore;

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
}

impl PeerMemorySync {
    /// Create a new PeerMemorySync attached to the given store.
    pub fn new(store: Arc<dyn MemoryStore>, node_id: String) -> Self {
        Self {
            store,
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("reqwest::Client::new()"),
            sync_interval: Duration::from_secs(300),
            node_id,
        }
    }

    /// Full two-way sync against a remote peer.
    ///
    /// 1. Pull the peer's manifest
    /// 2. Diff local vs remote manifest → determine what to send/receive
    /// 3. Push local changes newer than the peer's last sync
    /// 4. Pull remote changes newer than our last sync
    pub async fn sync_with(&self, peer_url: &str) -> Result<SyncSession> {
        let start = std::time::Instant::now();

        // 1. Build local manifest
        let local_manifest = crate::memory::sync::manifest::build_manifest(&*self.store).await?;

        // 2. Pull remote manifest
        let remote_manifest = self.pull_manifest(peer_url).await?;

        // 3. Diff: determine what to push and pull
        let (to_push, to_pull) =
            crate::memory::sync::diff::diff_manifests(&local_manifest, &remote_manifest)?;

        // 4. Push local changes — collect full chunk data from store
        let push_diffs = crate::memory::sync::push_pull::entries_as_push_diffs(
            &*self.store,
            &to_push,
        )
        .await?;
        let chunks_sent = self.push_diffs_raw(peer_url, &push_diffs).await?;

        // 5. Pull remote changes
        let received = self.pull_diffs(peer_url, &to_pull).await?;
        let chunks_received = received.len() as u64;

        // 6. Apply received diffs with LWW merge
        let mut conflicts = 0u64;
        crate::memory::sync::merge::apply_changes_received(&*self.store, &received, &mut conflicts)
            .await?;

        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(SyncSession {
            peer_id: peer_url.to_string(),
            chunks_sent,
            chunks_received,
            conflicts,
            duration_ms,
            success: true,
        })
    }

    /// One-shot push: send local chunks newer than `since` to peer.
    pub async fn push_to(
        &self,
        peer_url: &str,
        workspace_id: &str,
        since: SystemTime,
    ) -> Result<SyncSession> {
        let start = std::time::Instant::now();
        let diffs =
            crate::memory::sync::push_pull::collect_changes_since(&*self.store, workspace_id, since)
                .await?;
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
    ) -> Result<SyncSession> {
        let start = std::time::Instant::now();
        let received = self
            .pull_diffs_since(peer_url, workspace_id, since)
            .await?;
        let chunks_received = received.len() as u64;
        let mut conflicts = 0u64;
        crate::memory::sync::merge::apply_changes_received(&*self.store, &received, &mut conflicts)
            .await?;
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

    /// Ping a peer to check if it's alive.
    pub async fn ping(&self, peer_url: &str) -> bool {
        let url = format!("{}/health", peer_url.trim_end_matches('/'));
        self.http_client
            .get(&url)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Pull the entire manifest from a peer.
    async fn pull_manifest(&self, peer_url: &str) -> Result<Manifest> {
        let url = format!("{}/v1/memory/manifest", peer_url.trim_end_matches('/'));
        let resp = self
            .http_client
            .get(&url)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!(
                "pull_manifest: HTTP {} from {}",
                resp.status(),
                url
            );
        }
        let manifest: Manifest = resp.json().await?;
        Ok(manifest)
    }

    /// Push a batch of diffs to a peer and return how many were sent.
    async fn push_diffs_raw(&self, peer_url: &str, diffs: &[ChunkDiff]) -> Result<u64> {
        if diffs.is_empty() {
            return Ok(0);
        }
        let url = format!("{}/v1/memory/push", peer_url.trim_end_matches('/'));
        let resp = self
            .http_client
            .post(&url)
            .json(diffs)
            .timeout(Duration::from_secs(60))
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!("push_diffs: HTTP {} from {}", resp.status(), url);
        }
        Ok(diffs.len() as u64)
    }

    /// Pull diffs (full manifest comparison) from a peer.
    async fn pull_diffs(&self, peer_url: &str, want: &[ManifestEntry]) -> Result<Vec<ChunkDiff>> {
        if want.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!(
            "{}/v1/memory/pull",
            peer_url.trim_end_matches('/')
        );
        let resp = self
            .http_client
            .post(&url)
            .json(want)
            .timeout(Duration::from_secs(60))
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!("pull_diffs: HTTP {} from {}", resp.status(), url);
        }
        let diffs: Vec<ChunkDiff> = resp.json().await?;
        Ok(diffs)
    }

    /// Pull diffs newer than a timestamp (incremental).
    async fn pull_diffs_since(
        &self,
        peer_url: &str,
        workspace_id: &str,
        since: SystemTime,
    ) -> Result<Vec<ChunkDiff>> {
        let since_epoch = since
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let url = format!(
            "{}/v1/memory/pull-since/{}/{}",
            peer_url.trim_end_matches('/'),
            workspace_id,
            since_epoch
        );
        let resp = self
            .http_client
            .get(&url)
            .timeout(Duration::from_secs(60))
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!(
                "pull_diffs_since: HTTP {} from {}",
                resp.status(),
                url
            );
        }
        let diffs: Vec<ChunkDiff> = resp.json().await?;
        Ok(diffs)
    }
}
