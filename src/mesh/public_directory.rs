//! SWAL Public Directory and Information Tree
//!
//! Provides the public directory for discovering SWAL nodes and publishing/querying
//! their public manifests (InfoTree). The directory is persisted as a JSON file at
//! `data/mesh/public-directory.json` by default.

use crate::mesh::node::NodeId;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Information about a public node entry registered in the SWAL public directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicNodeEntry {
    pub node_id: NodeId,
    pub name: String,
    pub capabilities: Vec<String>,
    pub iroh_addr: Option<String>,
    pub last_heartbeat: u64,
    pub tree: InfoTree,
}

/// A node's public manifest (InfoTree) showing depth without full code retrieval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfoTree {
    pub repos: HashMap<String, RepoInfo>,
    pub memorias: MemoriaInfo,
    pub skills: SkillInfo,
}

/// Statistics and metadata about a repository indexed in the SWAL public registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoInfo {
    pub snapshot: String,
    pub symbols: u64,
    pub files: u64,
}

/// Public metrics about shared memories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoriaInfo {
    pub count: u64,
    pub kinds: Vec<String>,
}

/// Public metrics about skills.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInfo {
    pub count: u64,
}

/// File-backed public directory of SWAL nodes.
pub struct PublicDirectory {
    entries: HashMap<NodeId, PublicNodeEntry>,
    storage_path: PathBuf,
}

impl PublicDirectory {
    /// Load the public directory from the default path: `data/mesh/public-directory.json`
    pub fn load() -> Result<Self> {
        Self::load_from(PathBuf::from("data/mesh/public-directory.json"))
    }

    /// Load the public directory from a custom storage file path.
    pub fn load_from(storage_path: PathBuf) -> Result<Self> {
        if let Some(parent) = storage_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if !storage_path.exists() {
            return Ok(Self {
                entries: HashMap::new(),
                storage_path,
            });
        }

        let raw = std::fs::read_to_string(&storage_path)
            .context("Failed to read public directory file")?;
        let entries_vec: Vec<PublicNodeEntry> =
            serde_json::from_str(&raw).context("Failed to parse public directory JSON")?;

        let entries = entries_vec
            .into_iter()
            .map(|entry| (entry.node_id.clone(), entry))
            .collect();

        Ok(Self {
            entries,
            storage_path,
        })
    }

    /// Save the current directory state to disk.
    pub fn save(&self) -> Result<()> {
        let entries_vec: Vec<&PublicNodeEntry> = self.entries.values().collect();
        let json = serde_json::to_string_pretty(&entries_vec)?;
        std::fs::write(&self.storage_path, json)
            .context("Failed to write public directory file")?;
        Ok(())
    }

    /// Register a new node entry or overwrite an existing one.
    pub fn register_node(&mut self, entry: PublicNodeEntry) -> Result<()> {
        self.entries.insert(entry.node_id.clone(), entry);
        self.save()
    }

    /// Update the last_heartbeat timestamp of a registered node to the current epoch time.
    pub fn heartbeat(&mut self, node_id: &NodeId) -> Result<()> {
        if let Some(entry) = self.entries.get_mut(node_id) {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("System time is before Unix Epoch")?
                .as_secs();
            entry.last_heartbeat = now;
            self.save()?;
            Ok(())
        } else {
            anyhow::bail!("Node not found in public directory: {}", node_id);
        }
    }

    /// List all registered public nodes.
    pub fn list_nodes(&self) -> Vec<PublicNodeEntry> {
        self.entries.values().cloned().collect()
    }

    /// Retrieve the InfoTree manifest of a specific node.
    pub fn get_tree(&self, node_id: &NodeId) -> Option<InfoTree> {
        self.entries.get(node_id).map(|entry| entry.tree.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_mock_entry(node_id: &str) -> PublicNodeEntry {
        let mut repos = HashMap::new();
        repos.insert(
            "xavier".to_string(),
            RepoInfo {
                snapshot: "2026-08-08".to_string(),
                symbols: 21500,
                files: 625,
            },
        );
        repos.insert(
            "gestalt".to_string(),
            RepoInfo {
                snapshot: "2026-08-07".to_string(),
                symbols: 9500,
                files: 300,
            },
        );
        repos.insert(
            "maloca".to_string(),
            RepoInfo {
                snapshot: "2026-07-01".to_string(),
                symbols: 12000,
                files: 450,
            },
        );

        PublicNodeEntry {
            node_id: NodeId(node_id.to_string()),
            name: format!("Mock Node {}", node_id),
            capabilities: vec!["mesh-sync".to_string(), "rag".to_string()],
            iroh_addr: Some("iroh-mock-address-123".to_string()),
            last_heartbeat: 1000,
            tree: InfoTree {
                repos,
                memorias: MemoriaInfo {
                    count: 13000,
                    kinds: vec![
                        "decision".to_string(),
                        "state".to_string(),
                        "analysis".to_string(),
                    ],
                },
                skills: SkillInfo { count: 90 },
            },
        }
    }

    #[test]
    fn test_empty_directory_creation() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("directory.json");
        let public_dir = PublicDirectory::load_from(path).unwrap();
        assert_eq!(public_dir.list_nodes().len(), 0);
    }

    #[test]
    fn test_register_node() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("directory.json");
        let mut public_dir = PublicDirectory::load_from(path).unwrap();

        let entry = create_mock_entry("xv1-test-node-1");
        public_dir.register_node(entry).unwrap();

        assert_eq!(public_dir.list_nodes().len(), 1);
    }

    #[test]
    fn test_heartbeat() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("directory.json");
        let mut public_dir = PublicDirectory::load_from(path).unwrap();

        let node_id = NodeId("xv1-test-node-1".to_string());
        let entry = create_mock_entry(node_id.as_str());
        public_dir.register_node(entry).unwrap();

        let initial_heartbeat = public_dir.entries.get(&node_id).unwrap().last_heartbeat;
        public_dir.heartbeat(&node_id).unwrap();

        let updated_heartbeat = public_dir.entries.get(&node_id).unwrap().last_heartbeat;
        assert!(updated_heartbeat > initial_heartbeat);
    }

    #[test]
    fn test_list_nodes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("directory.json");
        let mut public_dir = PublicDirectory::load_from(path).unwrap();

        let entry_1 = create_mock_entry("xv1-test-node-1");
        let entry_2 = create_mock_entry("xv1-test-node-2");

        public_dir.register_node(entry_1).unwrap();
        public_dir.register_node(entry_2).unwrap();

        let list = public_dir.list_nodes();
        assert_eq!(list.len(), 2);
        assert!(list
            .iter()
            .any(|n| n.node_id == NodeId("xv1-test-node-1".to_string())));
        assert!(list
            .iter()
            .any(|n| n.node_id == NodeId("xv1-test-node-2".to_string())));
    }

    #[test]
    fn test_get_tree() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("directory.json");
        let mut public_dir = PublicDirectory::load_from(path).unwrap();

        let node_id = NodeId("xv1-test-node-1".to_string());
        let entry = create_mock_entry(node_id.as_str());
        public_dir.register_node(entry).unwrap();

        let tree = public_dir.get_tree(&node_id).unwrap();
        assert_eq!(tree.skills.count, 90);
        assert_eq!(tree.memorias.count, 13000);
        assert_eq!(tree.repos.get("xavier").unwrap().symbols, 21500);
    }

    #[test]
    fn test_persistence_save_load() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("directory.json");

        {
            let mut public_dir = PublicDirectory::load_from(path.clone()).unwrap();
            let entry = create_mock_entry("xv1-persisted");
            public_dir.register_node(entry).unwrap();
        }

        let public_dir_loaded = PublicDirectory::load_from(path).unwrap();
        assert_eq!(public_dir_loaded.list_nodes().len(), 1);
        let node_id = NodeId("xv1-persisted".to_string());
        assert!(public_dir_loaded.get_tree(&node_id).is_some());
    }

    #[test]
    fn test_register_node_updates_existing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("directory.json");
        let mut public_dir = PublicDirectory::load_from(path).unwrap();

        let node_id = NodeId("xv1-test-node-1".to_string());
        let mut entry = create_mock_entry(node_id.as_str());
        entry.name = "Original Name".to_string();
        public_dir.register_node(entry).unwrap();

        let mut updated_entry = create_mock_entry(node_id.as_str());
        updated_entry.name = "Updated Name".to_string();
        public_dir.register_node(updated_entry).unwrap();

        let list = public_dir.list_nodes();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Updated Name");
    }

    #[test]
    fn test_heartbeat_error_on_unknown_node() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("directory.json");
        let mut public_dir = PublicDirectory::load_from(path).unwrap();

        let node_id = NodeId("xv1-unknown".to_string());
        let res = public_dir.heartbeat(&node_id);
        assert!(res.is_err());
    }
}
