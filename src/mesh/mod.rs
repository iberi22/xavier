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
pub mod network;
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
pub use network::{CrossGrant, MeshNetwork, NetworkAcl};
pub use node::{NodeId, NodeIdentity};
pub use p2p::{
    calculate_backoff, parse_strategy, CandidatePair, CandidatePairState, FallbackError,
    FallbackStrategy, FilterSummary, FilteredSyncSession, HolePunchState, IceCandidate,
    IceCandidateType, NatTraversalEngine, NatTraversalError, NatType, OfflineQueue,
    OfflineQueueConfig, QueuedMessage, StunAttribute, StunMessage, StunMessageType, SyncFilter,
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

/// Create a minimal in-memory store for client-only transport construction.
///
/// The Iroh accept loop (server-side) needs a real [`MemoryStore`] to serve
/// chunks, but client-side callers (CLI ping, sync, etc.) only use the
/// transport for outbound requests. This helper provides a no-op store so
/// those callers don't need to bootstrap a full database.
pub fn dummy_store() -> std::sync::Arc<dyn crate::memory::store::MemoryStore> {
    use crate::memory::store::*;
    use std::sync::Arc;

    struct _Dummy;
    #[async_trait::async_trait]
    impl MemoryStore for _Dummy {
        fn backend(&self) -> MemoryBackend {
            MemoryBackend::Memory
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        async fn health(&self) -> anyhow::Result<String> {
            Ok("ok".into())
        }
        async fn put(&self, _: MemoryRecord) -> anyhow::Result<()> {
            Ok(())
        }
        async fn get(&self, _: &str, _: &str) -> anyhow::Result<Option<MemoryRecord>> {
            Ok(None)
        }
        async fn update(&self, _: MemoryRecord) -> anyhow::Result<()> {
            Ok(())
        }
        async fn delete(&self, _: &str, _: &str) -> anyhow::Result<Option<MemoryRecord>> {
            Ok(None)
        }
        async fn list(&self, _: &str) -> anyhow::Result<Vec<MemoryRecord>> {
            Ok(Vec::new())
        }
        async fn list_workspaces(&self) -> anyhow::Result<Vec<String>> {
            Ok(Vec::new())
        }
        async fn search(
            &self,
            _: &str,
            _: &str,
            _: Option<&crate::memory::schema::MemoryQueryFilters>,
        ) -> anyhow::Result<Vec<MemoryRecord>> {
            Ok(Vec::new())
        }
        async fn load_workspace_state(&self, _: &str) -> anyhow::Result<DurableWorkspaceState> {
            anyhow::bail!("dummy store")
        }
        async fn save_beliefs(
            &self,
            _: &str,
            _: Vec<crate::domain::memory::belief::BeliefEdge>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn save_session_token(&self, _: &str, _: SessionTokenRecord) -> anyhow::Result<()> {
            Ok(())
        }
        async fn is_session_token_valid(&self, _: &str, _: &str) -> anyhow::Result<bool> {
            Ok(false)
        }
        async fn save_checkpoint(
            &self,
            _: &str,
            _: crate::checkpoint::Checkpoint,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn load_checkpoint(
            &self,
            _: &str,
            _: &str,
            _: &str,
        ) -> anyhow::Result<Option<crate::checkpoint::Checkpoint>> {
            Ok(None)
        }
        async fn list_checkpoints(
            &self,
            _: &str,
            _: &str,
        ) -> anyhow::Result<Vec<crate::checkpoint::Checkpoint>> {
            Ok(Vec::new())
        }
        async fn delete_checkpoint(&self, _: &str, _: &str, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }
    Arc::new(_Dummy)
}

/// Active Iroh Transport initialization helper.
#[cfg(feature = "mesh")]
pub fn init_active_transport(
    identity: std::sync::Arc<NodeIdentity>,
    store: std::sync::Arc<dyn crate::memory::store::MemoryStore>,
) -> IrohTransport {
    IrohTransport::new(identity, store)
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
    store: std::sync::Arc<dyn crate::memory::store::MemoryStore>,
) -> (IrohTransport, tokio::task::JoinHandle<()>) {
    let transport = init_active_transport(identity, store);
    let handle = transport.spawn_accept_loop().await;
    (transport, handle)
}
