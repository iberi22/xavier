use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Permissions configuration for an Information Group.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupAcl {
    pub read: bool,
    pub write: bool,
    pub audit: bool,
}

/// A structured group of information containing identification, names,
/// members allowed to access, and the respective Access Control List (ACL).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InfoGroup {
    pub id: String,
    pub name: String,
    pub members: Vec<String>,
    pub acl: GroupAcl,
}

/// Central registry managing all `InfoGroup` elements, persistence,
/// membership modifications, and central access control enforcement.
#[derive(Debug, Clone)]
pub struct GroupRegistry {
    groups: HashMap<String, InfoGroup>,
    storage_path: PathBuf,
}

impl GroupRegistry {
    /// Load the GroupRegistry from the default configuration path.
    pub fn load() -> Result<Self> {
        Self::load_from(PathBuf::from("data/security/groups.json"))
    }

    /// Load the GroupRegistry from a specified storage path.
    pub fn load_from<P: AsRef<Path>>(storage_path: P) -> Result<Self> {
        let path = storage_path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if !path.exists() {
            return Ok(Self {
                groups: HashMap::new(),
                storage_path: path,
            });
        }

        let raw = std::fs::read_to_string(&path).context("Failed to read groups storage file")?;

        let groups = if raw.trim().is_empty() {
            HashMap::new()
        } else {
            serde_json::from_str(&raw).context("Failed to parse groups JSON storage")?
        };

        Ok(Self {
            groups,
            storage_path: path,
        })
    }

    /// Save the registry to the configured storage path.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.storage_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.groups)?;
        std::fs::write(&self.storage_path, json).context("Failed to write groups storage file")?;
        Ok(())
    }

    /// Create or update an information group.
    pub fn create(&mut self, group: InfoGroup) -> Result<()> {
        self.groups.insert(group.id.clone(), group);
        self.save()
    }

    /// Add a member to a group if they are not already a member.
    /// Returns Ok(true) if the member was added, Ok(false) if already there,
    /// or an Err if the group does not exist.
    pub fn join(&mut self, group_id: &str, member_id: &str) -> Result<bool> {
        if let Some(group) = self.groups.get_mut(group_id) {
            if !group.members.contains(&member_id.to_string()) {
                group.members.push(member_id.to_string());
                self.save()?;
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Err(anyhow!("Group with ID '{}' not found", group_id))
        }
    }

    /// Retrieve an information group by its ID.
    pub fn get_group(&self, group_id: &str) -> Option<&InfoGroup> {
        self.groups.get(group_id)
    }

    /// Remove a group by its ID. Returns Ok(true) if it existed and was removed, Ok(false) otherwise.
    pub fn remove_group(&mut self, group_id: &str) -> Result<bool> {
        if self.groups.remove(group_id).is_some() {
            self.save()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Central enforcement check: verify if a member has a specific permission on a group.
    pub fn check_access(&self, group_id: &str, member_id: &str, action: &str) -> bool {
        let group = match self.groups.get(group_id) {
            Some(g) => g,
            None => return false,
        };

        if !group.members.contains(&member_id.to_string()) {
            return false;
        }

        match action.to_lowercase().as_str() {
            "read" => group.acl.read,
            "write" => group.acl.write,
            "audit" => group.acl.audit,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn make_test_group(id: &str, name: &str, read: bool, write: bool, audit: bool) -> InfoGroup {
        InfoGroup {
            id: id.to_string(),
            name: name.to_string(),
            members: vec![],
            acl: GroupAcl { read, write, audit },
        }
    }

    #[test]
    fn test_create_group() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let group = make_test_group("core-dev", "Core Developers", true, true, false);
        let create_res = registry.create(group.clone());
        assert!(create_res.is_ok());

        let retrieved = registry.get_group("core-dev");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "Core Developers");
    }

    #[test]
    fn test_join_group() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let group = make_test_group("core-dev", "Core Developers", true, true, false);
        registry.create(group).unwrap();

        let join_res1 = registry.join("core-dev", "bela");
        assert!(join_res1.unwrap());

        // Try joining again - should return false as already joined
        let join_res2 = registry.join("core-dev", "bela");
        assert!(!join_res2.unwrap());

        let group = registry.get_group("core-dev").unwrap();
        assert_eq!(group.members, vec!["bela".to_string()]);
    }

    #[test]
    fn test_join_nonexistent_group() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let join_res = registry.join("nonexistent", "bela");
        assert!(join_res.is_err());
    }

    #[test]
    fn test_check_access_read_allowed() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let group = make_test_group("readers", "Read Only Group", true, false, false);
        registry.create(group).unwrap();
        registry.join("readers", "bela").unwrap();

        assert!(registry.check_access("readers", "bela", "read"));
    }

    #[test]
    fn test_check_access_read_denied() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let group = make_test_group("no-read", "No Read Group", false, true, false);
        registry.create(group).unwrap();
        registry.join("no-read", "bela").unwrap();

        assert!(!registry.check_access("no-read", "bela", "read"));
    }

    #[test]
    fn test_check_access_write_allowed() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let group = make_test_group("writers", "Write Group", true, true, false);
        registry.create(group).unwrap();
        registry.join("writers", "bela").unwrap();

        assert!(registry.check_access("writers", "bela", "write"));
    }

    #[test]
    fn test_check_access_write_denied() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        // Target test requirement: membre sin permiso write -> check_access(write) = false
        let group = make_test_group("readers", "Read Only Group", true, false, false);
        registry.create(group).unwrap();
        registry.join("readers", "bela").unwrap();

        assert!(!registry.check_access("readers", "bela", "write"));
    }

    #[test]
    fn test_check_access_audit_allowed() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let group = make_test_group("auditors", "Audit Group", false, false, true);
        registry.create(group).unwrap();
        registry.join("auditors", "bela").unwrap();

        assert!(registry.check_access("auditors", "bela", "audit"));
    }

    #[test]
    fn test_check_access_audit_denied() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let group = make_test_group("no-audit", "No Audit Group", true, true, false);
        registry.create(group).unwrap();
        registry.join("no-audit", "bela").unwrap();

        assert!(!registry.check_access("no-audit", "bela", "audit"));
    }

    #[test]
    fn test_check_access_non_member() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let group = make_test_group("writers", "Write Group", true, true, true);
        registry.create(group).unwrap();

        // 'stranger' is not a member of the group
        assert!(!registry.check_access("writers", "stranger", "read"));
        assert!(!registry.check_access("writers", "stranger", "write"));
        assert!(!registry.check_access("writers", "stranger", "audit"));
    }

    #[test]
    fn test_check_access_nonexistent_group() {
        let temp_file = NamedTempFile::new().unwrap();
        let registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        assert!(!registry.check_access("nonexistent", "bela", "read"));
    }

    #[test]
    fn test_check_access_unknown_action() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut registry = GroupRegistry::load_from(temp_file.path()).unwrap();

        let group = make_test_group("writers", "Write Group", true, true, true);
        registry.create(group).unwrap();
        registry.join("writers", "bela").unwrap();

        assert!(!registry.check_access("writers", "bela", "delete"));
        assert!(!registry.check_access("writers", "bela", "manage"));
    }

    #[test]
    fn test_registry_persistence() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        {
            let mut registry = GroupRegistry::load_from(&path).unwrap();
            let mut group = make_test_group("family", "Family Group", true, true, true);
            group.members.push("bela".to_string());
            registry.create(group).unwrap();
        } // drops and saves

        // reload from the same file
        let reloaded = GroupRegistry::load_from(&path).unwrap();
        let group = reloaded.get_group("family");
        assert!(group.is_some());
        let g = group.unwrap();
        assert_eq!(g.name, "Family Group");
        assert_eq!(g.members, vec!["bela".to_string()]);
        assert!(g.acl.read);
        assert!(g.acl.write);
        assert!(g.acl.audit);
    }
}
