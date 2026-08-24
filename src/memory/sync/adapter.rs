//! Compatibility adapter for memory sync endpoints.
//!
//! Xavier has two families of sync endpoints:
//!
//! - **Old data-plane** (`/v1/memory/*`): raw manifest/push/pull with `Vec<ChunkDiff>` payloads.
//! - **New control-plane** (`/api/v1/memory/sync/*`): orchestrated push/pull via `SyncPeerRequest{peer_url}`.
//!
//! `PeerMemorySync` internally calls the old data-plane endpoints on remote peers.
//! This adapter wraps the HTTP transport so `PeerMemorySync` can transparently
//! talk to peers running either endpoint generation.
//!
//! ## Design
//!
//! ```text
//! PeerMemorySync
//!   └── SyncEndpointAdapter  (enum dispatch)
//!         ├── LegacyAdapter   — /v1/memory/* (data-plane, current default)
//!         └── ControlAdapter  — /api/v1/memory/sync/* (control-plane)
//! ```

use std::time::Duration;

use super::{ChunkDiff, Manifest, ManifestEntry, SyncError, SyncResult};

// ---------------------------------------------------------------------------
// Adapter enum — unified interface
// ---------------------------------------------------------------------------

/// Which endpoint generation to target when talking to a remote peer.
#[derive(Debug, Clone)]
pub enum SyncEndpointAdapter {
    /// Old `/v1/memory/*` data-plane endpoints (raw ChunkDiff payloads).
    Legacy(LegacyAdapter),
    /// New `/api/v1/memory/sync/*` control-plane endpoints.
    ControlPlane(ControlAdapter),
}

impl SyncEndpointAdapter {
    /// Pull the full manifest from a remote peer.
    pub async fn pull_manifest(&self, peer_url: &str) -> SyncResult<Manifest> {
        match self {
            Self::Legacy(a) => a.pull_manifest(peer_url).await,
            Self::ControlPlane(a) => a.pull_manifest(peer_url).await,
        }
    }

    /// Push a batch of diffs to a remote peer and return the count sent.
    pub async fn push_diffs(&self, peer_url: &str, diffs: &[ChunkDiff]) -> SyncResult<u64> {
        match self {
            Self::Legacy(a) => a.push_diffs(peer_url, diffs).await,
            Self::ControlPlane(a) => a.push_diffs(peer_url, diffs).await,
        }
    }

    /// Pull diffs for the given manifest entries.
    pub async fn pull_diffs(
        &self,
        peer_url: &str,
        want: &[ManifestEntry],
    ) -> SyncResult<Vec<ChunkDiff>> {
        match self {
            Self::Legacy(a) => a.pull_diffs(peer_url, want).await,
            Self::ControlPlane(a) => a.pull_diffs(peer_url, want).await,
        }
    }

    /// Pull diffs newer than a timestamp (incremental).
    pub async fn pull_diffs_since(
        &self,
        peer_url: &str,
        workspace_id: &str,
        since_epoch_secs: u64,
    ) -> SyncResult<Vec<ChunkDiff>> {
        match self {
            Self::Legacy(a) => {
                a.pull_diffs_since(peer_url, workspace_id, since_epoch_secs)
                    .await
            }
            Self::ControlPlane(a) => {
                a.pull_diffs_since(peer_url, workspace_id, since_epoch_secs)
                    .await
            }
        }
    }

    /// Create the default (legacy) adapter with optional mesh token.
    pub fn legacy(client: reqwest::Client, peer_token: Option<String>) -> Self {
        Self::Legacy(LegacyAdapter::new(client, peer_token))
    }

    /// Create a control-plane adapter with optional mesh token.
    pub fn control_plane(client: reqwest::Client, peer_token: Option<String>) -> Self {
        Self::ControlPlane(ControlAdapter::new(client, peer_token))
    }

    /// Detect which protocol variant a peer supports by probing the
    /// control-plane status endpoint.
    pub async fn detect(peer_url: &str, client: &reqwest::Client) -> Self {
        let url = format!(
            "{}/api/v1/memory/sync/status",
            peer_url.trim_end_matches('/')
        );
        match client
            .get(&url)
            .timeout(Duration::from_secs(5))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                Self::ControlPlane(ControlAdapter::new(client.clone(), None))
            }
            _ => Self::Legacy(LegacyAdapter::new(client.clone(), None)),
        }
    }
}

// ---------------------------------------------------------------------------
// Legacy adapter  (old `/v1/memory/*` data-plane)
// ---------------------------------------------------------------------------

/// Adapter targeting the old `/v1/memory/*` data-plane endpoints.
///
/// This is the protocol `PeerMemorySync` has always used internally.
#[derive(Debug, Clone)]
pub struct LegacyAdapter {
    client: reqwest::Client,
    peer_token: Option<String>,
}

impl LegacyAdapter {
    pub fn new(client: reqwest::Client, peer_token: Option<String>) -> Self {
        Self { client, peer_token }
    }

    fn apply_token(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(token) = &self.peer_token {
            req.header("X-Xavier-Token", token.as_str())
        } else {
            req
        }
    }

    async fn pull_manifest(&self, peer_url: &str) -> SyncResult<Manifest> {
        let url = format!("{}/v1/memory/manifest", peer_url.trim_end_matches('/'));
        let mut req = self.client.get(&url).timeout(Duration::from_secs(30));
        req = self.apply_token(req);
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(SyncError::Http { status, url, body });
        }
        let manifest: Manifest = resp.json().await?;
        Ok(manifest)
    }

    async fn push_diffs(&self, peer_url: &str, diffs: &[ChunkDiff]) -> SyncResult<u64> {
        if diffs.is_empty() {
            return Ok(0);
        }
        let url = format!("{}/v1/memory/push", peer_url.trim_end_matches('/'));
        let mut req = self
            .client
            .post(&url)
            .json(diffs)
            .timeout(Duration::from_secs(60));
        req = self.apply_token(req);
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(SyncError::Http { status, url, body });
        }
        Ok(diffs.len() as u64)
    }

    async fn pull_diffs(
        &self,
        peer_url: &str,
        want: &[ManifestEntry],
    ) -> SyncResult<Vec<ChunkDiff>> {
        if want.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!("{}/v1/memory/pull", peer_url.trim_end_matches('/'));
        let mut req = self
            .client
            .post(&url)
            .json(want)
            .timeout(Duration::from_secs(60));
        req = self.apply_token(req);
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(SyncError::Http { status, url, body });
        }
        let diffs: Vec<ChunkDiff> = resp.json().await?;
        Ok(diffs)
    }

    async fn pull_diffs_since(
        &self,
        peer_url: &str,
        workspace_id: &str,
        since_epoch_secs: u64,
    ) -> SyncResult<Vec<ChunkDiff>> {
        let url = format!(
            "{}/v1/memory/pull-since/{}/{}",
            peer_url.trim_end_matches('/'),
            workspace_id,
            since_epoch_secs
        );
        let mut req = self.client.get(&url).timeout(Duration::from_secs(60));
        req = self.apply_token(req);
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(SyncError::Http { status, url, body });
        }
        let diffs: Vec<ChunkDiff> = resp.json().await?;
        Ok(diffs)
    }
}

// ---------------------------------------------------------------------------
// Control-plane adapter  (new `/api/v1/memory/sync/*`)
// ---------------------------------------------------------------------------

/// Adapter targeting the new `/api/v1/memory/sync/*` control-plane endpoints.
///
/// Uses `SyncPeerRequest` payloads. Falls back to legacy transport for
/// manifest and incremental pull, since the control-plane is designed for
/// orchestration (push_to/pull_from) rather than raw data exchange.
#[derive(Debug, Clone)]
pub struct ControlAdapter {
    client: reqwest::Client,
    peer_token: Option<String>,
}

impl ControlAdapter {
    pub fn new(client: reqwest::Client, peer_token: Option<String>) -> Self {
        Self { client, peer_token }
    }

    fn apply_token(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(token) = &self.peer_token {
            req.header("X-Xavier-Token", token.as_str())
        } else {
            req
        }
    }

    /// Manifest is always fetched via legacy path (no control-plane equivalent).
    async fn pull_manifest(&self, peer_url: &str) -> SyncResult<Manifest> {
        let url = format!("{}/v1/memory/manifest", peer_url.trim_end_matches('/'));
        let mut req = self.client.get(&url).timeout(Duration::from_secs(30));
        req = self.apply_token(req);
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(SyncError::Http { status, url, body });
        }
        let manifest: Manifest = resp.json().await?;
        Ok(manifest)
    }

    /// Push via control-plane endpoint.
    async fn push_diffs(&self, peer_url: &str, diffs: &[ChunkDiff]) -> SyncResult<u64> {
        if diffs.is_empty() {
            return Ok(0);
        }
        let url = format!("{}/api/v1/memory/sync/push", peer_url.trim_end_matches('/'));
        let mut req = self
            .client
            .post(&url)
            .json(diffs)
            .timeout(Duration::from_secs(60));
        req = self.apply_token(req);
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(SyncError::Http { status, url, body });
        }
        // Control-plane may return a SyncSuccessResponse; try to extract count.
        // If the body is not JSON or doesn't have chunks_sent, fall back to len.
        let body_text = resp.text().await?;
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&body_text) {
            if let Some(count) = val.get("chunks_sent").and_then(|v| v.as_u64()) {
                return Ok(count);
            }
        }
        Ok(diffs.len() as u64)
    }

    /// Pull diffs via control-plane endpoint.
    async fn pull_diffs(
        &self,
        peer_url: &str,
        want: &[ManifestEntry],
    ) -> SyncResult<Vec<ChunkDiff>> {
        if want.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!("{}/api/v1/memory/sync/pull", peer_url.trim_end_matches('/'));
        let mut req = self
            .client
            .post(&url)
            .json(want)
            .timeout(Duration::from_secs(60));
        req = self.apply_token(req);
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(SyncError::Http { status, url, body });
        }
        let diffs: Vec<ChunkDiff> = resp.json().await?;
        Ok(diffs)
    }

    /// Incremental pull falls back to legacy `/v1/memory/pull-since/...`.
    async fn pull_diffs_since(
        &self,
        peer_url: &str,
        workspace_id: &str,
        since_epoch_secs: u64,
    ) -> SyncResult<Vec<ChunkDiff>> {
        let url = format!(
            "{}/v1/memory/pull-since/{}/{}",
            peer_url.trim_end_matches('/'),
            workspace_id,
            since_epoch_secs
        );
        let mut req = self.client.get(&url).timeout(Duration::from_secs(60));
        req = self.apply_token(req);
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(SyncError::Http { status, url, body });
        }
        let diffs: Vec<ChunkDiff> = resp.json().await?;
        Ok(diffs)
    }
}

// ---------------------------------------------------------------------------
// Supported protocol variants
// ---------------------------------------------------------------------------

/// Supported sync protocol variants (for config / env var selection).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncProtocol {
    /// Old `/v1/memory/*` data-plane endpoints.
    Legacy,
    /// New `/api/v1/memory/sync/*` control-plane endpoints.
    ControlPlane,
}

impl SyncProtocol {
    /// Parse from a string (env var or config value).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "legacy" | "data-plane" | "data_plane" => Some(SyncProtocol::Legacy),
            "control-plane" | "control_plane" | "control" => Some(SyncProtocol::ControlPlane),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_from_str() {
        assert_eq!(SyncProtocol::parse("legacy"), Some(SyncProtocol::Legacy));
        assert_eq!(
            SyncProtocol::parse("data-plane"),
            Some(SyncProtocol::Legacy)
        );
        assert_eq!(
            SyncProtocol::parse("control-plane"),
            Some(SyncProtocol::ControlPlane)
        );
        assert_eq!(
            SyncProtocol::parse("control_plane"),
            Some(SyncProtocol::ControlPlane)
        );
        assert_eq!(SyncProtocol::parse("bogus"), None);
    }

    #[test]
    fn protocol_case_insensitive() {
        assert_eq!(SyncProtocol::parse("LEGACY"), Some(SyncProtocol::Legacy));
        assert_eq!(
            SyncProtocol::parse("Control-Plane"),
            Some(SyncProtocol::ControlPlane)
        );
    }

    #[test]
    fn adapter_legacy_construction() {
        let client = reqwest::Client::new();
        let adapter = SyncEndpointAdapter::legacy(client, Some("tok".into()));
        assert!(matches!(adapter, SyncEndpointAdapter::Legacy(_)));
    }

    #[test]
    fn adapter_control_plane_construction() {
        let client = reqwest::Client::new();
        let adapter = SyncEndpointAdapter::control_plane(client, None);
        assert!(matches!(adapter, SyncEndpointAdapter::ControlPlane(_)));
    }
}
