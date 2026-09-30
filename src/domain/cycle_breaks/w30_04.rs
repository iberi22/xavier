//! Shared data definitions for health, observability, alerts, and mesh.
//! Adapter-specific behavior stays at the original implementation sites.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

/// A unique identifier for a Xavier Mesh node.
///
/// Derived from the Ed25519 public key — stable across reboots, network
/// changes, and IP address changes.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub String);

/// Information about a trusted peer node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub node_id: NodeId,
    pub alias: Option<String>,
    pub endpoint_url: String,
    pub public_key_hex: String,
    pub added_at: i64,
    pub last_seen_at: Option<i64>,
    pub sync_enabled: bool,
    #[serde(default)]
    pub is_cloud: bool,
    /// Iroh endpoint address for QUIC-based P2P sync (Phase 2 mesh).
    ///
    /// Holds the remote endpoint's `EndpointId` string (an Ed25519 `PublicKey`
    /// encoding) used by the Iroh transport to dial
    /// the peer. `None`/absent for peers that only speak HTTP mesh — existing
    /// `mesh_peers.json` files deserialize unchanged thanks to
    /// `#[serde(default)]`.
    #[serde(default)]
    pub iroh_addr: Option<String>,
    #[serde(default)]
    pub shared_workspace_ids: Vec<String>,
    #[serde(default)]
    pub shared_workspace_tokens: HashMap<String, String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// A persistent, file-backed registry of trusted peers.
#[derive(Clone, Debug)]
pub struct PeerRegistry {
    pub(crate) peers: HashMap<NodeId, PeerInfo>,
    pub(crate) storage_path: PathBuf,
}

/// Maturity report of the Xavier Mesh components.
/// Exposes honest maturity percentages and feature presence flags.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MeshMaturityReport {
    /// HTTP mesh transport: fully functional for handshakes, manifests, chunks sync, session sharing.
    pub http_transport: bool,
    pub http_transport_percent: u8,
    /// libp2p transport: legacy/broken in current build, superseded by Iroh.
    pub libp2p: bool,
    pub libp2p_percent: u8,
    /// Mesh access control lists (ACL): fully functional.
    pub acl: bool,
    pub acl_percent: u8,
    /// Tokenomics: XP-based placeholder/mock system is present.
    pub tokenomics: bool,
    pub tokenomics_percent: u8,
    /// On-chain governance (DAO): not implemented/unsupported.
    pub onchain_gov: bool,
    pub onchain_gov_percent: u8,
}

/// Metrics for a single peer node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerMetrics {
    pub uptime_secs: u64,
    pub message_count: u64,
    pub latencies_ms: VecDeque<u64>,
    pub agreement_outcomes: VecDeque<bool>,
    pub last_seen: u64,
}

/// Collector for mesh-wide peer telemetry.
#[derive(Debug)]
pub struct MeshTelemetryCollector {
    pub(crate) peer_metrics: Arc<Mutex<HashMap<NodeId, PeerMetrics>>>,
    pub(crate) started_at: Instant,
}

#[derive(Debug, Clone)]
pub(crate) struct SuppressionRecord {
    pub(crate) last_seen: Instant,
    pub(crate) count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum OperationalMode {
    LocalHealthy,
    LocalDegraded,
    CloudFallback,
    Disabled,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SystemAlert {
    pub id: String,
    pub level: String,
    pub message: String,
    pub component: String,
    pub created_at: DateTime<Utc>,
}

pub struct SystemAlertStore {
    pub(crate) alerts: RwLock<Vec<SystemAlert>>,
    pub(crate) last_email_sent: RwLock<std::collections::HashMap<String, DateTime<Utc>>>,
    pub(crate) suppression_map: Mutex<HashMap<(String, String), SuppressionRecord>>,
    pub(crate) total_suppressed_count: AtomicU64,
}

impl SystemAlertStore {
    /// New.
    pub fn new() -> Self {
        Self {
            alerts: RwLock::new(Vec::new()),
            last_email_sent: RwLock::new(std::collections::HashMap::new()),
            suppression_map: Mutex::new(HashMap::new()),
            total_suppressed_count: AtomicU64::new(0),
        }
    }
}

// Global instance
pub static SYSTEM_ALERTS: std::sync::LazyLock<SystemAlertStore> =
    std::sync::LazyLock::new(SystemAlertStore::new);
