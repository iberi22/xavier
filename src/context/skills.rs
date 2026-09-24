//! Skill loading for context-aware tool use.
//!
//! Scans skill directories for `SKILL.md` files and validates them
//! against the Agent Skills frontmatter shape (`name:`/`description:`).
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub content: String,
}

pub struct SkillLoader {
    base_path: PathBuf,
}

impl SkillLoader {
    /// Create a loader rooted at `base_path`.
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }

    /// Load all `SKILL.md` files below `base_path`.
    pub async fn load_all(&self) -> Result<Vec<Skill>> {
        let mut skills = Vec::new();

        if !self.base_path.exists() {
            return Ok(skills);
        }

        for entry in WalkDir::new(&self.base_path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| e.file_name().to_str().is_some_and(|n| n == "SKILL.md"))
        {
            let content = fs::read_to_string(entry.path()).await?;
            if !self.validate_skill(&content) {
                continue;
            }
            // Frontmatter name wins; fall back to the parent dir name,
            // mirroring `skill_registry::parse_frontmatter`.
            let name = frontmatter_field(&content, "name").unwrap_or_else(|| {
                entry
                    .path()
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string()
            });
            skills.push(Skill { name, content });
        }

        Ok(skills)
    }

    /// Accept content with Agent Skills frontmatter (`name:`/`description:`).
    pub fn validate_skill(&self, content: &str) -> bool {
        frontmatter_field(content, "name").is_some()
            || frontmatter_field(content, "description").is_some()
    }
}

/// Read one non-empty field from YAML frontmatter, if present.
fn frontmatter_field(content: &str, key: &str) -> Option<String> {
    if !content.starts_with("---") {
        return None;
    }
    let parts: Vec<&str> = content.splitn(3, "---").collect();
    if parts.len() < 3 {
        return None;
    }
    let prefix = format!("{key}:");
    for line in parts[1].lines() {
        if let Some(value) = line.trim().strip_prefix(prefix.as_str()) {
            let value = value.trim().trim_matches('"').trim().to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self as stdfs, File};
    use std::io::Write;
    use std::path::Path;
    use tempfile::tempdir;

    /// Real store shape: `<base>/<skill>/SKILL.md` with YAML frontmatter.
    fn real_skill(name: &str) -> String {
        format!(
            "---\nname: {name}\ndescription: Fixture skill for probe test.\n---\n\n# {name}\n\nBody.\n"
        )
    }

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            stdfs::create_dir_all(parent).expect("test assertion");
        }
        let mut file = File::create(path).expect("test assertion");
        write!(file, "{content}").expect("test assertion");
    }

    #[tokio::test]
    async fn test_loader_loads_real_skill_format_or_module_removed() {
        let dir = tempdir().expect("test assertion");
        write_file(&dir.path().join("adhd/SKILL.md"), &real_skill("adhd"));
        write_file(
            &dir.path().join("agents-sdk/SKILL.md"),
            &real_skill("agents-sdk"),
        );
        // Frontmatter-less SKILL.md must be skipped.
        write_file(&dir.path().join("broken/SKILL.md"), "No frontmatter here\n");
        // Non-SKILL markdown must be ignored.
        write_file(&dir.path().join("adhd/notes.md"), &real_skill("stray"));

        let loader = SkillLoader::new(dir.path());
        let skills = loader.load_all().await.expect("test assertion");

        assert_eq!(skills.len(), 2);
        let mut names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, vec!["adhd", "agents-sdk"]);
    }

    #[tokio::test]
    async fn falls_back_to_parent_dir_name() {
        let dir = tempdir().expect("test assertion");
        write_file(
            &dir.path().join("nodesc/SKILL.md"),
            "---\ndescription: No name field here.\n---\n\n# Body\n",
        );

        let loader = SkillLoader::new(dir.path());
        let skills = loader.load_all().await.expect("test assertion");

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "nodesc");
    }

    #[test]
    fn rejects_frontmatter_less_garbage() {
        let loader = SkillLoader::new("/nonexistent");
        assert!(!loader.validate_skill("No purpose here\n"));
        assert!(!loader.validate_skill(""));
        assert!(!loader.validate_skill("# Purpose\nOld marker only\n"));
        assert!(!loader.validate_skill("---\nname: \n---\nEmpty name\n"));
        assert!(loader.validate_skill(&real_skill("ok")));
    }
}
