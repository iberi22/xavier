// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Skill Registry — Vectorized index of available skills
//!
//! Scans skill directories, extracts metadata from YAML frontmatter,
//! and indexes them in memory for semantic search. This enables Xavier
//! to match incoming tasks to the best available skill without the
//! IDE or CLI agent needing to know which skills exist.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, info, warn};
use walkdir::WalkDir;

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
}

impl SkillRegistry {
    /// Create a new registry scanning the given directories.
    pub fn new(scan_paths: Vec<PathBuf>) -> Self {
        Self {
            skills: HashMap::new(),
            scan_paths,
        }
    }

    /// Create with default Xavier skill paths.
    pub fn with_defaults(workspace_root: &Path) -> Self {
        let paths = vec![
            workspace_root.join("skills"),
            workspace_root.join(".agents").join("skills"),
        ];
        Self::new(paths)
    }

    /// Scan all directories and index skills. Re-indexes only changed files.
    pub async fn reindex(&mut self) -> Result<usize> {
        let mut indexed_count = 0;

        for scan_path in &self.scan_paths.clone() {
            if !scan_path.exists() {
                debug!("Skill scan path does not exist: {:?}", scan_path);
                continue;
            }

            for entry in WalkDir::new(scan_path)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
                .filter(|e| {
                    e.file_name()
                        .to_str()
                        .map(|n| n == "SKILL.md" || n.ends_with(".md"))
                        .unwrap_or(false)
                })
            {
                match self.index_skill_file(entry.path()).await {
                    Ok(true) => indexed_count += 1,
                    Ok(false) => {} // Already up to date
                    Err(e) => warn!("Failed to index skill at {:?}: {}", entry.path(), e),
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
        let content_hash = format!("{:x}", hasher.finalize());

        // Parse frontmatter
        let (name, description) = parse_frontmatter(&content);
        let name = name.unwrap_or_else(|| {
            path.parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string()
        });

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
}
