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

use crate::memory::store::MemoryStore;
use crate::mesh::node::NodeIdentity;
use crate::mesh::peer::PeerInfo;
use crate::mesh::private_mesh::{
    decrypt_session_payload, encrypt_session_payload, signing_key_to_x25519_secret,
    EncryptedSessionPayload, PrivateSyncPayload,
};
use crate::mesh::protocol::{
    ChunkRef, MeshHandshake, MeshHandshakeResponse, MeshManifest, MeshSessionShare, MeshSyncRequest,
};
use crate::session::sharing::SessionBundle;
use anyhow::{anyhow, Context, Result};
use iroh::endpoint::presets::{Minimal, N0};
use iroh::endpoint::Connection;
use iroh::Endpoint;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::OnceCell;

/// ALPN identifier for the XMesh-Sync protocol over Iroh.
pub const XMESH_ALPN: &[u8] = b"/xavier/mesh-sync/1";

/// Default timeout for handshake operations (connect + round-trip).
pub const HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// A framed request envelope sent over a QUIC stream.
///
/// `node_id` carries the local XMesh NodeID for signing/verification context;
/// the Iroh-level peer identity is established separately by the QUIC handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MeshRequest {
    Handshake {
        body: MeshHandshake,
    },
    FetchManifest {
        node_id: String,
        timestamp: String,
        nonce: String,
        signature: String,
    },
    FetchChunks {
        request: MeshSyncRequest,
    },
    PushChunks {
        request: MeshSyncRequest,
    },
    ShareSession {
        share: MeshSessionShare,
    },
    PrivateSync {
        wallet_id: String,
        encrypted: EncryptedSessionPayload,
    },
    /// Sealed memory sync: payload encrypted to the receiver, signed by the sender.
    MemorySync {
        sender_public_key_hex: String,
        timestamp: i64,
        nonce: String,
        signature_hex: String,
        encrypted: EncryptedSessionPayload,
    },
}

/// Result of a sealed memory sync, returned by the receiving node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySyncAck {
    pub accepted: bool,
    pub applied: usize,
    pub reason: Option<String>,
}

/// Maximum clock skew tolerated for a signed `MemorySync` request.
const MEMORY_SYNC_MAX_SKEW_SECS: i64 = 300;
/// Workspace that receives synced memories lacking a `workspace_id` in metadata.
const MESH_SYNC_DEFAULT_WORKSPACE: &str = "mesh-sync";

/// Which peers the accept loop serves. `None` means no allowlist is enforced
/// (handshakes only); sealed memory sync always requires an allowlisted sender.
type PeerPolicy = Option<Arc<HashSet<String>>>;

fn memory_sync_message(
    timestamp: i64,
    nonce: &str,
    encrypted: &EncryptedSessionPayload,
) -> Vec<u8> {
    format!(
        "memory-sync:{timestamp}:{nonce}:{}:{}:{}",
        encrypted.nonce_hex,
        encrypted.ephemeral_public_key_hex.as_deref().unwrap_or(""),
        encrypted.ciphertext_hex
    )
    .into_bytes()
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
    store: Arc<dyn MemoryStore>,
    /// Localhost-only endpoint: no relays, no address lookup (tests, LAN).
    direct_only: bool,
    allowed_peers: PeerPolicy,
}

impl IrohTransport {
    /// Create a new Iroh transport for the given node identity.
    ///
    /// Cheap and infallible — the Iroh [`Endpoint`] is bound lazily on first use
    /// (see [`Self::endpoint`]). This mirrors the synchronous
    /// [`super::MeshTransport::new`] signature so `SyncTransport::for_peer` can
    /// construct either variant without `await`.
    pub fn new(identity: Arc<NodeIdentity>, store: Arc<dyn MemoryStore>) -> Self {
        Self {
            local_identity: identity,
            endpoint: OnceCell::new(),
            store,
            direct_only: false,
            allowed_peers: None,
        }
    }

    /// Transport bound to localhost with relays and discovery disabled. Peers
    /// must be dialed by explicit address; only `allowed_peers` are served.
    pub fn new_direct(
        identity: Arc<NodeIdentity>,
        store: Arc<dyn MemoryStore>,
        allowed_peers: impl IntoIterator<Item = crate::mesh::node::NodeId>,
    ) -> Self {
        let allowed: HashSet<String> = allowed_peers.into_iter().map(|n| n.0).collect();
        Self {
            direct_only: true,
            allowed_peers: Some(Arc::new(allowed)),
            ..Self::new(identity, store)
        }
    }

    /// Full dialable address (id + bound direct sockets) of the local endpoint.
    pub async fn direct_addr(&self) -> Result<iroh::EndpointAddr> {
        Ok(self.endpoint().await?.addr())
    }

    /// The local Iroh endpoint's node id (debug form). Binds the endpoint on
    /// first call. The full dial address is obtained via the Iroh ticket
    /// mechanism; callers persist that in `PeerInfo.iroh_addr`.
    pub async fn my_addr_string(&self) -> Result<String> {
        let endpoint = self.endpoint().await?;
        Ok(endpoint.id().to_string())
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
                if self.direct_only {
                    return Endpoint::builder(Minimal)
                        .relay_mode(iroh::RelayMode::Disabled)
                        .clear_address_lookup()
                        .clear_ip_transports()
                        .bind_addr("127.0.0.1:0")
                        .map_err(|e| anyhow!("invalid bind addr: {e:?}"))?
                        .alpns(vec![XMESH_ALPN.to_vec()])
                        .bind()
                        .await
                        .context("Failed to bind direct Iroh endpoint");
                }
                Endpoint::builder(N0)
                    .alpns(vec![XMESH_ALPN.to_vec()])
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
        let addr = peer
            .iroh_addr
            .clone()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| anyhow!("peer {} has no iroh_addr", peer.node_id))?;
        if addr.chars().any(|c| c.is_control() || c.is_whitespace()) {
            tracing::debug!(
                "Rejected invalid peer iroh_addr containing control/whitespace chars: {addr}"
            );
            anyhow::bail!("invalid iroh peer addr: {addr}");
        }
        Ok(addr)
    }

    /// Dial a remote endpoint and return a (cheap, `Clone`) QUIC [`Connection`].
    ///
    /// `peer_addr` is the `EndpointId` string (as stored in
    /// `PeerInfo.iroh_addr`). It is parsed into a [`iroh::PublicKey`] and
    /// wrapped in an `EndpointAddr`; the endpoint's address-lookup service
    /// resolves the relay/direct addresses needed to reach it.
    pub async fn connect(&self, peer_addr: &str) -> Result<Connection> {
        let trimmed = peer_addr.trim();
        if trimmed.is_empty() || peer_addr.chars().any(|c| c.is_control()) {
            tracing::debug!("Rejected invalid iroh peer addr before IO: {peer_addr}");
            anyhow::bail!("invalid iroh peer addr: {peer_addr}");
        }
        let public_key = trimmed
            .parse::<iroh::PublicKey>()
            .with_context(|| format!("invalid iroh peer addr: {peer_addr}"))?;
        self.connect_addr(iroh::EndpointAddr::new(public_key)).await
    }

    /// Dial a remote endpoint by full address (id + direct sockets).
    pub async fn connect_addr(&self, addr: iroh::EndpointAddr) -> Result<Connection> {
        let endpoint = self.endpoint().await?;
        let id = addr.id;
        endpoint
            .connect(addr, XMESH_ALPN)
            .await
            .with_context(|| format!("failed to connect to iroh peer {id}"))
    }

    /// Open a framed request/response stream to a connection and exchange one
    /// message. The caller obtains the `Connection` via [`Self::connect`].
    async fn round_trip(&self, conn: &Connection, req: &MeshRequest) -> Result<serde_json::Value> {
        use tokio::io::AsyncReadExt;

        let (mut send, mut recv) = conn.open_bi().await.context("open_bi failed")?;

        let payload = serde_json::to_vec(req).context("serialize request")?;
        send.write_all(&(payload.len() as u32).to_be_bytes())
            .await?;
        send.write_all(&payload).await?;
        send.finish().context("finish send stream")?;

        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];
        recv.read_exact(&mut buf).await?;

        let value: serde_json::Value = serde_json::from_slice(&buf).context("parse response")?;
        Ok(value)
    }

    /// Build a signed sync request (shared by fetch/push).
    pub fn signed_sync_request(&self, wanted_hashes: Vec<String>) -> MeshSyncRequest {
        let nonce = uuid::Uuid::new_v4().to_string();
        let timestamp = chrono::Utc::now().timestamp();
        let message = format!("{}:{}", timestamp, nonce);
        let signature = crate::crypto::hex_encode(self.local_identity.sign(message.as_bytes()));
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
        let handshake_fut = async {
            let conn = self.connect(peer_addr).await?;
            self.handshake_on(&conn, pairing_secret).await
        };

        match tokio::time::timeout(HANDSHAKE_TIMEOUT, handshake_fut).await {
            Ok(res) => res,
            Err(_) => {
                tracing::warn!("Handshake timed out for peer {peer_addr}");
                Err(anyhow!("handshake timed out for peer {peer_addr}"))
            }
        }
    }

    /// Handshake with a peer dialed by explicit address (bounded by
    /// [`HANDSHAKE_TIMEOUT`]).
    pub async fn handshake_addr(
        &self,
        addr: iroh::EndpointAddr,
        pairing_secret: Option<String>,
    ) -> Result<MeshHandshakeResponse> {
        let fut = async {
            let conn = self.connect_addr(addr).await?;
            self.handshake_on(&conn, pairing_secret).await
        };
        tokio::time::timeout(HANDSHAKE_TIMEOUT, fut)
            .await
            .map_err(|_| anyhow!("handshake timed out"))?
    }

    async fn handshake_on(
        &self,
        conn: &Connection,
        pairing_secret: Option<String>,
    ) -> Result<MeshHandshakeResponse> {
        {
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
            let resp = self
                .round_trip(conn, &MeshRequest::Handshake { body })
                .await?;
            serde_json::from_value(resp).context("parse handshake response")
        }
    }

    /// Seal `payload` to `recipient_public_key` (Ed25519), sign it with the
    /// local identity and deliver it to the peer at `addr`.
    pub async fn send_memory_sync(
        &self,
        addr: iroh::EndpointAddr,
        recipient_public_key: &[u8],
        payload: &PrivateSyncPayload,
    ) -> Result<MemorySyncAck> {
        let recipient: [u8; 32] = recipient_public_key
            .try_into()
            .map_err(|_| anyhow!("recipient public key must be 32 bytes"))?;
        let encrypted = encrypt_session_payload(payload, &recipient)?;
        let nonce = uuid::Uuid::new_v4().to_string();
        let timestamp = chrono::Utc::now().timestamp();
        let signature = self
            .local_identity
            .sign(&memory_sync_message(timestamp, &nonce, &encrypted));
        let req = MeshRequest::MemorySync {
            sender_public_key_hex: crate::crypto::hex_encode(&self.local_identity.public_key),
            timestamp,
            nonce,
            signature_hex: crate::crypto::hex_encode(signature),
            encrypted,
        };
        let fut = async {
            let conn = self.connect_addr(addr).await?;
            self.round_trip(&conn, &req).await
        };
        let resp = tokio::time::timeout(HANDSHAKE_TIMEOUT, fut)
            .await
            .map_err(|_| anyhow!("memory sync timed out"))??;
        serde_json::from_value(resp).context("parse memory sync ack")
    }

    /// Fetch the sync manifest from a peer over Iroh. Reads the address out of
    /// `peer.iroh_addr`.
    pub async fn fetch_manifest(&self, peer: &PeerInfo, _token: &str) -> Result<MeshManifest> {
        let addr = Self::addr_from_peer(peer)?;
        let conn = self.connect(&addr).await?;
        let timestamp = chrono::Utc::now().timestamp().to_string();
        let nonce = uuid::Uuid::new_v4().to_string();
        let message = format!("{}:{}", timestamp, nonce);
        let signature = crate::crypto::hex_encode(self.local_identity.sign(message.as_bytes()));
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
        let resp = self
            .round_trip(&conn, &MeshRequest::FetchChunks { request })
            .await?;
        let chunks: HashMap<String, Vec<u8>> =
            serde_json::from_value(resp).context("parse chunks")?;
        Ok(chunks)
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
        let resp = self
            .round_trip(&conn, &MeshRequest::PushChunks { request })
            .await?;
        let ack: Vec<String> = serde_json::from_value(resp).context("parse push ack")?;
        Ok(ack)
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
        let _resp = self
            .round_trip(&conn, &MeshRequest::ShareSession { share })
            .await?;
        Ok(())
    }

    /// Perform a private sync payload transfer over Iroh QUIC with session encryption.
    pub async fn private_sync(
        &self,
        peer: &PeerInfo,
        wallet_id: &str,
        encrypted: EncryptedSessionPayload,
    ) -> Result<EncryptedSessionPayload> {
        let addr = Self::addr_from_peer(peer)?;
        let conn = self.connect(&addr).await?;
        let resp = self
            .round_trip(
                &conn,
                &MeshRequest::PrivateSync {
                    wallet_id: wallet_id.to_string(),
                    encrypted,
                },
            )
            .await?;
        serde_json::from_value(resp).context("parse private sync response")
    }

    /// Spawn the server-side accept loop in a background Tokio task.
    pub async fn spawn_accept_loop(&self) -> tokio::task::JoinHandle<()> {
        let endpoint = match self.endpoint().await {
            Ok(ep) => ep.clone(),
            Err(e) => {
                tracing::error!("Failed to initialize endpoint for accept loop: {e:#}");
                return tokio::spawn(async {});
            }
        };
        let local_identity = self.local_identity.clone();
        let store = self.store.clone();
        let policy = self.allowed_peers.clone();
        tokio::spawn(async move {
            if let Err(err) = Self::run_accept_loop(endpoint, local_identity, store, policy).await {
                tracing::warn!("Iroh accept loop ended: {err:#}");
            }
        })
    }

    /// Run the server-side accept loop on the Iroh endpoint.
    pub async fn accept_loop(&self) -> Result<()> {
        let endpoint = self.endpoint().await?.clone();
        Self::run_accept_loop(
            endpoint,
            self.local_identity.clone(),
            self.store.clone(),
            self.allowed_peers.clone(),
        )
        .await
    }

    /// Internal worker loop for processing incoming QUIC connections and bi-streams.
    async fn run_accept_loop(
        endpoint: Endpoint,
        local_identity: Arc<NodeIdentity>,
        store: Arc<dyn MemoryStore>,
        policy: PeerPolicy,
    ) -> Result<()> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        while let Some(incoming) = endpoint.accept().await {
            let Ok(connecting) = incoming.accept() else {
                continue;
            };
            let Ok(conn) = connecting.await else {
                continue;
            };

            if conn.alpn() != XMESH_ALPN {
                tracing::debug!("Rejected connection with mismatched ALPN");
                continue;
            }

            let local_identity = local_identity.clone();
            let store = store.clone();
            let policy = policy.clone();
            tokio::spawn(async move {
                while let Ok((mut send, mut recv)) = conn.accept_bi().await {
                    let mut len_buf = [0u8; 4];
                    if recv.read_exact(&mut len_buf).await.is_err() {
                        break;
                    }
                    let len = u32::from_be_bytes(len_buf) as usize;
                    let mut buf = vec![0u8; len];
                    if recv.read_exact(&mut buf).await.is_err() {
                        break;
                    }

                    let req: MeshRequest = match serde_json::from_slice(&buf) {
                        Ok(r) => r,
                        Err(e) => {
                            tracing::warn!("Failed to deserialize MeshRequest: {e:#}");
                            break;
                        }
                    };

                    let response_value =
                        handle_request_with_policy(&local_identity, &*store, &req, &policy).await;

                    let resp_bytes = match response_value.and_then(|v| Ok(serde_json::to_vec(&v)?))
                    {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            tracing::warn!("Failed to serialize response: {e:#}");
                            break;
                        }
                    };

                    if send
                        .write_all(&(resp_bytes.len() as u32).to_be_bytes())
                        .await
                        .is_err()
                    {
                        break;
                    }
                    if send.write_all(&resp_bytes).await.is_err() {
                        break;
                    }
                    let _ = send.finish();
                }
            });
        }
        Ok(())
    }
}

/// Process an incoming `MeshRequest` and return the JSON response value.
///
/// This is the server-side dispatch function used by the Iroh accept loop.
/// It reads real data from the local [`MemoryStore`] via the sync manifest and
/// chunk lookup helpers, instead of returning empty stubs.
///
/// Extracted as a standalone `async fn` (outside the `impl` block) so it can
/// be unit-tested without an Iroh endpoint.
pub(crate) async fn handle_request(
    local_identity: &NodeIdentity,
    store: &dyn MemoryStore,
    req: &MeshRequest,
) -> Result<serde_json::Value> {
    handle_request_with_policy(local_identity, store, req, &None).await
}

/// Reject a handshake whose identity is forged or not allowlisted.
fn check_handshake(body: &MeshHandshake, policy: &HashSet<String>) -> Result<(), String> {
    let pk = crate::crypto::hex_decode(&body.public_key_hex).map_err(|_| "bad public key")?;
    let sig = crate::crypto::hex_decode(&body.signature_hex).map_err(|_| "bad signature")?;
    if !NodeIdentity::verify(&pk, body.nonce.as_bytes(), &sig) {
        return Err("invalid handshake signature".into());
    }
    if crate::mesh::node::NodeId::from_public_key_bytes(&pk) != body.node_id {
        return Err("node_id does not match public key".into());
    }
    if !policy.contains(body.node_id.as_str()) {
        return Err("unknown peer".into());
    }
    Ok(())
}

/// Verify, decrypt and apply a sealed memory sync. Fails closed: without an
/// allowlist, or for an unlisted or forged sender, nothing is applied.
async fn apply_memory_sync(
    local_identity: &NodeIdentity,
    store: &dyn MemoryStore,
    policy: &PeerPolicy,
    req: &MeshRequest,
) -> Result<MemorySyncAck, String> {
    let MeshRequest::MemorySync {
        sender_public_key_hex,
        timestamp,
        nonce,
        signature_hex,
        encrypted,
    } = req
    else {
        return Err("not a memory sync".into());
    };
    let pk = crate::crypto::hex_decode(sender_public_key_hex).map_err(|_| "bad sender key")?;
    let sig = crate::crypto::hex_decode(signature_hex).map_err(|_| "bad signature")?;
    let message = memory_sync_message(*timestamp, nonce, encrypted);
    if !NodeIdentity::verify(&pk, &message, &sig) {
        return Err("invalid sender signature".into());
    }
    let sender = crate::mesh::node::NodeId::from_public_key_bytes(&pk);
    match policy {
        Some(allowed) if allowed.contains(sender.as_str()) => {}
        _ => return Err("unknown peer".into()),
    }
    if (chrono::Utc::now().timestamp() - timestamp).abs() > MEMORY_SYNC_MAX_SKEW_SECS {
        return Err("stale request".into());
    }
    let seed: [u8; 32] = local_identity
        .private_key_bytes()
        .try_into()
        .map_err(|_| "bad local key")?;
    let secret = signing_key_to_x25519_secret(&ed25519_dalek::SigningKey::from_bytes(&seed));
    let payload = decrypt_session_payload(encrypted, &secret).map_err(|e| e.to_string())?;

    let mut diffs = Vec::with_capacity(payload.memories.len());
    for m in &payload.memories {
        let path = m.path.trim();
        if path.is_empty()
            || path.starts_with('/')
            || path.starts_with('\\')
            || path.split(['/', '\\']).any(|c| c == "..")
        {
            return Err(format!("unsafe memory path: {path}"));
        }
        let workspace = m
            .metadata
            .get("workspace_id")
            .and_then(|v| v.as_str())
            .unwrap_or(MESH_SYNC_DEFAULT_WORKSPACE)
            .to_string();
        let at = chrono::DateTime::from_timestamp(m.created_at, 0).unwrap_or_else(chrono::Utc::now);
        let record = crate::memory::store::MemoryRecord {
            id: uuid::Uuid::new_v4().to_string(),
            workspace_id: workspace.clone(),
            path: path.to_string(),
            content: m.content.clone(),
            metadata: m.metadata.clone(),
            embedding: Vec::new(),
            created_at: at,
            updated_at: at,
            revision: 1,
            primary: true,
            parent_id: None,
            cluster_id: None,
            level: Default::default(),
            relation: None,
            clearance: Default::default(),
            revisions: Vec::new(),
            encrypted_dek: None,
            content_iv: None,
            metadata_iv: None,
            score: 0.0,
            deleted_at: None,
            embedding_status: "pending".into(),
            embedding_attempts: 0,
        };
        let data =
            crate::memory::sync::merge::serialise_chunk(&record).map_err(|e| e.to_string())?;
        diffs.push(crate::memory::sync::ChunkDiff {
            chunk_hash: crate::memory::sync::merge::chunk_hash(&record),
            namespace: workspace,
            action: crate::memory::sync::DiffAction::Add,
            data: Some(data),
            timestamp: std::time::SystemTime::now(),
            record_path: Some(record.path.clone()),
        });
    }
    crate::memory::sync::merge::validate_changes_received(
        &diffs,
        crate::security::clearance::ClearanceLevel::Internal,
    )
    .map_err(|e| e.to_string())?;
    let mut conflicts = 0u64;
    crate::memory::sync::merge::apply_changes_received(store, &diffs, &mut conflicts)
        .await
        .map_err(|e| e.to_string())?;
    Ok(MemorySyncAck {
        accepted: true,
        applied: diffs.len(),
        reason: None,
    })
}

/// [`handle_request`] with a peer policy enforced for handshakes and memory sync.
async fn handle_request_with_policy(
    local_identity: &NodeIdentity,
    store: &dyn MemoryStore,
    req: &MeshRequest,
    policy: &PeerPolicy,
) -> Result<serde_json::Value> {
    match req {
        MeshRequest::MemorySync { .. } => {
            let ack = match apply_memory_sync(local_identity, store, policy, req).await {
                Ok(ack) => ack,
                Err(reason) => {
                    tracing::warn!("memory sync rejected: {reason}");
                    MemorySyncAck {
                        accepted: false,
                        applied: 0,
                        reason: Some(reason),
                    }
                }
            };
            Ok(serde_json::to_value(ack)?)
        }

        MeshRequest::Handshake { body } if policy.is_some() => {
            let verdict = check_handshake(body, policy.as_deref().expect("checked"));
            let resp = MeshHandshakeResponse {
                accepted: verdict.is_ok(),
                node_id: local_identity.node_id.clone(),
                public_key_hex: crate::crypto::hex_encode(&local_identity.public_key),
                reason: verdict.err(),
            };
            Ok(serde_json::to_value(resp)?)
        }

        MeshRequest::Handshake { body: _ } => {
            let resp = MeshHandshakeResponse {
                accepted: true,
                node_id: local_identity.node_id.clone(),
                public_key_hex: crate::crypto::hex_encode(&local_identity.public_key),
                reason: None,
            };
            Ok(serde_json::to_value(resp)?)
        }

        MeshRequest::FetchManifest { .. } => {
            // Build the real manifest from the local store.
            let sync_manifest = crate::memory::sync::manifest::build_manifest(store).await?;

            // Convert sync ManifestEntries → protocol ChunkRefs.
            let chunks: Vec<ChunkRef> = sync_manifest
                .into_iter()
                .map(|entry| ChunkRef {
                    hash: entry.chunk_hash,
                    document_count: entry.size_bytes as usize,
                    created_at: entry.updated_at.timestamp(),
                })
                .collect();

            let resp = MeshManifest {
                node_id: local_identity.node_id.clone(),
                chunks,
                generated_at: chrono::Utc::now().timestamp(),
            };
            Ok(serde_json::to_value(resp)?)
        }

        MeshRequest::FetchChunks { request } => {
            // Look up each requested hash across all workspaces.
            let workspaces = store.list_workspaces().await?;
            let mut result: HashMap<String, Vec<u8>> = HashMap::new();

            for ws in &workspaces {
                let records = store.list(ws).await?;
                for rec in &records {
                    let hash = crate::memory::sync::merge::chunk_hash(rec);
                    if request.wanted_hashes.contains(&hash) {
                        let data = crate::memory::sync::merge::serialise_chunk(rec)?;
                        result.insert(hash, data);
                    }
                }
                // Early exit once we've found all requested hashes.
                if result.len() >= request.wanted_hashes.len() {
                    break;
                }
            }
            Ok(serde_json::to_value(result)?)
        }

        MeshRequest::PushChunks { request } => {
            // PushChunks carries only the hashes being offered (no payload in
            // the current protocol). Acknowledge all offered hashes.
            Ok(serde_json::to_value(&request.wanted_hashes)?)
        }

        MeshRequest::ShareSession { .. } => {
            Ok(serde_json::to_value(serde_json::json!({"status": "ok"}))?)
        }

        MeshRequest::PrivateSync {
            wallet_id: _,
            encrypted,
        } => Ok(serde_json::to_value(encrypted)?),
    }
}

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
    fn test_private_sync_request_serializes_with_op_tag() {
        let req = MeshRequest::PrivateSync {
            wallet_id: "w1".into(),
            encrypted: EncryptedSessionPayload {
                ciphertext_hex: "aabb".into(),
                nonce_hex: "1122".into(),
                ephemeral_public_key_hex: None,
            },
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"op\":\"private_sync\""));
        assert!(json.contains("\"wallet_id\":\"w1\""));
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
        assert!(
            json.contains("\"op\":\"fetch_chunks\""),
            "serialized: {json}"
        );
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
            shared_workspace_ids: Vec::new(),
            shared_workspace_tokens: std::collections::HashMap::new(),
            capabilities: Vec::new(),
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
            shared_workspace_ids: Vec::new(),
            shared_workspace_tokens: std::collections::HashMap::new(),
            capabilities: Vec::new(),
        };
        assert_eq!(
            IrohTransport::addr_from_peer(&peer).unwrap(),
            "deadbeefdeadbeef"
        );
    }

    #[tokio::test]
    async fn connect_rejects_empty_whitespace_and_control_chars_without_io() {
        let transport =
            IrohTransport::new(Arc::new(test_identity()), test_store_with_records(vec![]));

        let invalid_inputs = [
            "",
            "   ",
            "key\nwith\nnewlines",
            "key\twith\ttabs",
            "key\r\nctrl",
        ];
        for input in invalid_inputs {
            let res = transport.connect(input).await;
            assert!(res.is_err(), "Expected error for input {:?}", input);
            let err = res.unwrap_err().to_string();
            assert!(
                err.contains("invalid iroh peer addr"),
                "Error string for {:?} was: {}",
                input,
                err
            );
        }

        // Endpoint must NOT have been initialized/bound (zero IO occurred).
        assert!(
            transport.endpoint.get().is_none(),
            "Endpoint must remain uninitialized on input guard rejection"
        );
    }

    #[test]
    fn addr_from_peer_rejects_control_chars() {
        let peer = PeerInfo {
            node_id: crate::mesh::node::NodeId("peer-ctrl".into()),
            alias: None,
            endpoint_url: String::new(),
            public_key_hex: "aabb".into(),
            added_at: 0,
            last_seen_at: None,
            sync_enabled: true,
            is_cloud: false,
            iroh_addr: Some("invalid\naddr\tcontrol".into()),
            shared_workspace_ids: Vec::new(),
            shared_workspace_tokens: std::collections::HashMap::new(),
            capabilities: Vec::new(),
        };
        let res = IrohTransport::addr_from_peer(&peer);
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("invalid iroh peer addr"));
    }

    #[test]
    fn addr_from_peer_and_valid_key_format() {
        let secret = iroh::SecretKey::generate();
        let public_key = secret.public();
        let pk_str = public_key.to_string();

        let peer = PeerInfo {
            node_id: crate::mesh::node::NodeId("peer-valid".into()),
            alias: None,
            endpoint_url: String::new(),
            public_key_hex: "aabb".into(),
            added_at: 0,
            last_seen_at: None,
            sync_enabled: true,
            is_cloud: false,
            iroh_addr: Some(pk_str.clone()),
            shared_workspace_ids: Vec::new(),
            shared_workspace_tokens: std::collections::HashMap::new(),
            capabilities: Vec::new(),
        };

        let resolved = IrohTransport::addr_from_peer(&peer).unwrap();
        assert_eq!(resolved, pk_str);
        assert!(resolved.parse::<iroh::PublicKey>().is_ok());
    }

    #[tokio::test]
    async fn handshake_times_out_on_unroutable_addr() {
        tokio::time::pause();
        let transport =
            IrohTransport::new(Arc::new(test_identity()), test_store_with_records(vec![]));

        let secret = iroh::SecretKey::generate();
        let unroutable_pk = secret.public().to_string();

        let res = transport.handshake(&unroutable_pk, "token", None).await;
        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(
            err_msg.contains("handshake timed out for peer"),
            "Error was: {}",
            err_msg
        );
    }

    // -----------------------------------------------------------------------
    // Tests for handle_request — the accept loop dispatch function.
    // -----------------------------------------------------------------------

    use crate::memory::sync::manifest::tests::TestStore;

    fn test_identity() -> NodeIdentity {
        NodeIdentity::generate()
    }

    fn test_store_with_records(records: Vec<crate::memory::store::MemoryRecord>) -> Arc<TestStore> {
        Arc::new(TestStore {
            records: std::sync::Mutex::new(records),
        })
    }

    fn make_test_record(
        id: &str,
        workspace: &str,
        content: &str,
        revision: u64,
    ) -> crate::memory::store::MemoryRecord {
        crate::memory::store::MemoryRecord {
            id: id.into(),
            workspace_id: workspace.into(),
            path: format!("test/{id}"),
            content: content.into(),
            metadata: serde_json::Value::Null,
            embedding: Vec::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            revision,
            primary: true,
            parent_id: None,
            cluster_id: None,
            level: Default::default(),
            relation: None,
            clearance: Default::default(),
            revisions: Vec::new(),
            encrypted_dek: None,
            content_iv: None,
            metadata_iv: None,
            score: 0.0,
            deleted_at: None,
            embedding_status: "ok".into(),
            embedding_attempts: 0,
        }
    }

    #[tokio::test]
    async fn handle_handshake_returns_identity() {
        let identity = test_identity();
        let store = test_store_with_records(vec![]);
        let req = MeshRequest::Handshake {
            body: crate::mesh::protocol::MeshHandshake {
                node_id: crate::mesh::node::NodeId("remote".into()),
                public_key_hex: "dead".into(),
                xavier_version: "0.1.0".into(),
                capabilities: vec![],
                timestamp: 0,
                nonce: "n".into(),
                signature_hex: "s".into(),
                pairing_secret: None,
            },
        };
        let val = handle_request(&identity, &*store, &req).await.unwrap();
        assert_eq!(val["accepted"], true);
        assert_eq!(val["node_id"], identity.node_id.as_str());
    }

    #[tokio::test]
    async fn handle_fetch_manifest_empty_store() {
        let identity = test_identity();
        let store = test_store_with_records(vec![]);
        let req = MeshRequest::FetchManifest {
            node_id: "remote".into(),
            timestamp: "0".into(),
            nonce: "n".into(),
            signature: "s".into(),
        };
        let val = handle_request(&identity, &*store, &req).await.unwrap();
        let chunks = val["chunks"].as_array().unwrap();
        assert!(
            chunks.is_empty(),
            "empty store should produce empty manifest"
        );
        assert_eq!(val["node_id"], identity.node_id.as_str());
    }

    #[tokio::test]
    async fn handle_fetch_manifest_with_records() {
        let identity = test_identity();
        let store = test_store_with_records(vec![make_test_record("r1", "ws1", "hello", 1)]);
        let req = MeshRequest::FetchManifest {
            node_id: "remote".into(),
            timestamp: "0".into(),
            nonce: "n".into(),
            signature: "s".into(),
        };
        let val = handle_request(&identity, &*store, &req).await.unwrap();
        let chunks = val["chunks"].as_array().unwrap();
        assert_eq!(chunks.len(), 1, "one record should produce one chunk ref");
        assert!(!chunks[0]["hash"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn handle_fetch_chunks_returns_real_data() {
        let identity = test_identity();
        let rec = make_test_record("r2", "ws2", "chunk data", 1);
        let store = test_store_with_records(vec![rec]);

        // Compute the expected hash for the record.
        let records = {
            let lock = store.records.lock().unwrap();
            lock.clone()
        };
        let expected_hash = crate::memory::sync::merge::chunk_hash(&records[0]);

        let req = MeshRequest::FetchChunks {
            request: MeshSyncRequest {
                requesting_node_id: crate::mesh::node::NodeId("remote".into()),
                wanted_hashes: vec![expected_hash.clone()],
                timestamp: 0,
                nonce: "n".into(),
                signature_hex: "s".into(),
            },
        };
        let val = handle_request(&identity, &*store, &req).await.unwrap();
        let map = val.as_object().unwrap();
        assert!(
            map.contains_key(&expected_hash),
            "response must contain the requested hash"
        );
        // The value is serialized MemoryRecord bytes.
        let data = map[&expected_hash].as_array().unwrap();
        assert!(!data.is_empty(), "chunk data must not be empty");
    }

    #[tokio::test]
    async fn handle_fetch_chunks_missing_hash() {
        let identity = test_identity();
        let store = test_store_with_records(vec![]);
        let req = MeshRequest::FetchChunks {
            request: MeshSyncRequest {
                requesting_node_id: crate::mesh::node::NodeId("remote".into()),
                wanted_hashes: vec!["nonexistent_hash".into()],
                timestamp: 0,
                nonce: "n".into(),
                signature_hex: "s".into(),
            },
        };
        let val = handle_request(&identity, &*store, &req).await.unwrap();
        let map = val.as_object().unwrap();
        assert!(
            map.is_empty(),
            "missing hash should produce empty result map"
        );
    }

    #[tokio::test]
    async fn handle_push_chunks_acks_hashes() {
        let identity = test_identity();
        let store = test_store_with_records(vec![]);
        let hashes = vec!["h1".into(), "h2".into()];
        let req = MeshRequest::PushChunks {
            request: MeshSyncRequest {
                requesting_node_id: crate::mesh::node::NodeId("remote".into()),
                wanted_hashes: hashes.clone(),
                timestamp: 0,
                nonce: "n".into(),
                signature_hex: "s".into(),
            },
        };
        let val = handle_request(&identity, &*store, &req).await.unwrap();
        let acked: Vec<String> = serde_json::from_value(val).unwrap();
        assert_eq!(acked, hashes);
    }

    #[tokio::test]
    async fn handle_private_sync_echoes_payload() {
        let identity = test_identity();
        let store = test_store_with_records(vec![]);
        let enc = crate::mesh::private_mesh::EncryptedSessionPayload {
            ciphertext_hex: "aabb".into(),
            nonce_hex: "1122".into(),
            ephemeral_public_key_hex: None,
        };
        let req = MeshRequest::PrivateSync {
            wallet_id: "w1".into(),
            encrypted: enc.clone(),
        };
        let val = handle_request(&identity, &*store, &req).await.unwrap();
        let result: crate::mesh::private_mesh::EncryptedSessionPayload =
            serde_json::from_value(val).unwrap();
        assert_eq!(result.ciphertext_hex, enc.ciphertext_hex);
        assert_eq!(result.nonce_hex, enc.nonce_hex);
    }
}
