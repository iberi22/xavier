//! Peer Registry — Persistent storage for trusted Xavier nodes
//!
//! Stores information about known peers, their public keys, and sync settings.
//! The registry is stored as a JSON file at `~/.config/xavier/mesh_peers.json`.

use crate::mesh::node::NodeId;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

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
    /// encoding) used by [`crate::mesh::iroh_transport::IrohTransport`] to dial
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

impl Default for PeerInfo {
    fn default() -> Self {
        Self {
            node_id: NodeId(String::new()),
            alias: None,
            endpoint_url: String::new(),
            public_key_hex: String::new(),
            added_at: chrono::Utc::now().timestamp(),
            last_seen_at: None,
            sync_enabled: true,
            is_cloud: false,
            iroh_addr: None,
            shared_workspace_ids: Vec::new(),
            shared_workspace_tokens: HashMap::new(),
            capabilities: Vec::new(),
        }
    }
}

impl PeerInfo {
    /// Check if peer is healthy based on last seen timestamp (unhealthy after 3 missed pings / 90 seconds)
    pub fn is_healthy(&self) -> bool {
        match self.last_seen_at {
            Some(last_seen) => {
                let now = chrono::Utc::now().timestamp();
                (now - last_seen) < 90
            }
            None => false,
        }
    }

    /// Check if peer has the required fields to be considered valid.
    pub fn is_valid(&self) -> bool {
        !self.node_id.0.is_empty() && !self.endpoint_url.is_empty()
    }
}

/// A persistent, file-backed registry of trusted peers.
pub struct PeerRegistry {
    peers: HashMap<NodeId, PeerInfo>,
    storage_path: PathBuf,
}

impl PeerRegistry {
    /// Load the registry from the default storage path.
    pub fn load() -> Result<Self> {
        let config_dir = if let Ok(val) = std::env::var("XAVIER_CONFIG_DIR") {
            PathBuf::from(val)
        } else {
            dirs::config_dir()
                .context("Could not determine config directory")?
                .join("xavier")
        };
        Self::load_from(config_dir.join("mesh_peers.json"))
    }

    /// Load the registry from a specific file path.
    pub fn load_from(storage_path: PathBuf) -> Result<Self> {
        if let Some(parent) = storage_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if !storage_path.exists() {
            return Ok(Self {
                peers: HashMap::new(),
                storage_path,
            });
        }

        let raw =
            std::fs::read_to_string(&storage_path).context("Failed to read peer registry file")?;
        let peers: Vec<PeerInfo> =
            serde_json::from_str(&raw).context("Failed to parse peer registry JSON")?;

        let peers_map = peers.into_iter().map(|p| (p.node_id.clone(), p)).collect();

        Ok(Self {
            peers: peers_map,
            storage_path,
        })
    }

    /// Save the registry to disk.
    pub fn save(&self) -> Result<()> {
        let peers_vec: Vec<&PeerInfo> = self.peers.values().collect();
        let json = serde_json::to_string_pretty(&peers_vec)?;
        std::fs::write(&self.storage_path, json).context("Failed to write peer registry file")?;
        Ok(())
    }

    /// Add or update a peer in the registry.
    pub fn add_peer(&mut self, peer: PeerInfo) -> Result<()> {
        self.peers.insert(peer.node_id.clone(), peer);
        self.save()
    }

    /// Remove a peer from the registry.
    pub fn remove_peer(&mut self, node_id: &NodeId) -> Result<()> {
        if self.peers.remove(node_id).is_some() {
            self.save()?;
        }
        Ok(())
    }

    /// List all registered peers.
    pub fn list_peers(&self) -> Vec<&PeerInfo> {
        self.peers.values().collect()
    }

    /// Return all peers as a slice.
    pub fn all_peers(&self) -> Vec<&PeerInfo> {
        self.peers.values().collect()
    }

    /// Get information about a specific peer.
    pub fn get_peer(&self, node_id: &NodeId) -> Option<&PeerInfo> {
        self.peers.get(node_id)
    }

    /// Get a mutable reference to a specific peer.
    pub fn get_peer_mut(&mut self, node_id: &NodeId) -> Option<&mut PeerInfo> {
        self.peers.get_mut(node_id)
    }

    /// Get all peers that are currently unhealthy (not seen in the last 90 seconds)
    pub fn get_unhealthy_peers(&self) -> Vec<&PeerInfo> {
        let now = chrono::Utc::now().timestamp();
        self.peers
            .values()
            .filter(|p| match p.last_seen_at {
                Some(last_seen) => (now - last_seen) >= 90,
                None => true,
            })
            .collect()
    }

    /// Remove all peers that are unhealthy (not seen in the last 90 seconds)
    pub fn remove_unhealthy_peers(&mut self) -> Result<()> {
        let unhealthy_ids: Vec<NodeId> = self
            .get_unhealthy_peers()
            .into_iter()
            .map(|p| p.node_id.clone())
            .collect();

        for id in unhealthy_ids {
            self.remove_peer(&id)?;
        }
        Ok(())
    }

    /// Select a random healthy peer with the "maintenance" capability.
    /// If no peer explicitly has "maintenance", any healthy peer can be considered if open.
    pub fn select_random_maintainer(&self) -> Option<PeerInfo> {
        use rand::seq::SliceRandom;

        let maintainers: Vec<PeerInfo> = self
            .peers
            .values()
            .filter(|p| {
                p.is_healthy()
                    && (p.capabilities.iter().any(|c| c == "maintenance")
                        || p.capabilities.is_empty())
            })
            .cloned()
            .collect();

        if maintainers.is_empty() {
            None
        } else {
            let mut rng = rand::thread_rng();
            maintainers.choose(&mut rng).cloned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_peer_registry_lifecycle() {
        let dir = tempdir().unwrap();
        let storage_path = dir.path().join("peers.json");

        let mut registry = PeerRegistry {
            peers: HashMap::new(),
            storage_path: storage_path.clone(),
        };

        let node_id = NodeId("xv1-test-peer".to_string());
        let peer = PeerInfo {
            node_id: node_id.clone(),
            alias: Some("Test Peer".to_string()),
            endpoint_url: "http://localhost:8006".to_string(),
            public_key_hex: "aabbccdd".to_string(),
            added_at: 1000,
            last_seen_at: None,
            sync_enabled: true,
            is_cloud: false,
            iroh_addr: None,
            shared_workspace_ids: Vec::new(),
            shared_workspace_tokens: HashMap::new(),
            capabilities: vec!["maintenance".to_string()],
        };

        registry.add_peer(peer).unwrap();
        assert_eq!(registry.list_peers().len(), 1);
        assert!(registry.get_peer(&node_id).is_some());

        // Test random maintainer selection
        let mut healthy_peer = registry.get_peer(&node_id).unwrap().clone();
        healthy_peer.last_seen_at = Some(chrono::Utc::now().timestamp());
        registry.add_peer(healthy_peer).unwrap();

        let chosen = registry.select_random_maintainer();
        assert!(chosen.is_some());
        assert_eq!(chosen.unwrap().node_id, node_id);

        // Test persistence
        let _reloaded = PeerRegistry {
            peers: HashMap::new(),
            storage_path,
        };
        // Manually trigger reload by re-implementing part of load() logic for test
        let raw = std::fs::read_to_string(&registry.storage_path).unwrap();
        let peers: Vec<PeerInfo> = serde_json::from_str(&raw).unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].node_id, node_id);

        registry.remove_peer(&node_id).unwrap();
        assert_eq!(registry.list_peers().len(), 0);
    }
}
