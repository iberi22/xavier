//! Node Provisioning Core module (Olas M6/M7, REQ-029/030)
//!
//! Provides the foundational registry, certificates, secrets bridge,
//! and provisioning lifecycle for SWAL nodes (BaaS and SSH/VPS).

use serde::{Deserialize, Serialize};

pub mod audit;
pub mod cert;
pub mod provision;
pub mod registry;
pub mod secrets;

pub use audit::*;
pub use cert::*;
pub use provision::*;
pub use registry::*;
pub use secrets::*;

/// Supported node providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Supabase,
    Neon,
    Vps,
}

impl std::str::FromStr for Provider {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "supabase" => Ok(Provider::Supabase),
            "neon" => Ok(Provider::Neon),
            "vps" => Ok(Provider::Vps),
            _ => Err(anyhow::anyhow!(
                "Invalid provider '{}'. Expected 'supabase', 'neon', or 'vps'",
                s
            )),
        }
    }
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provider::Supabase => write!(f, "supabase"),
            Provider::Neon => write!(f, "neon"),
            Provider::Vps => write!(f, "vps"),
        }
    }
}

/// Visibility of a node in the mesh network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum NodeVisibility {
    Public,
    #[default]
    Private,
}

impl std::str::FromStr for NodeVisibility {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "public" => Ok(NodeVisibility::Public),
            "private" => Ok(NodeVisibility::Private),
            _ => Err(anyhow::anyhow!(
                "Invalid visibility '{}'. Expected 'public' or 'private'",
                s
            )),
        }
    }
}

impl std::fmt::Display for NodeVisibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeVisibility::Public => write!(f, "public"),
            NodeVisibility::Private => write!(f, "private"),
        }
    }
}

/// Lifecycle status of a provisioned node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Active,
    Degraded,
    Revoked,
    PartialRevocation,
}

impl std::str::FromStr for NodeStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "active" => Ok(NodeStatus::Active),
            "degraded" => Ok(NodeStatus::Degraded),
            "revoked" => Ok(NodeStatus::Revoked),
            "partial_revocation" | "partialrevocation" | "partial" => {
                Ok(NodeStatus::PartialRevocation)
            }
            _ => Err(anyhow::anyhow!("Invalid node status '{}'", s)),
        }
    }
}

impl std::fmt::Display for NodeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeStatus::Active => write!(f, "active"),
            NodeStatus::Degraded => write!(f, "degraded"),
            NodeStatus::Revoked => write!(f, "revoked"),
            NodeStatus::PartialRevocation => write!(f, "partial_revocation"),
        }
    }
}

/// Complete persistent record of a node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeRecord {
    pub node_id: String,
    pub provider: Provider,
    pub visibility: NodeVisibility,
    pub status: NodeStatus,
    pub pubkey: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cert: Option<cert::NodeCertificate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_key_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_id: Option<String>,
    pub created_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_heartbeat: Option<u64>,
}

/// Sanitized public view of a node (never contains secrets, leases, or private metadata).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublicNodeInfo {
    pub node_id: String,
    pub provider: Provider,
    pub status: NodeStatus,
    pub pubkey: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_heartbeat: Option<u64>,
}

impl From<&NodeRecord> for PublicNodeInfo {
    fn from(record: &NodeRecord) -> Self {
        Self {
            node_id: record.node_id.clone(),
            provider: record.provider,
            status: record.status,
            pubkey: record.pubkey.clone(),
            last_heartbeat: record.last_heartbeat,
        }
    }
}
