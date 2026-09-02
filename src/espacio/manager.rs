//! Space manager — creates and isolates Spaces (T-01)
//!
//! Each Space is a dedicated directory `data/spaces/{space_id}/` with its own
//! SQLite_vec store, belief graph and timeline. Isolation is enforced by
//! distinct WorkspaceConfig.id and separate storage paths.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::workspace::config::{PlanTier, WorkspaceConfig};
use crate::workspace::state::WorkspaceState;

/// Payload for creating a new Space
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSpaceRequest {
    /// Unique space identifier (e.g., esp_01H...)
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description
    #[serde(default)]
    pub description: String,
    /// Owner node id (admin)
    pub owner_node: String,
    /// Whether the space is public
    #[serde(default)]
    pub is_public: bool,
}

/// Information about a Space (Telegram-like group)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceInfo {
    /// Unique space identifier (e.g., esp_01H...)
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description
    pub description: String,
    /// Owner node id (admin)
    pub owner_node: String,
    /// Whether the space is public (listed in Directory Chain)
    pub is_public: bool,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Namespace: xavier://{space_id}/{appId}/{instanceId}
    pub namespace: String,
    /// Storage path
    pub storage_path: PathBuf,
}

/// Errors for Space operations
#[derive(Debug, thiserror::Error)]
pub enum SpaceError {
    #[error("Space {0} already exists")]
    AlreadyExists(String),
    #[error("Space {0} not found")]
    NotFound(String),
    #[error("Invalid space id: {0}")]
    InvalidId(String),
    #[error("Storage error: {0}")]
    Storage(String),
}

/// Manages isolated Spaces. Wraps WorkspaceRegistry concept but keeps
/// Spaces separate from legacy single-workspace flow until migration.
#[derive(Debug, Default)]
pub struct SpaceManager {
    spaces: Arc<RwLock<HashMap<String, SpaceInfo>>>,
    base_dir: PathBuf,
}

impl SpaceManager {
    /// Create a manager rooted at `base_dir` (e.g., data/spaces)
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            spaces: Arc::new(RwLock::new(HashMap::new())),
            base_dir: base_dir.into(),
        }
    }

    /// Validate space id format (alphanumeric, dash, underscore)
    fn validate_id(id: &str) -> Result<()> {
        if id.is_empty() || id.len() > 64 {
            return Err(anyhow!(SpaceError::InvalidId(id.to_string())));
        }
        if !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(anyhow!(SpaceError::InvalidId(id.to_string())));
        }
        Ok(())
    }

    /// Build namespace for a space
    pub fn namespace_for(space_id: &str, app_id: &str, instance_id: &str) -> String {
        format!("xavier://{}/{}/{}", space_id, app_id, instance_id)
    }

    /// Create a new isolated Space
    pub async fn create(
        &self,
        id: String,
        name: String,
        description: String,
        owner_node: String,
        is_public: bool,
    ) -> Result<SpaceInfo> {
        Self::validate_id(&id)?;
        let mut guard = self.spaces.write().await;
        if guard.contains_key(&id) {
            return Err(anyhow!(SpaceError::AlreadyExists(id)));
        }
        let storage_path = self.base_dir.join(&id);
        tokio::fs::create_dir_all(&storage_path)
            .await
            .map_err(|e| anyhow!(SpaceError::Storage(e.to_string())))?;

        let info = SpaceInfo {
            id: id.clone(),
            name,
            description,
            owner_node,
            is_public,
            created_at: Utc::now(),
            namespace: Self::namespace_for(&id, "xavier", "default"),
            storage_path,
        };
        guard.insert(id, info.clone());
        Ok(info)
    }

    /// Get a Space by id
    pub async fn get(&self, id: &str) -> Result<SpaceInfo> {
        let guard = self.spaces.read().await;
        guard
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!(SpaceError::NotFound(id.to_string())))
    }

    /// List all Spaces
    pub async fn list(&self) -> Vec<SpaceInfo> {
        let guard = self.spaces.read().await;
        let mut v: Vec<_> = guard.values().cloned().collect();
        v.sort_by_key(|a| a.created_at);
        v
    }

    /// Delete a Space (removes from registry and deletes storage dir)
    pub async fn delete(&self, id: &str) -> Result<()> {
        let mut guard = self.spaces.write().await;
        let info = guard
            .remove(id)
            .ok_or_else(|| anyhow!(SpaceError::NotFound(id.to_string())))?;
        // Best-effort delete directory
        let _ = tokio::fs::remove_dir_all(&info.storage_path).await;
        Ok(())
    }

    /// Check isolation: two spaces never share storage path
    pub async fn are_isolated(&self, a: &str, b: &str) -> bool {
        let guard = self.spaces.read().await;
        match (guard.get(a), guard.get(b)) {
            (Some(sa), Some(sb)) => sa.storage_path != sb.storage_path && sa.id != sb.id,
            _ => false,
        }
    }

    /// Storage path for a space
    pub fn storage_path(&self, id: &str) -> PathBuf {
        self.base_dir.join(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_and_isolate() {
        let mgr = SpaceManager::new(std::env::temp_dir().join("xavier_spaces_test"));
        let a = mgr
            .create(
                "esp_a".into(),
                "A".into(),
                "desc".into(),
                "xv1_owner".into(),
                false,
            )
            .await
            .unwrap();
        let b = mgr
            .create(
                "esp_b".into(),
                "B".into(),
                "desc".into(),
                "xv1_owner".into(),
                true,
            )
            .await
            .unwrap();
        assert_ne!(a.storage_path, b.storage_path);
        assert!(mgr.are_isolated("esp_a", "esp_b").await);
        assert!(mgr.get("esp_a").await.is_ok());
        assert_eq!(mgr.list().await.len(), 2);
        mgr.delete("esp_a").await.unwrap();
        assert!(mgr.get("esp_a").await.is_err());
        let _ = mgr.delete("esp_b").await;
    }

    #[tokio::test]
    async fn reject_duplicate_and_invalid() {
        let mgr = SpaceManager::new(std::env::temp_dir().join("xavier_spaces_test2"));
        mgr.create("esp_x".into(), "X".into(), "".into(), "n1".into(), false)
            .await
            .unwrap();
        assert!(mgr
            .create("esp_x".into(), "X".into(), "".into(), "n1".into(), false)
            .await
            .is_err());
        assert!(mgr
            .create("bad/id".into(), "X".into(), "".into(), "n1".into(), false)
            .await
            .is_err());
        let _ = mgr.delete("esp_x").await;
    }

    #[test]
    fn namespace_format() {
        let ns = SpaceManager::namespace_for("esp_01H", "myapp", "inst1");
        assert_eq!(ns, "xavier://esp_01H/myapp/inst1");
    }
}
