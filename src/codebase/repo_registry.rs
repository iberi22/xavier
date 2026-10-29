use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// A registry of known git repositories, persisted in `XAVIER_DATA_DIR/code_repos.json`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RepoRegistry {
    /// The absolute paths to registered repositories.
    repos: HashSet<PathBuf>,
}

impl RepoRegistry {
    /// Loads the registry from the data directory.
    ///
    /// If the file does not exist, returns an empty registry.
    pub fn load() -> Result<Self> {
        let path = Self::file_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)?;
        let registry = serde_json::from_str(&content)?;
        Ok(registry)
    }

    /// Saves the registry to the data directory atomically.
    pub fn save(&self) -> Result<()> {
        let path = Self::file_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let temp_path = path.with_extension("json.tmp");
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&temp_path, content)?;
        std::fs::rename(temp_path, path)?;
        Ok(())
    }

    /// Validates and adds a path to the registry.
    ///
    /// The path must be an existing git repository (must contain a `.git` directory).
    /// Stores the canonicalized absolute path. Duplicate additions are no-ops.
    pub fn add<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let path = path.as_ref();
        if !path.join(".git").exists() {
            bail!(
                "Path is not a git repository (missing .git directory): {}",
                path.display()
            );
        }

        let canonical = std::fs::canonicalize(path)?;
        self.repos.insert(canonical);
        Ok(())
    }

    /// Removes a repository by its path or its directory name (basename).
    pub fn remove(&mut self, path_or_name: &str) {
        // First try removing it exactly if it matches an absolute canonical path.
        let as_path = Path::new(path_or_name);
        if as_path.is_absolute() {
            if let Ok(canonical) = std::fs::canonicalize(as_path) {
                if self.repos.remove(&canonical) {
                    return;
                }
            }
        }

        // Otherwise, match against the basename.
        self.repos.retain(|p| {
            if let Some(name) = p.file_name() {
                name.to_string_lossy() != path_or_name
            } else {
                true
            }
        });
    }

    /// Returns a list of all registered canonical absolute paths.
    pub fn list(&self) -> Vec<PathBuf> {
        let mut list: Vec<_> = self.repos.iter().cloned().collect();
        list.sort();
        list
    }

    /// Path to the registry JSON file.
    fn file_path() -> PathBuf {
        crate::settings::XavierSettings::resolve_data_dir().join("code_repos.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    #[serial_test::serial]
    fn test_repo_registry_load_save() {
        let _env = crate::settings::tests::TempEnv::new();
        let tmp = tempdir().unwrap();
        std::env::set_var("XAVIER_DATA_DIR", tmp.path());

        // Initially empty
        let mut registry = RepoRegistry::load().unwrap();
        assert!(registry.list().is_empty());

        let fake_repo = tmp.path().join("fake_repo");
        std::fs::create_dir_all(fake_repo.join(".git")).unwrap();

        registry.add(&fake_repo).unwrap();
        assert_eq!(registry.list().len(), 1);

        // Save and reload
        registry.save().unwrap();
        let loaded = RepoRegistry::load().unwrap();
        assert_eq!(loaded.list().len(), 1);

        // Canonicalization can change path string, but they point to the same directory
        let canonical_fake_repo = std::fs::canonicalize(&fake_repo).unwrap();
        assert_eq!(loaded.list()[0], canonical_fake_repo);

        std::env::remove_var("XAVIER_DATA_DIR");
    }

    #[test]
    #[serial_test::serial]
    fn test_repo_registry_add_rejects_non_git() {
        let _env = crate::settings::tests::TempEnv::new();
        let tmp = tempdir().unwrap();
        std::env::set_var("XAVIER_DATA_DIR", tmp.path());

        let mut registry = RepoRegistry::load().unwrap();

        let not_a_repo = tmp.path().join("not_a_repo");
        std::fs::create_dir_all(&not_a_repo).unwrap();

        let err = registry.add(&not_a_repo).unwrap_err();
        assert!(err.to_string().contains("missing .git directory"));

        std::env::remove_var("XAVIER_DATA_DIR");
    }

    #[test]
    #[serial_test::serial]
    fn test_repo_registry_duplicate_add_is_noop() {
        let _env = crate::settings::tests::TempEnv::new();
        let tmp = tempdir().unwrap();
        std::env::set_var("XAVIER_DATA_DIR", tmp.path());

        let mut registry = RepoRegistry::load().unwrap();

        let fake_repo = tmp.path().join("dup_repo");
        std::fs::create_dir_all(fake_repo.join(".git")).unwrap();

        registry.add(&fake_repo).unwrap();
        registry.add(&fake_repo).unwrap();
        assert_eq!(registry.list().len(), 1);

        std::env::remove_var("XAVIER_DATA_DIR");
    }

    #[test]
    #[serial_test::serial]
    fn test_repo_registry_remove() {
        let _env = crate::settings::tests::TempEnv::new();
        let tmp = tempdir().unwrap();
        std::env::set_var("XAVIER_DATA_DIR", tmp.path());

        let mut registry = RepoRegistry::load().unwrap();

        let repo1 = tmp.path().join("repo_one");
        let repo2 = tmp.path().join("repo_two");
        std::fs::create_dir_all(repo1.join(".git")).unwrap();
        std::fs::create_dir_all(repo2.join(".git")).unwrap();

        registry.add(&repo1).unwrap();
        registry.add(&repo2).unwrap();
        assert_eq!(registry.list().len(), 2);

        // Remove by basename
        registry.remove("repo_one");
        assert_eq!(registry.list().len(), 1);
        assert!(registry.list()[0].ends_with("repo_two"));

        // Remove by absolute path
        registry.remove(&repo2.to_string_lossy());
        assert!(registry.list().is_empty());

        std::env::remove_var("XAVIER_DATA_DIR");
    }
}
