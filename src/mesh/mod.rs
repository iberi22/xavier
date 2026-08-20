//! Xavier Mesh — Distributed P2P Memory Synchronization
//!
//! This module implements the foundational layer for connecting Xavier nodes
//! across the internet in a secure, peer-to-peer manner. Each Xavier instance
//! is identified by a unique **NodeID** derived from an Ed25519 public key,
//! enabling cryptographically-authenticated connections without relying on
//! central servers or IP addresses.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                  Xavier Mesh Layer                  │
//! ├─────────────────────────────────────────────────────┤
//! │  Identity   │ NodeID = blake3(ed25519_public_key)   │
//! │  Protocol   │ XMesh-Sync v1: handshake + manifest   │
//! │  Transport  │ HTTP REST (Phase 1) → Iroh/QUIC (P2)  │
//! │  Registry   │ Persistent peer list with metadata    │
//! └─────────────────────────────────────────────────────┘
//! ```
//!
//! # Phase 1 Scope
//!
//! - NodeIdentity generation and persistence (Ed25519 keypair)
//! - Peer registry (add, list, remove trusted peers)
//! - HTTP-based sync transport (connect to a remote Xavier via its HTTP API)
//! - XMesh-Sync v1 protocol types (manifest exchange, chunk requests)
//! - CLI commands: `xavier mesh id`, `mesh add-peer`, `mesh list`, `mesh sync`
//!
//! # Future Phases
//!
//! - Phase 2: Iroh QUIC transport with automatic NAT traversal
//! - Phase 3: Loro CRDT for conflict-free memory merge
//! - Phase 4: Tor/Yggdrasil transport for anonymous operation

pub mod acl;
pub mod auth;
#[cfg(feature = "mesh")]
pub mod auto_update;
pub mod challenge;
pub mod cloud_node;
pub mod context_bridge;
pub mod crypto_gating;
pub mod dashboard;
pub mod data_consent;
pub mod data_sanitizer;
pub mod governance;
#[cfg(feature = "mesh")]
pub mod heartbeat;
#[cfg(feature = "mesh")]
pub mod iroh_transport;
pub mod maturity;
pub mod namespace;
pub mod node;
pub mod p2p;
pub mod pairing;
pub mod pairing_registry;
pub mod peer;
pub mod private_mesh;
pub mod pro_gate;
pub mod protocol;
pub mod public_directory;
pub mod public_rag;
pub mod registry;
pub mod service_network;
pub mod telemetry;
pub mod telemetry_collector;
pub mod tokenomics;
pub mod transport;

pub use acl::{MeshAcl, NamespaceAclEntry, NodeAclEntry};
#[cfg(feature = "mesh")]
pub use auto_update::{AutoUpdateService, UpdateStatus};
pub use context_bridge::{BridgeKind, BridgeRegistry, ContextBridge};
pub use dashboard::{aggregate_dashboard, MeshBandwidth, MeshDashboardResponse, MeshPeerHealth};
pub use data_consent::{ActiveConsent, ConsentLevel, ConsentRecord, DataConsentManager};
pub use data_sanitizer::{DataSanitizer, SanitizationAction, SanitizationRule};
#[cfg(feature = "mesh")]
pub use heartbeat::{HeartbeatPayload, HeartbeatReceipt, HeartbeatService, HeartbeatStatus};
pub use maturity::MeshMaturityReport;
pub use node::{NodeId, NodeIdentity};
pub use p2p::{
    CandidatePair, CandidatePairState, HolePunchState, IceCandidate, IceCandidateType, NatType,
    NatTraversalEngine, NatTraversalError, StunAttribute, StunMessage, StunMessageType,
    SyncFilter, SyncFilterConfig, SyncFilterDecision, SyncFilterError, SyncFilterStats,
    TransportProtocol, TurnServerConfig,
};
pub use peer::{PeerInfo, PeerRegistry};
pub use private_mesh::{derive_wallet_id, is_same_wallet, PrivateMeshRegistry, WalletNode};
pub use protocol::{MeshHandshake, MeshManifest, MeshSyncRequest};
pub use public_rag::{search_public, PublicRagQuery, PublicRagResult};
pub use registry::PeerRegistrySyncAdapter;
pub use service_network::{ServiceInfo, ServiceKind, ServiceRegistry, TelemetrySample};
pub use telemetry_collector::{RetentionPolicy, TelemetryAggregate, TelemetryCollector};
pub use tokenomics::{
    ContributionType, RewardEngine, RewardEvent, Transaction, TransactionKind, Wallet,
    WalletBalance,
};
pub use transport::MeshTransport;

#[cfg(feature = "mesh")]
pub use iroh_transport::IrohTransport;

/// Active Iroh Transport initialization helper.
#[cfg(feature = "mesh")]
pub fn init_active_transport(identity: std::sync::Arc<NodeIdentity>) -> IrohTransport {
    IrohTransport::new(identity)
}

/// Helper method to connect via the active Iroh transport.
#[cfg(feature = "mesh")]
pub async fn connect_active_transport(
    transport: &IrohTransport,
    peer_addr: &str,
) -> anyhow::Result<iroh::endpoint::Connection> {
    transport.connect(peer_addr).await
}

/// Initialize active Iroh transport and start the background accept loop.
#[cfg(feature = "mesh")]
pub async fn start_mesh_node(
    identity: std::sync::Arc<NodeIdentity>,
) -> (IrohTransport, tokio::task::JoinHandle<()>) {
    let transport = init_active_transport(identity);
    let handle = transport.spawn_accept_loop().await;
    (transport, handle)
}
