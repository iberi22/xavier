// SPDX-License-Identifier: MIT OR LICENSE-MESH
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
pub mod cloud_node;
pub mod crypto_gating;
pub mod data_consent;
pub mod data_sanitizer;
#[cfg(feature = "mesh-legacy")]
pub mod discovery;
pub mod governance;
#[cfg(feature = "mesh")]
pub mod heartbeat;
#[cfg(feature = "mesh")]
pub mod iroh_transport;
pub mod maturity;
pub mod node;
pub mod pairing;
pub mod pairing_registry;
pub mod peer;
pub mod protocol;
pub mod context_bridge;
pub mod telemetry;
pub mod telemetry_collector;
pub mod tokenomics;
pub mod transport;
// Legacy libp2p transport — broken against libp2p 0.56 and superseded by Iroh.
// Gated behind mesh-legacy so it doesn't break the default mesh build.
#[cfg(feature = "mesh-legacy")]
pub mod libp2p_transport;

pub use acl::{MeshAcl, NodeAclEntry, NamespaceAclEntry};
#[cfg(feature = "mesh")]
pub use auto_update::{AutoUpdateService, UpdateStatus};
pub use data_consent::{ConsentLevel, DataConsentManager, ConsentRecord, ActiveConsent};
pub use data_sanitizer::{DataSanitizer, SanitizationAction, SanitizationRule};
#[cfg(feature = "mesh-legacy")]
pub use discovery::DiscoveryService;
#[cfg(feature = "mesh")]
pub use heartbeat::{HeartbeatPayload, HeartbeatReceipt, HeartbeatService};
pub use maturity::MeshMaturityReport;
pub use node::{NodeId, NodeIdentity};
pub use peer::{PeerInfo, PeerRegistry};
pub use protocol::{MeshHandshake, MeshManifest, MeshSyncRequest};
pub use context_bridge::{BridgeKind, ContextBridge, BridgeRegistry};
pub use telemetry_collector::{
    RetentionPolicy, TelemetryAggregate, TelemetryCollector, TelemetrySample,
};
pub use tokenomics::{
    ContributionType, RewardEngine, RewardEvent, Transaction, TransactionKind, Wallet,
    WalletBalance,
};
pub use transport::MeshTransport;
