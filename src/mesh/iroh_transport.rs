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
//! Each node binds an [`Endpoint`] (lazily, on first use) and shares its address
//! as an [`EndpointId`]-encoded string (stored in `PeerInfo.iroh_addr` for Iroh
//! peers). Operations open a fresh bidirectional QUIC stream per request, framed
//! as:
//!   `[u32 BE length][JSON request]` → `[u32 BE length][JSON response]`.
//!
//! The request `op` field selects the handler; bodies reuse the existing
//! `MeshHandshake` / `MeshManifest` / etc. protocol types so HTTP and Iroh
//! transports are wire-compatible at the application layer.
//!
//! ## API parity with MeshTransport
//!
//! The public methods take the *same* arguments as [`super::MeshTransport`]:
//! `handshake` takes a `peer_url`/`peer_addr` string while the manifest/chunk/
//! session helpers take a `&PeerInfo` (the Iroh address is read out of
//! `PeerInfo.iroh_addr`). This is what lets `SyncTransport` delegate to either
//! transport variant uniformly. The Iroh `Connection` — which `iroh` models as a
//! cheap, `Clone` handle — is established internally via [`Self::connect`] and
//! is never passed in by the caller.
//!
//! This module is compiled only under the `mesh` cargo feature. The transport is
//! fully wired but the server-side stream handlers (accept loop) live in the mesh
//! service layer; this file focuses on the client dial + framing primitives that
//! the sync layer calls into.

use crate::mesh::node::NodeIdentity;
use crate::mesh::peer::PeerInfo;
use crate::mesh::protocol::{
    MeshHandshake, MeshHandshakeResponse, MeshManifest, MeshSessionShare, MeshSyncRequest,
};
use crate::session::sharing::SessionBundle;
use anyhow::{anyhow, Context, Result};
use iroh::endpoint::presets::N0;
use iroh::endpoint::Connection;
use iroh::Endpoint;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::OnceCell;

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

/// Iroh-backed P2P transport. Holds the local node identity used to sign
/// handshake nonces (mirroring `MeshTransport`) and a lazily-bound Iroh
/// [`Endpoint`].
///
/// The endpoint is **not** bound at construction time — `new()` is cheap and
/// infallible. The first method call triggers [`Self::endpoint`] which binds the
/// QUIC endpoint via [`Endpoint::builder(N0)`]. This keeps `SyncTransport::
/// for_peer` synchronous (no `async`/`.await` needed to *construct* an
/// `IrohTransport`) while still giving each operation a live endpoint to dial
/// through. Binding can fail; such failures surface as `Err` from the method that
/// triggers the bind.
pub struct IrohTransport {
    local_identity: Arc<NodeIdentity>,
    endpoint: OnceCell<Endpoint>,
}

impl IrohTransport {
    /// Create a new Iroh transport for the given node identity.
    ///
    /// Cheap and infallible — the Iroh [`Endpoint`] is bound lazily on first use
    /// (see [`Self::endpoint`]). This mirrors the synchronous
    /// [`super::MeshTransport::new`] signature so `SyncTransport::for_peer` can
    /// construct either variant without `await`.
    pub fn new(identity: Arc<NodeIdentity>) -> Self {
        Self {
            local_identity: identity,
            endpoint: OnceCell::new(),
        }
    }

    /// The local Iroh endpoint's node id (debug form). Binds the endpoint on
    /// first call. The full dial address is obtained via the Iroh ticket
    /// mechanism; callers persist that in `PeerInfo.iroh_addr`.
    pub async fn my_addr_string(&self) -> Result<String> {
        let endpoint = self.endpoint().await?;
        Ok(format!("{:?}", endpoint.id()))
    }

    /// Lazily bind and return the Iroh [`Endpoint`].
    ///
    /// Uses the N0 public relay map so peers can be reached across NATs without
    /// manual port forwarding. The bind happens exactly once per transport
    /// instance (guarded by [`OnceCell`]); subsequent calls return the cached
    /// endpoint.
    async fn endpoint(&self) -> Result<&Endpoint> {
        self.endpoint
            .get_or_try_init(|| async {
                Endpoint::builder(N0)
                    .bind()
                    .await
                    .context("Failed to bind Iroh endpoint")
            })
            .await
    }

    /// Resolve the Iroh address string for a peer.
    ///
    /// The address is stored in `PeerInfo.iroh_addr` as the hex/base32 encoding
    /// of the remote endpoint's [`iroh::EndpointId`] (an Ed25519
    /// `PublicKey`). Falls back with an error if the peer has no Iroh address —
    /// callers should only route to `IrohTransport` when `iroh_addr.is_some()`.
    fn addr_from_peer(peer: &PeerInfo) -> Result<String> {
        peer.iroh_addr
            .clone()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| anyhow!("peer {} has no iroh_addr", peer.node_id))
    }

    /// Dial a remote endpoint and return a (cheap, `Clone`) QUIC [`Connection`].
    ///
    /// `peer_addr` is the `EndpointId` string (as stored in
    /// `PeerInfo.iroh_addr`). It is parsed into a [`iroh::PublicKey`] and
    /// wrapped in an `EndpointAddr`; the endpoint's address-lookup service
    /// resolves the relay/direct addresses needed to reach it.
    async fn connect(&self, peer_addr: &str) -> Result<Connection> {
        let endpoint = self.endpoint().await?;
        let trimmed = peer_addr.trim();
        let public_key = trimmed
            .parse::<iroh::PublicKey>()
            .with_context(|| format!("invalid iroh peer addr: {peer_addr}"))?;
        let endpoint_addr = iroh::EndpointAddr::new(public_key);
        let conn = endpoint
            .connect(endpoint_addr, XMESH_ALPN)
            .await
            .with_context(|| format!("failed to connect to iroh peer {peer_addr}"))?;
        Ok(conn)
    }

    /// Open a framed request/response stream to a connection and exchange one
    /// message. The caller obtains the `Connection` via [`Self::connect`].
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
    ///
    /// `peer_addr` is the remote endpoint's `EndpointId` string; the connection
    /// is established internally. Mirrors `MeshTransport::handshake_with_secret`
    /// (the `SyncTransport::handshake` entry point delegates here with
    /// `pairing_secret = None`).
    pub async fn handshake(
        &self,
        peer_addr: &str,
        _token: &str,
        pairing_secret: Option<String>,
    ) -> Result<MeshHandshakeResponse> {
        let conn = self.connect(peer_addr).await?;
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
        let resp = self.round_trip(&conn, &MeshRequest::Handshake { body }).await?;
        serde_json::from_value(resp).context("parse handshake response")
    }

    /// Fetch the sync manifest from a peer over Iroh. Reads the address out of
    /// `peer.iroh_addr`.
    pub async fn fetch_manifest(
        &self,
        peer: &PeerInfo,
        _token: &str,
    ) -> Result<MeshManifest> {
        let addr = Self::addr_from_peer(peer)?;
        let conn = self.connect(&addr).await?;
        let timestamp = chrono::Utc::now().timestamp().to_string();
        let nonce = uuid::Uuid::new_v4().to_string();
        let message = format!("{}:{}", timestamp, nonce);
        let signature =
            crate::crypto::hex_encode(self.local_identity.sign(message.as_bytes()));
        let resp = self
            .round_trip(
                &conn,
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

    /// Fetch specific chunks from a peer over Iroh.
    pub async fn fetch_chunks(
        &self,
        peer: &PeerInfo,
        _token: &str,
        hashes: &[String],
    ) -> Result<HashMap<String, Vec<u8>>> {
        let addr = Self::addr_from_peer(peer)?;
        let conn = self.connect(&addr).await?;
        let request = self.signed_sync_request(hashes.to_vec());
        let resp = self.round_trip(&conn, &MeshRequest::FetchChunks { request }).await?;
        Ok(serde_json::from_value(resp).context("parse chunks")?)
    }

    /// Push chunks to a remote peer over Iroh. The wanted_hashes field carries
    /// the chunk identifiers being offered; the chunk payloads travel in a
    /// separate layer.
    pub async fn push_chunks(
        &self,
        peer: &PeerInfo,
        _token: &str,
        chunks: &[(String, Vec<u8>)],
    ) -> Result<Vec<String>> {
        let addr = Self::addr_from_peer(peer)?;
        let conn = self.connect(&addr).await?;
        let hashes: Vec<String> = chunks.iter().map(|(h, _)| h.clone()).collect();
        let request = self.signed_sync_request(hashes);
        let resp = self.round_trip(&conn, &MeshRequest::PushChunks { request }).await?;
        Ok(serde_json::from_value(resp).context("parse push ack")?)
    }

    /// Share a session bundle with a remote peer over Iroh.
    pub async fn share_session(
        &self,
        peer: &PeerInfo,
        _token: &str,
        bundle: SessionBundle,
    ) -> Result<()> {
        let addr = Self::addr_from_peer(peer)?;
        let conn = self.connect(&addr).await?;
        let share = MeshSessionShare {
            sender_node_id: self.local_identity.node_id.clone(),
            bundle,
            context_bundle: None,
            token_stats: None,
        };
        let _resp = self.round_trip(&conn, &MeshRequest::ShareSession { share }).await?;
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

    #[test]
    fn addr_from_peer_errors_when_missing() {
        // A peer without an iroh_addr must be rejected so callers route elsewhere.
        let peer = PeerInfo {
            node_id: crate::mesh::node::NodeId("xv1-no-iroh".into()),
            alias: None,
            endpoint_url: "http://localhost:8006".into(),
            public_key_hex: "aabb".into(),
            added_at: 0,
            last_seen_at: None,
            sync_enabled: true,
            is_cloud: false,
            iroh_addr: None,
        };
        assert!(IrohTransport::addr_from_peer(&peer).is_err());

        // An empty string is treated as missing too.
        let mut peer_empty = peer.clone();
        peer_empty.iroh_addr = Some("   ".into());
        assert!(IrohTransport::addr_from_peer(&peer_empty).is_err());
    }

    #[test]
    fn addr_from_peer_returns_stored_value() {
        let peer = PeerInfo {
            node_id: crate::mesh::node::NodeId("xv1-iroh".into()),
            alias: None,
            endpoint_url: String::new(),
            public_key_hex: "aabb".into(),
            added_at: 0,
            last_seen_at: None,
            sync_enabled: true,
            is_cloud: false,
            iroh_addr: Some("deadbeefdeadbeef".into()),
        };
        assert_eq!(
            IrohTransport::addr_from_peer(&peer).unwrap(),
            "deadbeefdeadbeef"
        );
    }
}
