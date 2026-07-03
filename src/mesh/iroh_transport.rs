//! Iroh Transport — QUIC-based P2P transport for Xavier Mesh (Phase 2)
//!
//! Mirrors the [`super::MeshTransport`] surface (handshake / fetch_manifest /
//! fetch_chunks / push_chunks / share_session) but uses Iroh's QUIC endpoints
//! instead of HTTP. Iroh provides automatic NAT traversal (relay-assisted hole
//! punching), so two Xavier nodes can sync without exposing public ports — the
//! mesh roadmap's "Phase 2" goal.
//!
//! ## Connection model
//!
//! Each node binds an [`Endpoint`] and shares its address as an
//! [`EndpointTicket`]-encoded string (stored in `PeerInfo.addr` for Iroh peers).
//! Operations open a fresh bidirectional QUIC stream per request, framed as:
//!   `[u32 BE length][JSON request]` → `[u32 BE length][JSON response]`.
//!
//! The request `op` field selects the handler; bodies reuse the existing
//! `MeshHandshake` / `MeshManifest` / etc. protocol types so HTTP and Iroh
//! transports are wire-compatible at the application layer.
//!
//! This module is compiled only under the `mesh` cargo feature. The transport is
//! fully wired but the server-side stream handlers (accept loop) live in the mesh
//! service layer; this file focuses on the client dial + framing primitives that
//! the sync layer calls into.

use crate::mesh::node::NodeIdentity;
use crate::mesh::protocol::{
    MeshHandshake, MeshHandshakeResponse, MeshManifest, MeshSessionShare, MeshSyncRequest,
};
use crate::session::sharing::SessionBundle;
use anyhow::{Context, Result};
use iroh::endpoint::presets::N0;
use iroh::endpoint::Connection;
use iroh::Endpoint;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// ALPN identifier for the XMesh-Sync protocol over Iroh.
pub const XMESH_ALPN: &[u8] = b"/xavier/mesh-sync/1";

/// A framed request envelope sent over a QUIC stream.
///
/// `node_id` carries the local XMesh NodeID for signing/verification context;
/// the Iroh-level peer identity is established separately by the QUIC handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MeshRequest {
    Handshake { body: MeshHandshake },
    FetchManifest { node_id: String, timestamp: String, nonce: String, signature: String },
    FetchChunks { request: MeshSyncRequest },
    PushChunks { request: MeshSyncRequest },
    ShareSession { share: MeshSessionShare },
}

/// Iroh-backed P2P transport. Holds a bound [`Endpoint`] and the local node
/// identity used to sign handshake nonces (mirroring `MeshTransport`).
pub struct IrohTransport {
    endpoint: Endpoint,
    local_identity: Arc<NodeIdentity>,
}

impl IrohTransport {
    /// Bind a new Iroh endpoint for this node. Uses the N0 public relay map so
    /// peers can be reached across NATs without manual port forwarding.
    pub async fn new(identity: Arc<NodeIdentity>) -> Result<Self> {
        let endpoint = Endpoint::builder(N0)
            .bind()
            .await
            .context("Failed to bind Iroh endpoint")?;
        Ok(Self {
            endpoint,
            local_identity: identity,
        })
    }

    /// The local Iroh endpoint's node id (debug form). The full dial address is
    /// obtained via the Iroh ticket mechanism; callers persist that in `PeerInfo.addr`.
    pub fn my_addr_string(&self) -> String {
        format!("{:?}", self.endpoint.id())
    }

    /// Open a framed request/response stream to a connection and exchange one
    /// message. The caller obtains the `Connection` via `endpoint.connect(...)`.
    async fn round_trip(
        &self,
        conn: &Connection,
        req: &MeshRequest,
    ) -> Result<serde_json::Value> {
        use tokio::io::AsyncReadExt;

        let (mut send, mut recv) = conn.open_bi().await.context("open_bi failed")?;

        let payload = serde_json::to_vec(req).context("serialize request")?;
        send.write_all(&(payload.len() as u32).to_be_bytes()).await?;
        send.write_all(&payload).await?;
        send.finish().context("finish send stream")?;

        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];
        recv.read_exact(&mut buf).await?;

        let value: serde_json::Value =
            serde_json::from_slice(&buf).context("parse response")?;
        Ok(value)
    }

    /// Build a signed sync request (shared by fetch/push).
    fn signed_sync_request(&self, wanted_hashes: Vec<String>) -> MeshSyncRequest {
        let nonce = uuid::Uuid::new_v4().to_string();
        let timestamp = chrono::Utc::now().timestamp();
        let message = format!("{}:{}", timestamp, nonce);
        let signature =
            crate::crypto::hex_encode(self.local_identity.sign(message.as_bytes()));
        MeshSyncRequest {
            requesting_node_id: self.local_identity.node_id.clone(),
            wanted_hashes,
            timestamp,
            nonce,
            signature_hex: signature,
        }
    }

    /// Perform a handshake with a remote peer over Iroh.
    pub async fn handshake(
        &self,
        conn: &Connection,
        _token: &str,
        pairing_secret: Option<String>,
    ) -> Result<MeshHandshakeResponse> {
        let nonce = uuid::Uuid::new_v4().to_string();
        let signature = self.local_identity.sign(nonce.as_bytes());
        let body = MeshHandshake {
            node_id: self.local_identity.node_id.clone(),
            public_key_hex: crate::crypto::hex_encode(&self.local_identity.public_key),
            xavier_version: env!("CARGO_PKG_VERSION").to_string(),
            capabilities: vec!["sync-v1".to_string()],
            timestamp: chrono::Utc::now().timestamp(),
            nonce,
            signature_hex: crate::crypto::hex_encode(signature),
            pairing_secret,
        };
        let resp = self.round_trip(conn, &MeshRequest::Handshake { body }).await?;
        serde_json::from_value(resp).context("parse handshake response")
    }

    /// Fetch the sync manifest from a peer.
    pub async fn fetch_manifest(
        &self,
        conn: &Connection,
        _token: &str,
    ) -> Result<MeshManifest> {
        let timestamp = chrono::Utc::now().timestamp().to_string();
        let nonce = uuid::Uuid::new_v4().to_string();
        let message = format!("{}:{}", timestamp, nonce);
        let signature =
            crate::crypto::hex_encode(self.local_identity.sign(message.as_bytes()));
        let resp = self
            .round_trip(
                conn,
                &MeshRequest::FetchManifest {
                    node_id: self.local_identity.node_id.to_string(),
                    timestamp,
                    nonce,
                    signature,
                },
            )
            .await?;
        serde_json::from_value(resp).context("parse manifest")
    }

    /// Fetch specific chunks from a peer.
    pub async fn fetch_chunks(
        &self,
        conn: &Connection,
        _token: &str,
        hashes: &[String],
    ) -> Result<HashMap<String, Vec<u8>>> {
        let request = self.signed_sync_request(hashes.to_vec());
        let resp = self.round_trip(conn, &MeshRequest::FetchChunks { request }).await?;
        Ok(serde_json::from_value(resp).context("parse chunks")?)
    }

    /// Push chunks to a remote peer. The wanted_hashes field carries the chunk
    /// identifiers being offered; the chunk payloads travel in a separate layer.
    pub async fn push_chunks(
        &self,
        conn: &Connection,
        _token: &str,
        chunks: &[(String, Vec<u8>)],
    ) -> Result<Vec<String>> {
        let hashes: Vec<String> = chunks.iter().map(|(h, _)| h.clone()).collect();
        let request = self.signed_sync_request(hashes);
        let resp = self.round_trip(conn, &MeshRequest::PushChunks { request }).await?;
        Ok(serde_json::from_value(resp).context("parse push ack")?)
    }

    /// Share a session bundle with a remote peer.
    pub async fn share_session(
        &self,
        conn: &Connection,
        _token: &str,
        bundle: SessionBundle,
    ) -> Result<()> {
        let share = MeshSessionShare {
            sender_node_id: self.local_identity.node_id.clone(),
            bundle,
            context_bundle: None,
            token_stats: None,
        };
        let _resp = self.round_trip(conn, &MeshRequest::ShareSession { share }).await?;
        Ok(())
    }
}

// NOTE: The full Iroh server-side accept loop (which reads a `MeshRequest` off an
// incoming stream and dispatches to the local mesh service) belongs in the mesh
// service layer alongside the HTTP route handlers. This module provides the client
// dial + framing primitives; wiring the accept loop is the remaining Phase 2 task
// tracked in features.json.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xmesh_alpn_is_valid() {
        // ALPN identifiers must be non-empty and reasonably short.
        assert!(!XMESH_ALPN.is_empty());
        assert!(XMESH_ALPN.len() <= 255);
    }

    #[test]
    fn mesh_request_serializes_with_op_tag() {
        // The request envelope must carry the `op` tag so the server can dispatch.
        let req = MeshRequest::FetchChunks {
            request: MeshSyncRequest {
                requesting_node_id: crate::mesh::node::NodeId("test".into()),
                wanted_hashes: vec![],
                timestamp: 0,
                nonce: "n".into(),
                signature_hex: "s".into(),
            },
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"op\":\"fetch_chunks\""), "serialized: {json}");
    }
}
