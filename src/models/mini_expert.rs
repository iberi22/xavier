use std::collections::HashMap;
use std::path::PathBuf;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::memory::schema::ClearanceLevel;

/// Represents a personalized trained model (MiniExpert) inside Xavier.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MiniExpert {
    pub id: String,
    pub name: String,
    pub segment: String,
    pub language: String,
    pub clearance: ClearanceLevel,
    pub model_path: String,
    pub dataset_id: String,
    pub created_at: DateTime<Utc>,
}

/// Registry to load, save, and manage MiniExperts.
/// Follows the JSON load/save pattern from MeshAcl.
pub struct MiniExpertRegistry {
    pub experts: HashMap<String, MiniExpert>,
    pub storage_path: PathBuf,
}

impl MiniExpertRegistry {
    /// Creates a new, empty registry with a custom storage path.
    pub fn new(storage_path: PathBuf) -> Self {
        Self {
            experts: HashMap::new(),
            storage_path,
        }
    }

    /// Loads the registry from the default location.
    pub fn load() -> Result<Self> {
        let config_dir = if let Ok(val) = std::env::var("XAVIER_CONFIG_DIR") {
            PathBuf::from(val)
        } else {
            dirs::config_dir()
                .context("Could not determine config directory")?
                .join("xavier")
        };
        Self::load_from(config_dir.join("mini_experts.json"))
    }

    /// Loads the registry from a specific path.
    pub fn load_from(storage_path: PathBuf) -> Result<Self> {
        if let Some(parent) = storage_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if !storage_path.exists() {
            return Ok(Self {
                experts: HashMap::new(),
                storage_path,
            });
        }

        let raw = std::fs::read_to_string(&storage_path)
            .context("Failed to read mini experts registry file")?;

        let experts: HashMap<String, MiniExpert> = serde_json::from_str(&raw)
            .context("Failed to parse mini experts registry JSON")?;

        Ok(Self {
            experts,
            storage_path,
        })
    }

    /// Saves the current registry to disk.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.storage_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.experts)
            .context("Failed to serialize mini experts registry")?;
        std::fs::write(&self.storage_path, json)
            .context("Failed to write mini experts registry file")?;
        Ok(())
    }

    /// Registers a new MiniExpert (or overwrites an existing one) and saves the change.
    pub fn register(&mut self, expert: MiniExpert) -> Result<()> {
        self.experts.insert(expert.id.clone(), expert);
        self.save()
    }

    /// Gets a reference to a registered MiniExpert by ID.
    pub fn get(&self, id: &str) -> Option<&MiniExpert> {
        self.experts.get(id)
    }

    /// Removes a MiniExpert by ID and saves the change.
    pub fn remove(&mut self, id: &str) -> Result<Option<MiniExpert>> {
        if let Some(expert) = self.experts.remove(id) {
            self.save()?;
            Ok(Some(expert))
        } else {
            Ok(None)
        }
    }

    /// Lists all registered MiniExperts in the registry.
    pub fn list(&self) -> Vec<&MiniExpert> {
        self.experts.values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_mini_expert_instantiation() {
        let now = Utc::now();
        let expert = MiniExpert {
            id: "expert-123".to_string(),
            name: "Iroh Transport Expert".to_string(),
            segment: "mesh_transport".to_string(),
            language: "es".to_string(),
            clearance: ClearanceLevel::Secret,
            model_path: "/home/user/models/iroh_expert.gguf".to_string(),
            dataset_id: "dataset-abc".to_string(),
            created_at: now,
        };

        assert_eq!(expert.id, "expert-123");
        assert_eq!(expert.name, "Iroh Transport Expert");
        assert_eq!(expert.segment, "mesh_transport");
        assert_eq!(expert.language, "es");
        assert_eq!(expert.clearance, ClearanceLevel::Secret);
        assert_eq!(expert.model_path, "/home/user/models/iroh_expert.gguf");
        assert_eq!(expert.dataset_id, "dataset-abc");
        assert_eq!(expert.created_at, now);
    }

    #[test]
    fn test_registry_new() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("mini_experts.json");
        let registry = MiniExpertRegistry::new(storage_path.clone());

        assert!(registry.experts.is_empty());
        assert_eq!(registry.storage_path, storage_path);
    }

    #[test]
    fn test_registry_register_and_get() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("mini_experts.json");
        let mut registry = MiniExpertRegistry::new(storage_path);

        let expert = MiniExpert {
            id: "expert-456".to_string(),
            name: "Gara-g Marketplace Expert".to_string(),
            segment: "marketplace".to_string(),
            language: "es".to_string(),
            clearance: ClearanceLevel::Confidential,
            model_path: "/models/gara_expert.gguf".to_string(),
            dataset_id: "dataset-xyz".to_string(),
            created_at: Utc::now(),
        };

        registry.register(expert.clone()).unwrap();

        let retrieved = registry.get("expert-456").unwrap();
        assert_eq!(retrieved, &expert);
    }

    #[test]
    fn test_registry_save_and_load() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("mini_experts.json");
        let mut registry = MiniExpertRegistry::new(storage_path.clone());

        let expert = MiniExpert {
            id: "expert-789".to_string(),
            name: "Xavier Core Expert".to_string(),
            segment: "core_dev".to_string(),
            language: "en".to_string(),
            clearance: ClearanceLevel::TopSecret,
            model_path: "/models/xavier_core.gguf".to_string(),
            dataset_id: "dataset-123".to_string(),
            created_at: Utc::now(),
        };

        registry.register(expert.clone()).unwrap();

        // Load from disk in a fresh registry instance
        let registry_loaded = MiniExpertRegistry::load_from(storage_path).unwrap();
        let loaded_expert = registry_loaded.get("expert-789").unwrap();
        assert_eq!(loaded_expert.id, expert.id);
        assert_eq!(loaded_expert.name, expert.name);
        assert_eq!(loaded_expert.segment, expert.segment);
        assert_eq!(loaded_expert.language, expert.language);
        assert_eq!(loaded_expert.clearance, expert.clearance);
        assert_eq!(loaded_expert.model_path, expert.model_path);
        assert_eq!(loaded_expert.dataset_id, expert.dataset_id);
    }

    #[test]
    fn test_registry_remove() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("mini_experts.json");
        let mut registry = MiniExpertRegistry::new(storage_path.clone());

        let expert = MiniExpert {
            id: "expert-to-remove".to_string(),
            name: "Temporary Expert".to_string(),
            segment: "temp".to_string(),
            language: "en".to_string(),
            clearance: ClearanceLevel::Unclassified,
            model_path: "/models/temp.gguf".to_string(),
            dataset_id: "dataset-temp".to_string(),
            created_at: Utc::now(),
        };

        registry.register(expert).unwrap();
        assert!(registry.get("expert-to-remove").is_some());

        let removed = registry.remove("expert-to-remove").unwrap();
        assert!(removed.is_some());
        assert!(registry.get("expert-to-remove").is_none());

        // Verify it was updated on disk too
        let registry_loaded = MiniExpertRegistry::load_from(storage_path).unwrap();
        assert!(registry_loaded.get("expert-to-remove").is_none());
    }

    #[test]
    fn test_registry_list() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("mini_experts.json");
        let mut registry = MiniExpertRegistry::new(storage_path);

        let expert1 = MiniExpert {
            id: "expert-1".to_string(),
            name: "Expert 1".to_string(),
            segment: "segment1".to_string(),
            language: "es".to_string(),
            clearance: ClearanceLevel::Unclassified,
            model_path: "/models/1.gguf".to_string(),
            dataset_id: "ds1".to_string(),
            created_at: Utc::now(),
        };

        let expert2 = MiniExpert {
            id: "expert-2".to_string(),
            name: "Expert 2".to_string(),
            segment: "segment2".to_string(),
            language: "en".to_string(),
            clearance: ClearanceLevel::Secret,
            model_path: "/models/2.gguf".to_string(),
            dataset_id: "ds2".to_string(),
            created_at: Utc::now(),
        };

        registry.register(expert1).unwrap();
        registry.register(expert2).unwrap();

        let list = registry.list();
        assert_eq!(list.len(), 2);
        let ids: Vec<&str> = list.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"expert-1"));
        assert!(ids.contains(&"expert-2"));
    }

    #[test]
    fn test_registry_load_non_existent() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("non_existent_file.json");
        assert!(!storage_path.exists());

        let registry = MiniExpertRegistry::load_from(storage_path).unwrap();
        assert!(registry.experts.is_empty());
    }
}
