//! Integrator Agent - Applies changes and manages git state

use crate::agents::evolve::experiment::Hypothesis;
use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tracing::{info, warn};

/// Integrator - Manages git state and applies/discards changes
pub struct Integrator {
    memory_path: PathBuf,
}

impl Integrator {
    /// New.
    pub fn new() -> Self {
        Self {
            memory_path: PathBuf::from("src/memory/"),
        }
    }

    /// Backup current memory modules state
    pub async fn backup_memory_modules(&self) -> Result<Backup> {
        let files = self.list_editable_files().await?;

        let mut backup = Backup { files: Vec::new() };

        for file in &files {
            if file.exists() {
                let content = tokio::fs::read_to_string(file).await?;
                backup.files.push((file.clone(), content));
            }
        }

        info!(files = backup.files.len(), "Created backup");
        Ok(backup)
    }

    /// Apply a hypothesis (modify files)
    pub async fn apply_hypothesis(&self, hypothesis: &Hypothesis) -> Result<bool> {
        if hypothesis.patch.is_empty()
            && hypothesis.hypothesis_type
                != crate::agents::evolve::experiment::HypothesisType::Simplification
        {
            return Ok(false);
        }

        // Path Sanitization (SECURITY)
        for file_path in &hypothesis.files {
            self.validate_path(file_path)?;
        }

        info!(
            hypothesis_id = %hypothesis.id,
            description = %hypothesis.description,
            files = ?hypothesis.files,
            "Applying hypothesis"
        );

        // Apply patch
        if !hypothesis.patch.is_empty() {
            if hypothesis.patch.contains("<<<<<<< SEARCH") {
                // Custom SEARCH/REPLACE format
                for file_path in &hypothesis.files {
                    self.apply_search_replace(file_path, &hypothesis.patch)
                        .await?;
                }
            } else {
                // Try standard patch via temp file
                let patch_file =
                    std::env::temp_dir().join(format!("xavier-patch-{}.patch", hypothesis.id));
                tokio::fs::write(&patch_file, &hypothesis.patch).await?;

                let mut cmd = Command::new("patch");
                cmd.arg("-p1").arg("-i").arg(&patch_file);

                let status = cmd.status().await?;

                if !status.success() {
                    warn!("Failed to apply patch via 'patch' command");
                    return Err(anyhow!("Patch application failed"));
                }
            }
        }

        // Verify compilation
        info!("Verifying compilation after changes...");
        let mut check_cmd = Command::new("cargo");
        check_cmd.args(["check", "--no-default-features", "--features", "ci-safe"]);

        let check = check_cmd.status().await?;

        if !check.success() {
            return Err(anyhow!("Compilation failed after applying hypothesis"));
        }

        Ok(true)
    }

    fn validate_path(&self, path_str: &str) -> Result<()> {
        let path = Path::new(path_str);

        // Ensure path is relative and stays within src/
        if path.is_absolute() {
            return Err(anyhow!("Absolute paths are not allowed: {}", path_str));
        }

        if path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(anyhow!(
                "Parent directory components ('..') are not allowed: {}",
                path_str
            ));
        }

        if !path_str.starts_with("src/") {
            return Err(anyhow!("Path must start with 'src/': {}", path_str));
        }

        // Block security critical paths
        if path_str.starts_with("src/crypto/")
            || path_str.starts_with("src/auth/")
            || path_str == "src/lib.rs"
        {
            return Err(anyhow!(
                "Modification of security-critical file blocked: {}",
                path_str
            ));
        }

        Ok(())
    }

    async fn apply_search_replace(&self, file_path: &str, patch: &str) -> Result<()> {
        let content = tokio::fs::read_to_string(file_path).await?;

        let parts: Vec<&str> = patch.split("<<<<<<< SEARCH").collect();
        let mut new_content = content;

        for part in parts.into_iter().skip(1) {
            let subparts: Vec<&str> = part.split("=======").collect();
            if subparts.len() < 2 {
                continue;
            }

            let search = subparts[0].trim_matches('\n');
            let replace_parts: Vec<&str> = subparts[1].split(">>>>>>> REPLACE").collect();
            if replace_parts.is_empty() {
                continue;
            }

            let replace = replace_parts[0].trim_matches('\n');

            // Check if search block is unique to avoid collateral damage
            let occurrences = new_content.matches(search).count();
            if occurrences == 0 {
                return Err(anyhow!("Search block not found in {}", file_path));
            }
            if occurrences > 1 {
                return Err(anyhow!(
                    "Search block not unique in {} (found {} times)",
                    file_path,
                    occurrences
                ));
            }

            new_content = new_content.replace(search, replace);
        }

        tokio::fs::write(file_path, new_content).await?;
        Ok(())
    }

    /// Restore from backup (discard changes)
    pub async fn restore(&self, backup: Backup) -> Result<()> {
        for (path, content) in backup.files {
            tokio::fs::write(&path, content).await?;
        }
        info!("Restored from backup");
        Ok(())
    }

    /// Commit changes (keep improvement)
    pub async fn commit(&self, hypothesis: &Hypothesis) -> Result<()> {
        // Run git commands if in a git repo
        for file in &hypothesis.files {
            let _ = Command::new("git").arg("add").arg(file).status().await;
        }

        let mut commit_cmd = Command::new("git");
        commit_cmd
            .arg("commit")
            .arg("-m")
            .arg(format!("[auto-evolve] {}", hypothesis.description));
        let _ = commit_cmd.status().await;

        info!(
            hypothesis_id = %hypothesis.id,
            description = %hypothesis.description,
            "Committed improvement"
        );
        Ok(())
    }

    /// Reset to baseline commit
    pub async fn reset_to_baseline(&self) -> Result<()> {
        let _ = Command::new("git")
            .arg("reset")
            .arg("--hard")
            .arg("HEAD")
            .status()
            .await;
        let _ = Command::new("git").arg("clean").arg("-fd").status().await;

        info!("Reset to baseline");
        Ok(())
    }

    /// List editable files
    async fn list_editable_files(&self) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();

        if self.memory_path.exists() {
            let entries = tokio::fs::read_dir(&self.memory_path).await?;
            let mut entries = std::pin::pin!(entries);

            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
                    files.push(path);
                }
            }
        }

        // Also include agent files
        let agent_files = vec![
            PathBuf::from("src/agents/system1.rs"),
            PathBuf::from("src/agents/system2.rs"),
            PathBuf::from("src/agents/system3/mod.rs"),
            PathBuf::from("src/agents/system3/types.rs"),
            PathBuf::from("src/agents/system3/client.rs"),
            PathBuf::from("src/agents/system3/helpers.rs"),
            PathBuf::from("src/agents/system3/engine.rs"),
            PathBuf::from("src/retrieval/scoring.rs"),
        ];

        for af in agent_files {
            if af.exists() {
                files.push(af);
            }
        }

        Ok(files)
    }
}

impl Default for Integrator {
    fn default() -> Self {
        Self::new()
    }
}

/// Backup of modified files
#[derive(Debug, Clone)]
pub struct Backup {
    files: Vec<(PathBuf, String)>,
}
