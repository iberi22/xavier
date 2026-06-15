//! Unified Transport — Abstraction for P2P and Cloud mesh sync
//!
//! Routes sync requests to either MeshTransport (P2P HTTP) or CloudPeer (Supabase REST)
//! based on the target peer's configuration.

use crate::mesh::node::NodeIdentity;
use crate::mesh::peer::PeerInfo;
use crate::mesh::protocol::{MeshHandshakeResponse, MeshManifest};
use crate::mesh::transport::MeshTransport;
use crate::mesh::cloud_node::CloudPeer;
use crate::session::sharing::SessionBundle;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;

pub enum SyncTransport {
    P2P(MeshTransport),
    Cloud(CloudPeer),
}

impl SyncTransport {
    /// Create the appropriate transport for a given peer.
    pub fn for_peer(peer: &PeerInfo, identity: Arc<NodeIdentity>) -> Result<Self> {
        if peer.is_cloud {
            Ok(Self::Cloud(CloudPeer::new(identity)?))
        } else {
            Ok(Self::P2P(MeshTransport::new(identity)))
        }
    }

    /// Perform a handshake with the remote peer/mailbox.
    pub async fn handshake(&self, peer_url: &str, token: &str) -> Result<MeshHandshakeResponse> {
        match self {
            Self::P2P(t) => t.handshake(peer_url, token).await,
            Self::Cloud(t) => t.handshake(peer_url, token).await,
        }
    }

    /// Fetch the sync manifest from the peer.
    pub async fn fetch_manifest(&self, peer: &PeerInfo, token: &str) -> Result<MeshManifest> {
        match self {
            Self::P2P(t) => t.fetch_manifest(peer, token).await,
            Self::Cloud(t) => t.fetch_manifest(peer, token).await,
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
        }
    }

    /// Publish the local manifest to the peer/mailbox.
    pub async fn publish_manifest(&self, manifest: &MeshManifest) -> Result<()> {
        match self {
            Self::P2P(_t) => Ok(()), // P2P serves manifest on request
            Self::Cloud(t) => t.publish_manifest(manifest).await,
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
        }
    }
}
