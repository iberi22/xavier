use crate::enterprise::rbac::{Permission, Role};
use crate::memory::schema::ClearanceLevel;
use crate::mesh::node::NodeId;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NamespaceAclEntry {
    pub namespace_pattern: String,
    pub permissions: Vec<Permission>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeAclEntry {
    pub role: Role,
    pub clearance: ClearanceLevel,
    #[serde(default)]
    pub namespaces: Option<Vec<String>>,
    #[serde(default)]
    pub public_key_hex: String,
    #[serde(default)]
    pub namespace_acl: Option<Vec<NamespaceAclEntry>>,
}

pub struct MeshAcl {
    entries: HashMap<NodeId, NodeAclEntry>,
    storage_path: PathBuf,
}

impl MeshAcl {
    pub fn load() -> Result<Self> {
        let config_dir = if let Ok(val) = std::env::var("XAVIER_CONFIG_DIR") {
            PathBuf::from(val)
        } else {
            dirs::config_dir()
                .context("Could not determine config directory")?
                .join("xavier")
        };
        Self::load_from(config_dir.join("mesh_acl.json"))
    }

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

        let raw = std::fs::read_to_string(&storage_path).context("Failed to read mesh ACL file")?;
        let entries: HashMap<String, NodeAclEntry> =
            serde_json::from_str(&raw).context("Failed to parse mesh ACL JSON")?;

        let entries_map = entries.into_iter().map(|(k, v)| (NodeId(k), v)).collect();

        Ok(Self {
            entries: entries_map,
            storage_path,
        })
    }

    pub fn save(&self) -> Result<()> {
        let entries_map: HashMap<String, &NodeAclEntry> =
            self.entries.iter().map(|(k, v)| (k.0.clone(), v)).collect();
        let json = serde_json::to_string_pretty(&entries_map)?;
        std::fs::write(&self.storage_path, json).context("Failed to write mesh ACL file")?;
        Ok(())
    }

    pub fn set_entry(&mut self, node_id: NodeId, entry: NodeAclEntry) -> Result<()> {
        self.entries.insert(node_id, entry);
        self.save()
    }

    pub fn get_entry(&self, node_id: &NodeId) -> Option<&NodeAclEntry> {
        self.entries.get(node_id)
    }

    pub fn remove_entry(&mut self, node_id: &NodeId) -> Result<()> {
        if self.entries.remove(node_id).is_some() {
            self.save()?;
        }
        Ok(())
    }
}
