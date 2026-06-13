//! Invite Code Management — Persistent storage for mesh invite codes
//!
//! Handles the generation, storage, and validation of invite codes which
//! allow peers to register with specific permissions.

use crate::memory::schema::ClearanceLevel;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

/// A mesh invite code and its associated permissions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshInvite {
    pub code: String,
    pub max_clearance: ClearanceLevel,
    pub allowed_namespaces: Vec<String>,
    pub allowed_paths: Vec<String>,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub max_uses: Option<u32>,
    pub uses: u32,
}

/// A persistent, file-backed registry of invite codes.
pub struct InviteRegistry {
    invites: HashMap<String, MeshInvite>,
    storage_path: PathBuf,
}

impl InviteRegistry {
    /// Load the registry from the default storage path.
    pub fn load() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .context("Could not determine config directory")?
            .join("xavier");
        Self::load_from(config_dir.join("mesh_invites.json"))
    }

    /// Load the registry from a specific file path.
    pub fn load_from(storage_path: PathBuf) -> Result<Self> {
        if let Some(parent) = storage_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if !storage_path.exists() {
            return Ok(Self {
                invites: HashMap::new(),
                storage_path,
            });
        }

        let raw =
            std::fs::read_to_string(&storage_path).context("Failed to read invite registry file")?;
        let invites: Vec<MeshInvite> =
            serde_json::from_str(&raw).context("Failed to parse invite registry JSON")?;

        let invites_map = invites.into_iter().map(|i| (i.code.clone(), i)).collect();

        Ok(Self {
            invites: invites_map,
            storage_path,
        })
    }

    /// Save the registry to disk.
    pub fn save(&self) -> Result<()> {
        let invites_vec: Vec<&MeshInvite> = self.invites.values().collect();
        let json = serde_json::to_string_pretty(&invites_vec)?;
        std::fs::write(&self.storage_path, json).context("Failed to write invite registry file")?;
        Ok(())
    }

    /// Create a new invite code with specific permissions.
    pub fn create_invite(
        &mut self,
        max_clearance: ClearanceLevel,
        allowed_namespaces: Vec<String>,
        allowed_paths: Vec<String>,
        expires_at: Option<i64>,
        max_uses: Option<u32>,
    ) -> Result<String> {
        let code = format!("XV1-{}", Uuid::new_v4().to_string()[..8].to_uppercase());
        let invite = MeshInvite {
            code: code.clone(),
            max_clearance,
            allowed_namespaces,
            allowed_paths,
            created_at: chrono::Utc::now().timestamp(),
            expires_at,
            max_uses,
            uses: 0,
        };
        self.invites.insert(code.clone(), invite);
        self.save()?;
        Ok(code)
    }

    /// Validate an invite code and increment its usage counter.
    pub fn use_invite(&mut self, code: &str) -> Result<MeshInvite> {
        let invite = self
            .invites
            .get_mut(code)
            .context("Invalid or expired invite code")?;

        let now = chrono::Utc::now().timestamp();
        if let Some(expiry) = invite.expires_at {
            if now > expiry {
                anyhow::bail!("Invite code has expired");
            }
        }

        if let Some(max) = invite.max_uses {
            if invite.uses >= max {
                anyhow::bail!("Invite code has reached its maximum usage limit");
            }
        }

        invite.uses += 1;
        let result = invite.clone();
        self.save()?;
        Ok(result)
    }

    /// List all active invites.
    pub fn list_invites(&self) -> Vec<&MeshInvite> {
        self.invites.values().collect()
    }

    /// Remove an invite code.
    pub fn remove_invite(&mut self, code: &str) -> Result<()> {
        if self.invites.remove(code).is_some() {
            self.save()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_invite_lifecycle() {
        let dir = tempdir().unwrap();
        let storage_path = dir.path().join("invites.json");

        let mut registry = InviteRegistry {
            invites: HashMap::new(),
            storage_path,
        };

        let code = registry
            .create_invite(
                ClearanceLevel::Secret,
                vec!["ns1".to_string()],
                vec!["path1".to_string()],
                None,
                Some(1),
            )
            .unwrap();

        assert_eq!(registry.list_invites().len(), 1);

        let invite = registry.use_invite(&code).unwrap();
        assert_eq!(invite.max_clearance, ClearanceLevel::Secret);
        assert_eq!(invite.uses, 1);

        // Should fail on second use (max_uses = 1)
        assert!(registry.use_invite(&code).is_err());

        registry.remove_invite(&code).unwrap();
        assert_eq!(registry.list_invites().len(), 0);
    }
}
