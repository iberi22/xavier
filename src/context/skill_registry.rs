//! Skill Registry — Vectorized index of available skills
//!
//! Scans skill directories, extracts metadata from YAML frontmatter,
//! and indexes them in memory for semantic search. This enables Xavier
//! to match incoming tasks to the best available skill without the
//! IDE or CLI agent needing to know which skills exist.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, info, warn};

/// A skill indexed for semantic search and dispatch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedSkill {
    /// Unique name from frontmatter (e.g. "openclaw-trace-analyzer")
    pub name: String,
    /// Human-readable description from frontmatter
    pub description: String,
    /// Domain tags inferred from description keywords
    pub domains: Vec<String>,
    /// SHA-256 of the skill file content (for change detection)
    pub content_hash: String,
    /// Estimated token cost of injecting this skill's full content
    pub token_cost: usize,
    /// The raw skill content (instructions)
    pub content: String,
    /// File path where the skill lives on disk
    pub source_path: String,
}

impl IndexedSkill {
    /// Build a compacted version of the skill that uses fewer tokens.
    /// Strips examples, tests sections, and verbose formatting.
    pub fn compacted_content(&self, max_tokens: usize) -> String {
        let words: Vec<&str> = self.content.split_whitespace().collect();
        if words.len() <= max_tokens {
            return self.content.clone();
        }
        // Keep the first max_tokens words and add a truncation marker
        let truncated: String = words[..max_tokens].join(" ");
        format!("{}...[skill truncated for token budget]", truncated)
    }
}

/// Registry that holds all indexed skills and supports semantic search.
pub struct SkillRegistry {
    /// All indexed skills by name
    skills: HashMap<String, IndexedSkill>,
    /// Directories to scan for skills
    scan_paths: Vec<PathBuf>,
    /// Skill names excluded via Hermes `skills.disabled` (fail-open if unreadable)
    disabled: HashSet<String>,
}

/// Resolve the user home dir from the environment at runtime (12-factor).
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Canonical Hermes skill store: `$HOME/.hermes/skills`.
fn hermes_store_path(home: &Path) -> PathBuf {
    home.join(".hermes").join("skills")
}

/// Hermes agent config: `$HOME/.hermes/config.yaml`.
fn hermes_config_path(home: &Path) -> PathBuf {
    home.join(".hermes").join("config.yaml")
}

/// Default scan paths for an explicit home dir (pure helper, testable).
fn default_scan_paths(workspace_root: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = vec![
        workspace_root.join("skills"),
        workspace_root.join(".agents").join("skills"),
    ];
    if let Some(home) = home {
        paths.push(hermes_store_path(home));
    }
    paths
}

/// Load `skills.disabled` names from a Hermes `config.yaml`.
/// Fail-open: missing/unreadable/invalid config yields an empty set.
fn load_disabled_from_config(config_path: &Path) -> HashSet<String> {
    let content = match std::fs::read_to_string(config_path) {
        Ok(content) => content,
        Err(_) => {
            debug!(
                "Hermes config not readable, no skills disabled: {:?}",
                config_path
            );
            return HashSet::new();
        }
    };
    let value: serde_yaml::Value = match serde_yaml::from_str(&content) {
        Ok(value) => value,
        Err(e) => {
            debug!("Hermes config not parseable, no skills disabled: {}", e);
            return HashSet::new();
        }
    };
    value
        .get("skills")
        .and_then(|skills| skills.get("disabled"))
        .and_then(|disabled| disabled.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|entry| entry.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Check whether a path points at a skill markdown file.
fn is_skill_file_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name == "SKILL.md" || name.ends_with(".md"))
        .unwrap_or(false)
}

/// Collect skill markdown files under `root` without following symlink cycles.
///
/// Tracks visited `(device, inode)` pairs and skips already-seen dirs, so a
/// symlink cycle (A -> B -> A) always terminates. File symlinks are still
/// followed (repo convention for shared skill stores).
fn collect_skill_files(root: &Path) -> Vec<PathBuf> {
    use std::os::unix::fs::MetadataExt;

    let mut files = Vec::new();
    let mut visited: HashSet<(u64, u64)> = HashSet::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let meta = match std::fs::metadata(&dir) {
            Ok(meta) => meta,
            Err(_) => {
                debug!("Skipping unreadable skill path: {:?}", dir);
                continue;
            }
        };
        if !meta.is_dir() {
            if meta.is_file() && is_skill_file_name(&dir) {
                files.push(dir);
            }
            continue;
        }
        if !visited.insert((meta.dev(), meta.ino())) {
            continue; // Already scanned: symlink cycle guard.
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => {
                debug!("Skipping unreadable skill dir: {:?}", dir);
                continue;
            }
        };
        for entry in entries.filter_map(|entry| entry.ok()) {
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            if file_type.is_symlink() {
                match std::fs::metadata(&path) {
                    Ok(target) if target.is_dir() => stack.push(path),
                    Ok(target) if target.is_file() && is_skill_file_name(&path) => {
                        files.push(path);
                    }
                    Ok(_) => {}
                    Err(_) => debug!("Skipping broken skill symlink: {:?}", path),
                }
            } else if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() && is_skill_file_name(&path) {
                files.push(path);
            }
        }
    }

    files.sort();
    files
}

impl SkillRegistry {
    /// Create a new registry scanning the given directories.
    pub fn new(scan_paths: Vec<PathBuf>) -> Self {
        Self {
            skills: HashMap::new(),
            scan_paths,
            disabled: HashSet::new(),
        }
    }

    /// Create with default Xavier skill paths plus the canonical Hermes store.
    /// `$HOME` is resolved from the environment at runtime, never hardcoded.
    pub fn with_defaults(workspace_root: &Path) -> Self {
        Self::with_home(workspace_root, home_dir().as_deref())
    }

    /// Create with default paths for an explicit home dir (testable helper).
    fn with_home(workspace_root: &Path, home: Option<&Path>) -> Self {
        let disabled = home
            .map(hermes_config_path)
            .map(|path| load_disabled_from_config(&path))
            .unwrap_or_default();
        Self {
            skills: HashMap::new(),
            scan_paths: default_scan_paths(workspace_root, home),
            disabled,
        }
    }

    /// Scan all directories and index skills. Re-indexes only changed files.
    pub async fn reindex(&mut self) -> Result<usize> {
        let mut indexed_count = 0;

        for scan_path in &self.scan_paths.clone() {
            if !scan_path.exists() {
                debug!("Skill scan path does not exist: {:?}", scan_path);
                continue;
            }

            for path in collect_skill_files(scan_path) {
                match self.index_skill_file(&path).await {
                    Ok(true) => indexed_count += 1,
                    Ok(false) => {} // Already up to date
                    Err(e) => warn!("Failed to index skill at {:?}: {}", path, e),
                }
            }
        }

        info!(
            "Skill registry reindex complete: {} skills indexed, {} total",
            indexed_count,
            self.skills.len()
        );
        Ok(indexed_count)
    }

    /// Index a single skill file. Returns true if it was new or changed.
    async fn index_skill_file(&mut self, path: &Path) -> Result<bool> {
        let content = fs::read_to_string(path)
            .await
            .with_context(|| format!("reading skill file {:?}", path))?;

        // Compute hash for change detection
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let content_hash = crate::crypto::hex_encode(hasher.finalize());

        // Parse frontmatter
        let (name, description) = parse_frontmatter(&content);
        let name = name.unwrap_or_else(|| {
            path.parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string()
        });

        // Exclude skills disabled via Hermes `skills.disabled`.
        if self.disabled.contains(&name) {
            debug!("Skipping disabled skill: {}", name);
            self.skills.remove(&name);
            return Ok(false);
        }

        // Check if already indexed with same hash
        if let Some(existing) = self.skills.get(&name) {
            if existing.content_hash == content_hash {
                return Ok(false);
            }
        }

        let token_cost = content.split_whitespace().count();
        let domains = infer_domains(&description, &content);

        let skill = IndexedSkill {
            name: name.clone(),
            description,
            domains,
            content_hash,
            token_cost,
            content,
            source_path: path.to_string_lossy().to_string(),
        };

        self.skills.insert(name, skill);
        Ok(true)
    }

    /// Search for skills matching a task description.
    /// Uses keyword matching against skill descriptions and domains.
    pub fn search(&self, query: &str, top_k: usize) -> Vec<(f32, &IndexedSkill)> {
        let query_lower = query.to_lowercase();
        let query_terms: Vec<&str> = query_lower.split_whitespace().collect();

        let mut scored: Vec<(f32, &IndexedSkill)> = self
            .skills
            .values()
            .map(|skill| {
                let score = score_skill_match(skill, &query_lower, &query_terms);
                (score, skill)
            })
            .filter(|(score, _)| *score > 0.0)
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        scored
    }

    /// Get a skill by name.
    pub fn get(&self, name: &str) -> Option<&IndexedSkill> {
        self.skills.get(name)
    }

    /// List all indexed skill names.
    pub fn list(&self) -> Vec<&str> {
        self.skills.keys().map(|s| s.as_str()).collect()
    }

    /// Total number of indexed skills.
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }
}

/// Score how well a skill matches a query.
fn score_skill_match(skill: &IndexedSkill, query_lower: &str, query_terms: &[&str]) -> f32 {
    let mut score = 0.0_f32;
    let desc_lower = skill.description.to_lowercase();
    let name_lower = skill.name.to_lowercase();

    // Exact name match
    if query_lower.contains(&name_lower) || name_lower.contains(query_lower) {
        score += 0.5;
    }

    // Term matches in description
    for term in query_terms {
        if term.len() < 3 {
            continue;
        }
        if desc_lower.contains(term) {
            score += 0.15;
        }
        if name_lower.contains(term) {
            score += 0.1;
        }
    }

    // Domain matches
    for domain in &skill.domains {
        let domain_lower = domain.to_lowercase();
        for term in query_terms {
            if domain_lower.contains(term) || term.contains(domain_lower.as_str()) {
                score += 0.1;
            }
        }
    }

    score.min(1.0)
}

/// Parse YAML frontmatter from a skill markdown file.
fn parse_frontmatter(content: &str) -> (Option<String>, String) {
    if !content.starts_with("---") {
        return (None, String::new());
    }

    let parts: Vec<&str> = content.splitn(3, "---").collect();
    if parts.len() < 3 {
        return (None, String::new());
    }

    let frontmatter = parts[1];
    let mut name = None;
    let mut description = String::new();

    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("name:") {
            name = Some(value.trim().trim_matches('"').to_string());
        } else if let Some(value) = line.strip_prefix("description:") {
            description = value.trim().trim_matches('"').to_string();
        }
    }

    (name, description)
}

/// Infer domain tags from the skill description and content.
fn infer_domains(description: &str, content: &str) -> Vec<String> {
    let combined = format!("{} {}", description, content).to_lowercase();
    let domain_keywords = [
        ("rust", "rust"),
        ("python", "python"),
        ("typescript", "typescript"),
        ("memory", "memory"),
        ("openclaw", "openclaw"),
        ("bot", "bots"),
        ("trace", "observability"),
        ("harness", "agent-harness"),
        ("skill", "skills"),
        ("xavier", "xavier"),
        ("debug", "debugging"),
        ("test", "testing"),
        ("deploy", "deployment"),
        ("git", "git"),
        ("api", "api"),
        ("database", "database"),
        ("security", "security"),
    ];

    let mut domains = Vec::new();
    for (keyword, domain) in &domain_keywords {
        if combined.contains(keyword) {
            domains.push(domain.to_string());
        }
    }

    domains.dedup();
    domains
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_yaml_frontmatter() {
        let content = r#"---
name: test-skill
description: "A test skill for unit testing"
---

# Test Skill

Instructions here.
"#;
        let (name, desc) = parse_frontmatter(content);
        assert_eq!(name.unwrap(), "test-skill");
        assert_eq!(desc, "A test skill for unit testing");
    }

    #[test]
    fn infers_domains_from_content() {
        let domains = infer_domains("Analyze OpenClaw bot traces", "rust memory debugging");
        assert!(domains.contains(&"openclaw".to_string()));
        assert!(domains.contains(&"bots".to_string()));
        assert!(domains.contains(&"rust".to_string()));
        assert!(domains.contains(&"memory".to_string()));
    }

    #[test]
    fn scores_skill_match() {
        let skill = IndexedSkill {
            name: "openclaw-trace-analyzer".to_string(),
            description: "Analyzes failed execution traces from OpenClaw bots".to_string(),
            domains: vec!["openclaw".to_string(), "observability".to_string()],
            content_hash: "abc".to_string(),
            token_cost: 500,
            content: "# Instructions".to_string(),
            source_path: "test".to_string(),
        };

        let query = "analyze openclaw bot failures";
        let query_lower = query.to_lowercase();
        let query_terms: Vec<&str> = query_lower.split_whitespace().collect();
        let score = super::score_skill_match(&skill, &query_lower, &query_terms);
        assert!(score > 0.3, "Expected high match score, got {}", score);
    }

    fn write_skill_md(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: \"Skill {name} for testing\"\n---\n\n# {name}\n"
            ),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn test_registry_scans_hermes_canonical_store() {
        // Fake HOME with a canonical-store layout, never the real $HOME.
        let home = tempfile::tempdir().unwrap();
        let store = super::hermes_store_path(home.path());
        write_skill_md(&store.join("fake-canon-skill"), "fake-canon-skill");

        let workspace = tempfile::tempdir().unwrap();
        let mut registry = SkillRegistry::with_home(workspace.path(), Some(home.path()));
        assert!(
            registry.scan_paths.contains(&store),
            "canonical store must be a default scan path"
        );
        let indexed = registry.reindex().await.unwrap();
        assert!(indexed >= 1, "expected the canonical store skill indexed");
        assert!(registry.get("fake-canon-skill").is_some());
    }

    #[tokio::test]
    async fn test_registry_symlink_cycle_terminates() {
        let tmp = tempfile::tempdir().unwrap();
        let dir_a = tmp.path().join("A");
        let dir_b = tmp.path().join("B");
        write_skill_md(&dir_a, "cycle-skill");
        std::fs::create_dir_all(&dir_b).unwrap();
        // A -> B -> A cycle.
        std::os::unix::fs::symlink(&dir_b, dir_a.join("link_to_b")).unwrap();
        std::os::unix::fs::symlink(&dir_a, dir_b.join("link_to_a")).unwrap();

        let mut registry = SkillRegistry::new(vec![tmp.path().to_path_buf()]);
        let result =
            tokio::time::timeout(std::time::Duration::from_secs(5), registry.reindex()).await;
        assert!(
            result.is_ok(),
            "reindex must terminate within 5s on a symlink cycle"
        );
        assert!(registry.get("cycle-skill").is_some());
    }

    #[tokio::test]
    async fn test_registry_honors_disabled_list() {
        let home = tempfile::tempdir().unwrap();
        let hermes_dir = home.path().join(".hermes");
        std::fs::create_dir_all(&hermes_dir).unwrap();
        std::fs::write(
            hermes_dir.join("config.yaml"),
            "skills:\n  disabled:\n    - no-go-skill\n",
        )
        .unwrap();

        let workspace = tempfile::tempdir().unwrap();
        let skills_dir = workspace.path().join("skills");
        write_skill_md(&skills_dir.join("no-go-skill"), "no-go-skill");
        write_skill_md(&skills_dir.join("yes-go-skill"), "yes-go-skill");

        let mut registry = SkillRegistry::with_home(workspace.path(), Some(home.path()));
        assert!(registry.disabled.contains("no-go-skill"));
        registry.reindex().await.unwrap();
        assert!(
            registry.get("no-go-skill").is_none(),
            "disabled skill must be excluded"
        );
        assert!(
            registry.get("yes-go-skill").is_some(),
            "enabled skill must be indexed"
        );
    }
}
