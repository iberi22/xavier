//! Mini-Expert Registry for local personal mini-experts.
//!
//! Manages persistent JSON storage for mini-experts metadata, including
//! domain segment, target language, security clearance, source dataset,
//! and local GGUF model path.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::settings::types::MiniExpertConfig;

/// Persistent record representing a personal mini-expert.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MiniExpertEntry {
    pub name: String,
    pub segment: String,
    pub language: String,
    pub clearance: u8,
    pub source_dataset: String,
    pub model_gguf_path: String,
    pub provider: String,
    pub endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

impl MiniExpertEntry {
    /// Converts a MiniExpertEntry to MiniExpertConfig for use in ProviderRouter.
    pub fn to_config(&self) -> MiniExpertConfig {
        MiniExpertConfig {
            name: self.name.clone(),
            provider: self.provider.clone(),
            endpoint: self.endpoint.clone(),
            api_key: self.api_key.clone(),
        }
    }
}

/// Persistent registry for mini-experts.
#[derive(Debug, Clone)]
pub struct MiniExpertRegistry {
    storage_path: PathBuf,
    entries: Arc<RwLock<Vec<MiniExpertEntry>>>,
}

impl MiniExpertRegistry {
    /// Returns the default storage path (`.xavier/mini_experts.json`).
    pub fn default_path() -> PathBuf {
        PathBuf::from(".xavier/mini_experts.json")
    }

    /// Creates a new MiniExpertRegistry backed by the given path and loads any existing records.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let storage_path = path.as_ref().to_path_buf();
        let registry = Self {
            storage_path,
            entries: Arc::new(RwLock::new(Vec::new())),
        };
        let _ = registry.reload();
        registry
    }

    /// Loads or creates the default registry at `.xavier/mini_experts.json`.
    pub fn load_default() -> Self {
        Self::new(Self::default_path())
    }

    /// Reloads entries from disk.
    pub fn reload(&self) -> Result<()> {
        if self.storage_path.exists() {
            let content = fs::read_to_string(&self.storage_path)
                .with_context(|| format!("Failed to read {}", self.storage_path.display()))?;
            if !content.trim().is_empty() {
                let loaded: Vec<MiniExpertEntry> = serde_json::from_str(&content)
                    .with_context(|| format!("Failed to parse {}", self.storage_path.display()))?;
                let mut guard = self
                    .entries
                    .write()
                    .map_err(|e| anyhow::anyhow!("RwLock poisoned: {}", e))?;
                *guard = loaded;
            }
        }
        Ok(())
    }

    /// Saves current entries to disk.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.storage_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }

        let guard = self
            .entries
            .read()
            .map_err(|e| anyhow::anyhow!("RwLock poisoned: {}", e))?;
        let json = serde_json::to_string_pretty(&*guard)?;
        fs::write(&self.storage_path, json)
            .with_context(|| format!("Failed to write {}", self.storage_path.display()))?;
        Ok(())
    }

    /// Registers or updates a mini-expert entry and persists to disk.
    pub fn register(&self, entry: MiniExpertEntry) -> Result<()> {
        {
            let mut guard = self
                .entries
                .write()
                .map_err(|e| anyhow::anyhow!("RwLock poisoned: {}", e))?;
            if let Some(pos) = guard.iter().position(|e| e.name == entry.name) {
                guard[pos] = entry;
            } else {
                guard.push(entry);
            }
        }
        self.save()
    }

    /// Retrieves a mini-expert entry by name.
    pub fn get(&self, name: &str) -> Option<MiniExpertEntry> {
        let guard = self.entries.read().ok()?;
        guard.iter().find(|e| e.name == name).cloned()
    }

    /// Returns a list of all registered mini-experts.
    pub fn list(&self) -> Vec<MiniExpertEntry> {
        self.entries
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Removes a mini-expert entry by name and persists to disk.
    pub fn delete(&self, name: &str) -> Result<bool> {
        let removed = {
            let mut guard = self
                .entries
                .write()
                .map_err(|e| anyhow::anyhow!("RwLock poisoned: {}", e))?;
            if let Some(pos) = guard.iter().position(|e| e.name == name) {
                guard.remove(pos);
                true
            } else {
                false
            }
        };

        if removed {
            self.save()?;
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_roundtrip() {
        let temp_dir = std::env::temp_dir().join(format!("xavier_test_mexp_{}", ulid::Ulid::new()));
        let db_file = temp_dir.join("mini_experts.json");

        let registry = MiniExpertRegistry::new(&db_file);
        assert!(registry.list().is_empty());

        let entry = MiniExpertEntry {
            name: "test-expert".to_string(),
            segment: "codebase/f12".to_string(),
            language: "es".to_string(),
            clearance: 1,
            source_dataset: "f12-dataset-v1".to_string(),
            model_gguf_path: "/models/f12-expert.gguf".to_string(),
            provider: "local".to_string(),
            endpoint: "http://localhost:11434/v1".to_string(),
            api_key: None,
        };

        registry.register(entry.clone()).unwrap();

        // Verify get & list
        let fetched = registry.get("test-expert");
        assert!(fetched.is_some());
        let fetched = fetched.unwrap();
        assert_eq!(fetched.segment, "codebase/f12");
        assert_eq!(fetched.language, "es");
        assert_eq!(fetched.clearance, 1);
        assert_eq!(fetched.source_dataset, "f12-dataset-v1");
        assert_eq!(fetched.model_gguf_path, "/models/f12-expert.gguf");

        assert_eq!(registry.list().len(), 1);

        // Re-load from new instance to verify disk persistence
        let registry_reloaded = MiniExpertRegistry::new(&db_file);
        assert_eq!(registry_reloaded.list().len(), 1);
        assert_eq!(registry_reloaded.get("test-expert").unwrap(), entry);

        // Delete entry
        assert!(registry.delete("test-expert").unwrap());
        assert!(registry.get("test-expert").is_none());
        assert!(registry.list().is_empty());

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
