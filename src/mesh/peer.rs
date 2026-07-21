// SPDX-License-Identifier: MIT OR LICENSE-MESH
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

    /// Get information about a specific peer.
    pub fn get_peer(&self, node_id: &NodeId) -> Option<&PeerInfo> {
        self.peers.get(node_id)
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
        };

        registry.add_peer(peer).unwrap();
        assert_eq!(registry.list_peers().len(), 1);
        assert!(registry.get_peer(&node_id).is_some());

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
