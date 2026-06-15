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
pub mod auto_update;
pub mod cloud_node;
pub mod crypto_gating;
pub mod data_consent;
pub mod data_sanitizer;
pub mod discovery;
pub mod governance;
pub mod heartbeat;
pub mod node;
pub mod pairing;
pub mod pairing_registry;
pub mod peer;
pub mod protocol;
pub mod telemetry;
pub mod telemetry_collector;
pub mod tokenomics;
pub mod transport;

pub use acl::{MeshAcl, NodeAclEntry};
pub use auto_update::{AutoUpdateService, UpdateStatus};
pub use data_consent::{ConsentLevel, DataConsentManager};
pub use data_sanitizer::{DataSanitizer, SanitizationAction, SanitizationRule};
pub use discovery::DiscoveryService;
pub use heartbeat::{HeartbeatPayload, HeartbeatReceipt, HeartbeatService};
pub use node::{NodeId, NodeIdentity};
pub use peer::{PeerInfo, PeerRegistry};
pub use protocol::{MeshHandshake, MeshManifest, MeshSyncRequest};
pub use telemetry_collector::{RetentionPolicy, TelemetryAggregate, TelemetryCollector, TelemetrySample};
pub use tokenomics::{Wallet, WalletBalance, Transaction, TransactionKind, RewardEngine, RewardEvent, ContributionType};
pub use transport::MeshTransport;
