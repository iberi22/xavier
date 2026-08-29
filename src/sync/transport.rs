//! Unified Transport — Abstraction for P2P and Cloud mesh sync
//!
//! Routes sync requests to one of three backends based on the target peer's
//! configuration:
//!
//! - [`SyncTransport::Cloud`] — `CloudPeer` (Supabase REST), for `is_cloud` peers.
//! - [`SyncTransport::P2pIroh`] — [`crate::mesh::iroh_transport::IrohTransport`]
//!   (Iroh QUIC), for peers that carry an `iroh_addr` (Phase 2 mesh). Only
//!   present under the `mesh` cargo feature.
//! - [`SyncTransport::P2P`] — `MeshTransport` (HTTP), the default fallback.

use crate::memory::schema::MemoryQueryFilters;
use crate::mesh::cloud_node::CloudPeer;
#[cfg(feature = "mesh")]
use crate::mesh::iroh_transport::IrohTransport;
use crate::mesh::node::NodeIdentity;
use crate::mesh::peer::PeerInfo;
use crate::mesh::protocol::{MeshHandshakeResponse, MeshManifest};
use crate::mesh::transport::MeshTransport;
use crate::session::sharing::SessionBundle;
use anyhow::Result;
use rand::Rng;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Configuration for mesh P2P reconnection backoff policies.
#[derive(Debug, Clone, Copy)]
pub struct ReconnectBackoffConfig {
    /// Initial base backoff duration.
    pub base: Duration,
    /// Upper ceiling bound for backoff duration.
    pub max: Duration,
    /// Jitter randomization factor between 0.0 and 1.0.
    pub jitter_factor: f64,
}

impl Default for ReconnectBackoffConfig {
    fn default() -> Self {
        Self {
            base: Duration::from_millis(500),
            max: Duration::from_secs(30),
            jitter_factor: 0.5,
        }
    }
}

impl ReconnectBackoffConfig {
    /// Calculate backoff duration for a given attempt number.
    pub fn calculate_backoff(&self, attempt: u32) -> Duration {
        calculate_reconnect_backoff(attempt, self.base, self.max, self.jitter_factor)
    }
}

/// Calculate reconnect backoff duration with exponential increase and randomized full jitter.
///
/// Avoids thundering herd problems when remote peers reconnect simultaneously after a disconnect.
pub fn calculate_reconnect_backoff(
    attempt: u32,
    base: Duration,
    max: Duration,
    jitter_factor: f64,
) -> Duration {
    let base_ms = base.as_millis() as f64;
    let max_ms = max.as_millis() as f64;

    // Exponential delay capped at max: base * 2^attempt
    let exp_ms = (base_ms * 2.0_f64.powi(attempt.min(30) as i32)).min(max_ms);

    if jitter_factor <= 0.0 {
        return Duration::from_millis(exp_ms as u64);
    }

    let clamped_jitter = jitter_factor.clamp(0.0, 1.0);
    let min_ms = exp_ms * (1.0 - clamped_jitter);

    let mut rng = rand::thread_rng();
    let jittered_ms = rng.gen_range(min_ms..=exp_ms);

    Duration::from_millis(jittered_ms as u64)
}

pub enum SyncTransport {
    P2P(MeshTransport),
    Cloud(CloudPeer),
    /// Iroh QUIC transport (Phase 2 mesh). Gated behind the `mesh` feature so the
    /// default build — which has no iroh dependency — compiles without it.
    #[cfg(feature = "mesh")]
    P2pIroh(IrohTransport),
}

/// Whether a peer should be reached over Iroh rather than plain HTTP.
///
/// True iff the peer is not cloud and carries a non-empty `iroh_addr`. Kept as a
/// free function (ungated by `mesh`) so it can be unit-tested in the default
/// `ci-safe` build and so `for_peer` can call it unconditionally — the resulting
/// [`SyncTransport::P2pIroh`] variant is what is `cfg`-gated, not this check.
pub fn is_iroh_peer(peer: &PeerInfo) -> bool {
    !peer.is_cloud
        && peer
            .iroh_addr
            .as_ref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
}

impl SyncTransport {
    /// Create the appropriate transport for a given peer.
    ///
    /// Routing precedence: `is_cloud` → [`SyncTransport::Cloud`]; else if the
    /// peer carries an `iroh_addr` (and the `mesh` feature is enabled) →
    /// [`SyncTransport::P2pIroh`]; otherwise → [`SyncTransport::P2P`] (HTTP).
    ///
    /// This stays synchronous because every backend constructor is synchronous:
    /// `IrohTransport::new` binds its Iroh [`Endpoint`] lazily (on first call)
    /// rather than at construction time. As a result no caller needs to be made
    /// `async` to pick a transport.
    pub fn for_peer(
        peer: &PeerInfo,
        identity: Arc<NodeIdentity>,
        store: Arc<dyn crate::memory::store::MemoryStore>,
    ) -> Result<Self> {
        if peer.is_cloud {
            Ok(Self::Cloud(CloudPeer::new(identity)?))
        } else {
            #[cfg(feature = "mesh")]
            if is_iroh_peer(peer) {
                return Ok(Self::P2pIroh(IrohTransport::new(identity, store)));
            }
            #[cfg(not(feature = "mesh"))]
            let _ = &store;
            Ok(Self::P2P(MeshTransport::new(identity)))
        }
    }

    /// Perform a handshake with the remote peer/mailbox.
    pub async fn handshake(&self, peer_url: &str, token: &str) -> Result<MeshHandshakeResponse> {
        match self {
            Self::P2P(t) => t.handshake(peer_url, token).await,
            Self::Cloud(t) => t.handshake(peer_url, token).await,
            #[cfg(feature = "mesh")]
            Self::P2pIroh(t) => t.handshake(peer_url, token, None).await,
        }
    }

    /// Fetch the sync manifest from the peer.
    pub async fn fetch_manifest(&self, peer: &PeerInfo, token: &str) -> Result<MeshManifest> {
        match self {
            Self::P2P(t) => t.fetch_manifest(peer, token).await,
            Self::Cloud(t) => t.fetch_manifest(peer, token).await,
            #[cfg(feature = "mesh")]
            Self::P2pIroh(t) => t.fetch_manifest(peer, token).await,
        }
    }

    /// Fetch specific chunks from the peer.
    pub async fn fetch_chunks(
        &self,
        peer: &PeerInfo,
        token: &str,
        hashes: &[String],
    ) -> Result<HashMap<String, Vec<u8>>> {
        match self {
            Self::P2P(t) => t.fetch_chunks(peer, token, hashes).await,
            Self::Cloud(t) => t.fetch_chunks(peer, token, hashes).await,
            #[cfg(feature = "mesh")]
            Self::P2pIroh(t) => t.fetch_chunks(peer, token, hashes).await,
        }
    }

    /// Push chunks to the remote peer/mailbox.
    pub async fn push_chunks(
        &self,
        peer: &PeerInfo,
        token: &str,
        chunks: &[(String, Vec<u8>)],
    ) -> Result<Vec<String>> {
        match self {
            Self::P2P(t) => t.push_chunks(peer, token, chunks).await,
            Self::Cloud(t) => t.push_chunks(peer, token, chunks).await,
            #[cfg(feature = "mesh")]
            Self::P2pIroh(t) => t.push_chunks(peer, token, chunks).await,
        }
    }

    /// Publish the local manifest to the peer/mailbox.
    pub async fn publish_manifest(&self, manifest: &MeshManifest) -> Result<()> {
        match self {
            Self::P2P(_t) => Ok(()), // P2P serves manifest on request
            Self::Cloud(t) => t.publish_manifest(manifest).await,
            #[cfg(feature = "mesh")]
            // Iroh peers likewise serve their manifest on request — no push step.
            Self::P2pIroh(_) => Ok(()),
        }
    }

    /// Share a session bundle with a remote peer.
    pub async fn share_session(
        &self,
        peer: &PeerInfo,
        token: &str,
        bundle: SessionBundle,
    ) -> Result<()> {
        match self {
            Self::P2P(t) => t.share_session(peer, token, bundle).await,
            Self::Cloud(t) => t.share_session(peer, token, bundle).await,
            #[cfg(feature = "mesh")]
            Self::P2pIroh(t) => t.share_session(peer, token, bundle).await,
        }
    }

    /// Calculate reconnect backoff duration for P2P sync retry attempts.
    pub fn reconnect_backoff(
        attempt: u32,
        base: Duration,
        max: Duration,
        jitter_factor: f64,
    ) -> Duration {
        calculate_reconnect_backoff(attempt, base, max, jitter_factor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::node::NodeId;

    /// Minimal in-memory store for transport routing tests.
    fn dummy_store() -> Arc<dyn crate::memory::store::MemoryStore> {
        use crate::memory::store::*;
        use std::sync::Mutex;

        struct DummyStore;
        #[async_trait::async_trait]
        impl MemoryStore for DummyStore {
            fn backend(&self) -> MemoryBackend {
                MemoryBackend::Memory
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            async fn health(&self) -> std::result::Result<String, anyhow::Error> {
                Ok("ok".into())
            }
            async fn put(&self, _: MemoryRecord) -> std::result::Result<(), anyhow::Error> {
                Ok(())
            }
            async fn get(
                &self,
                _: &str,
                _: &str,
            ) -> std::result::Result<Option<MemoryRecord>, anyhow::Error> {
                Ok(None)
            }
            async fn update(&self, _: MemoryRecord) -> std::result::Result<(), anyhow::Error> {
                Ok(())
            }
            async fn delete(
                &self,
                _: &str,
                _: &str,
            ) -> std::result::Result<Option<MemoryRecord>, anyhow::Error> {
                Ok(None)
            }
            async fn list(&self, _: &str) -> std::result::Result<Vec<MemoryRecord>, anyhow::Error> {
                Ok(Vec::new())
            }
            async fn list_workspaces(&self) -> std::result::Result<Vec<String>, anyhow::Error> {
                Ok(Vec::new())
            }
            async fn search(
                &self,
                _: &str,
                _: &str,
                _: Option<&MemoryQueryFilters>,
            ) -> std::result::Result<Vec<MemoryRecord>, anyhow::Error> {
                Ok(Vec::new())
            }
            async fn load_workspace_state(
                &self,
                _: &str,
            ) -> std::result::Result<DurableWorkspaceState, anyhow::Error> {
                anyhow::bail!("not implemented")
            }
            async fn save_beliefs(
                &self,
                _: &str,
                _: Vec<crate::domain::memory::belief::BeliefEdge>,
            ) -> std::result::Result<(), anyhow::Error> {
                Ok(())
            }
            async fn save_session_token(
                &self,
                _: &str,
                _: SessionTokenRecord,
            ) -> std::result::Result<(), anyhow::Error> {
                Ok(())
            }
            async fn is_session_token_valid(
                &self,
                _: &str,
                _: &str,
            ) -> std::result::Result<bool, anyhow::Error> {
                Ok(false)
            }
            async fn save_checkpoint(
                &self,
                _: &str,
                _: crate::checkpoint::Checkpoint,
            ) -> std::result::Result<(), anyhow::Error> {
                Ok(())
            }
            async fn load_checkpoint(
                &self,
                _: &str,
                _: &str,
                _: &str,
            ) -> std::result::Result<Option<crate::checkpoint::Checkpoint>, anyhow::Error>
            {
                Ok(None)
            }
            async fn list_checkpoints(
                &self,
                _: &str,
                _: &str,
            ) -> std::result::Result<Vec<crate::checkpoint::Checkpoint>, anyhow::Error>
            {
                Ok(Vec::new())
            }
            async fn delete_checkpoint(
                &self,
                _: &str,
                _: &str,
                _: &str,
            ) -> std::result::Result<(), anyhow::Error> {
                Ok(())
            }
        }
        Arc::new(DummyStore)
    }

    /// Build a minimal `PeerInfo` for routing tests.
    fn test_peer(is_cloud: bool, iroh_addr: Option<&str>) -> PeerInfo {
        PeerInfo {
            node_id: NodeId("xv1-test".into()),
            alias: None,
            endpoint_url: "http://localhost:8006".into(),
            public_key_hex: "deadbeef".into(),
            added_at: 0,
            last_seen_at: None,
            sync_enabled: true,
            is_cloud,
            iroh_addr: iroh_addr.map(String::from),
            shared_workspace_ids: Vec::new(),
            shared_workspace_tokens: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn is_iroh_peer_true_when_addr_present() {
        // A non-cloud peer with an iroh_addr routes to the Iroh transport.
        assert!(is_iroh_peer(&test_peer(false, Some("abcdef"))));
    }

    #[test]
    fn is_iroh_peer_false_when_missing() {
        // No iroh_addr → falls back to plain HTTP (P2P).
        assert!(!is_iroh_peer(&test_peer(false, None)));
    }

    #[test]
    fn is_iroh_peer_false_when_blank() {
        // A whitespace-only address is treated as absent.
        assert!(!is_iroh_peer(&test_peer(false, Some("   "))));
    }

    #[test]
    fn is_iroh_peer_false_for_cloud() {
        // Cloud peers always route to Cloud, even if they (oddly) carry an iroh addr.
        assert!(!is_iroh_peer(&test_peer(true, Some("abcdef"))));
    }

    #[test]
    fn cloud_peer_routes_to_cloud() {
        let peer = test_peer(true, Some("abcdef"));
        assert!(!is_iroh_peer(&peer), "cloud peer must not be an iroh peer");

        let identity = Arc::new(NodeIdentity::generate());
        let store = dummy_store();
        let res = SyncTransport::for_peer(&peer, identity, store);
        let took_cloud = match res {
            Err(e) => e.to_string().contains("PGHEART"),
            Ok(SyncTransport::Cloud(_)) => true,
            Ok(SyncTransport::P2P(_)) => false,
            #[cfg(feature = "mesh")]
            Ok(SyncTransport::P2pIroh(_)) => false,
        };
        assert!(took_cloud, "cloud peer did not route to the Cloud branch");
    }

    #[test]
    fn plain_peer_routes_to_p2p() {
        let peer = test_peer(false, None);
        let identity = Arc::new(NodeIdentity::generate());
        let store = dummy_store();
        assert!(matches!(
            SyncTransport::for_peer(&peer, identity, store).unwrap(),
            SyncTransport::P2P(_)
        ));
    }

    #[test]
    fn iroh_peer_routes_to_p2piroh_under_mesh() {
        let peer = test_peer(false, Some("abcdef"));
        let identity = Arc::new(NodeIdentity::generate());
        let store = dummy_store();
        let transport = SyncTransport::for_peer(&peer, identity, store).unwrap();
        #[cfg(feature = "mesh")]
        {
            assert!(matches!(transport, SyncTransport::P2pIroh(_)));
        }
        #[cfg(not(feature = "mesh"))]
        {
            assert!(matches!(transport, SyncTransport::P2P(_)));
        }
    }

    #[test]
    fn test_mesh_transport_reconnect_backoff_jitter() {
        let base = Duration::from_millis(100);
        let max = Duration::from_millis(1000);

        // 1. Zero jitter: deterministic exponential progression
        let b0 = calculate_reconnect_backoff(0, base, max, 0.0);
        let b1 = calculate_reconnect_backoff(1, base, max, 0.0);
        let b2 = calculate_reconnect_backoff(2, base, max, 0.0);
        let b_high = calculate_reconnect_backoff(10, base, max, 0.0);

        assert_eq!(b0, Duration::from_millis(100));
        assert_eq!(b1, Duration::from_millis(200));
        assert_eq!(b2, Duration::from_millis(400));
        assert_eq!(b_high, max);

        // 2. Full jitter (factor 0.5): samples within range [exp * 0.5, exp] and non-static
        let attempt = 3; // exp_ms = 800ms, range = [400ms, 800ms]
        let mut samples = Vec::new();

        for _ in 0..50 {
            let backoff = calculate_reconnect_backoff(attempt, base, max, 0.5);
            assert!(backoff >= Duration::from_millis(400));
            assert!(backoff <= Duration::from_millis(800));
            assert!(backoff <= max);
            samples.push(backoff.as_millis());
        }

        // Verify randomization jitter produced distinct values
        let first = samples[0];
        let has_variation = samples.iter().any(|&val| val != first);
        assert!(
            has_variation,
            "randomized jitter should generate varying backoff durations"
        );

        // 3. ReconnectBackoffConfig & SyncTransport helper methods
        let config = ReconnectBackoffConfig {
            base,
            max,
            jitter_factor: 0.2,
        };
        let cfg_backoff = config.calculate_backoff(1);
        assert!(cfg_backoff >= Duration::from_millis(160));
        assert!(cfg_backoff <= Duration::from_millis(200));

        let transport_backoff = SyncTransport::reconnect_backoff(2, base, max, 0.0);
        assert_eq!(transport_backoff, Duration::from_millis(400));
    }
}
