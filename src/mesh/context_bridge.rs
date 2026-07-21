// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Context Bridge — Semantic relations between isolated databases
//!
//! Allows defining connections between documents residing in separate databases
//! without copying data, using signed, lazy-resolved references.

use crate::settings::XavierSettings;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// The kind of relation created by the context bridge.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BridgeKind {
    SharedReference,
}

/// Defines a semantic relationship between two databases in the mesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBridge {
    pub id: String,
    pub source_db: String,
    pub source_namespace: String,
    pub target_db: String,
    pub bridge_kind: BridgeKind,
    pub acl: Vec<String>,
}

/// A registry for managing `ContextBridge` entries, persisted in `{XAVIER_DATA_DIR}/context_bridges.json`.
pub struct BridgeRegistry {
    bridges: HashMap<String, ContextBridge>,
    storage_path: PathBuf,
}

impl BridgeRegistry {
    /// Load the registry from the default storage path.
    pub fn load() -> Result<Self> {
        let data_dir = XavierSettings::resolve_data_dir();
        Self::load_from(data_dir.join("context_bridges.json"))
    }

    /// Load the registry from a specific file path.
    pub fn load_from(storage_path: PathBuf) -> Result<Self> {
        if let Some(parent) = storage_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if !storage_path.exists() {
            return Ok(Self {
                bridges: HashMap::new(),
                storage_path,
            });
        }

        let raw = std::fs::read_to_string(&storage_path)
            .context("Failed to read context bridges file")?;

        let bridges: Vec<ContextBridge> = serde_json::from_str(&raw)
            .context("Failed to parse context bridges JSON")?;

        let bridges_map = bridges.into_iter().map(|b| (b.id.clone(), b)).collect();

        Ok(Self {
            bridges: bridges_map,
            storage_path,
        })
    }

    /// Save the registry to disk.
    pub fn save(&self) -> Result<()> {
        let bridges_vec: Vec<&ContextBridge> = self.bridges.values().collect();
        let json = serde_json::to_string_pretty(&bridges_vec)?;
        std::fs::write(&self.storage_path, json)
            .context("Failed to write context bridges file")?;
        Ok(())
    }

    /// Add or update a bridge in the registry.
    pub fn add_bridge(&mut self, bridge: ContextBridge) -> Result<()> {
        self.bridges.insert(bridge.id.clone(), bridge);
        self.save()
    }

    /// Remove a bridge from the registry.
    pub fn remove_bridge(&mut self, id: &str) -> Result<()> {
        if self.bridges.remove(id).is_some() {
            self.save()?;
        }
        Ok(())
    }

    /// List all registered bridges.
    pub fn list_bridges(&self) -> Vec<ContextBridge> {
        self.bridges.values().cloned().collect()
    }

    /// Get a specific bridge by its ID.
    pub fn get_bridge(&self, id: &str) -> Option<&ContextBridge> {
        self.bridges.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_context_bridge_lifecycle() {
        let dir = tempdir().unwrap();
        let storage_path = dir.path().join("context_bridges.json");

        let mut registry = BridgeRegistry {
            bridges: HashMap::new(),
            storage_path: storage_path.clone(),
        };

        let bridge = ContextBridge {
            id: "test-bridge-1".to_string(),
            source_db: "Personal".to_string(),
            source_namespace: "finance".to_string(),
            target_db: "Family".to_string(),
            bridge_kind: BridgeKind::SharedReference,
            acl: vec!["read".to_string(), "write".to_string()],
        };

        registry.add_bridge(bridge.clone()).unwrap();
        assert_eq!(registry.list_bridges().len(), 1);
        assert!(registry.get_bridge("test-bridge-1").is_some());

        // Test persistence reload
        let reloaded = BridgeRegistry::load_from(storage_path).unwrap();
        assert_eq!(reloaded.list_bridges().len(), 1);
        assert_eq!(reloaded.get_bridge("test-bridge-1").unwrap().source_db, "Personal");

        registry.remove_bridge("test-bridge-1").unwrap();
        assert_eq!(registry.list_bridges().len(), 0);
    }
}
