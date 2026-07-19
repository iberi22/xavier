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

use crate::mesh::cloud_node::CloudPeer;
#[cfg(feature = "mesh")]
use crate::mesh::iroh_transport::IrohTransport;
use crate::mesh::node::NodeIdentity;
use crate::mesh::peer::PeerInfo;
use crate::mesh::protocol::{MeshHandshakeResponse, MeshManifest};
use crate::mesh::transport::MeshTransport;
use crate::session::sharing::SessionBundle;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;

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
    pub fn for_peer(peer: &PeerInfo, identity: Arc<NodeIdentity>) -> Result<Self> {
        if peer.is_cloud {
            Ok(Self::Cloud(CloudPeer::new(identity)?))
        } else {
            #[cfg(feature = "mesh")]
            if is_iroh_peer(peer) {
                return Ok(Self::P2pIroh(IrohTransport::new(identity)));
            }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::node::NodeId;

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
        // A cloud peer must select the Cloud transport regardless of iroh_addr.
        // We assert the routing *decision* (not construction): is_iroh_peer is
        // false for cloud, and for_peer enters the Cloud branch. In a test
        // environment without XAVIER_PGHEART_URL that branch surfaces a
        // recognizable error, which distinguishes it from the P2P branch (which
        // would construct successfully). This keeps the test network-free.
        let peer = test_peer(true, Some("abcdef"));
        assert!(!is_iroh_peer(&peer), "cloud peer must not be an iroh peer");

        let identity = Arc::new(NodeIdentity::generate());
        let res = SyncTransport::for_peer(&peer, identity);
        // Cloud construction needs the PGHEART url; without it we get an Err that
        // mentions that env var — proving we took the Cloud branch rather than the
        // infallible P2P branch. With the env set, we expect the Cloud arm.
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
        // A non-cloud peer with no iroh_addr selects the HTTP P2P transport.
        let peer = test_peer(false, None);
        let identity = Arc::new(NodeIdentity::generate());
        assert!(matches!(
            SyncTransport::for_peer(&peer, identity).unwrap(),
            SyncTransport::P2P(_)
        ));
    }

    #[test]
    fn iroh_peer_routes_to_p2piroh_under_mesh() {
        // Under the `mesh` feature, a peer with an iroh_addr selects P2pIroh;
        // without `mesh` it falls back to P2P (the variant simply doesn't exist).
        let peer = test_peer(false, Some("abcdef"));
        let identity = Arc::new(NodeIdentity::generate());
        let transport = SyncTransport::for_peer(&peer, identity).unwrap();
        #[cfg(feature = "mesh")]
        {
            assert!(matches!(transport, SyncTransport::P2pIroh(_)));
        }
        #[cfg(not(feature = "mesh"))]
        {
            assert!(matches!(transport, SyncTransport::P2P(_)));
        }
    }
}
