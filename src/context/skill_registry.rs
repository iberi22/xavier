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

use crate::ports::outbound::embedding_port::EmbeddingPort;

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
    /// Dense vector of [`skill_embed_text`] via `EmbeddingPort` (`None` = not embedded yet)
    #[serde(default)]
    pub embedding: Option<Vec<f32>>,
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
        self.index_skill_file_with_port(path, None).await
    }

    /// Index a single skill file, embedding it when a port is given.
    async fn index_skill_file_with_port(
        &mut self,
        path: &Path,
        port: Option<&EmbeddingPort>,
    ) -> Result<bool> {
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

        let mut skill = IndexedSkill {
            name: name.clone(),
            description,
            domains,
            content_hash,
            token_cost,
            content,
            source_path: path.to_string_lossy().to_string(),
            embedding: None,
        };

        // Embed at index time; failures stay fail-open (skill works keyword-only).
        if let Some(port) = port {
            let text = skill_embed_text(&skill.name, &skill.description);
            match port.embed(&text).await {
                Ok(vector) if !vector.is_empty() => skill.embedding = Some(vector),
                Ok(_) => debug!("Empty embedding for skill {}", skill.name),
                Err(e) => debug!("Embedding failed for skill {}: {}", skill.name, e),
            }
        }

        self.skills.insert(name, skill);
        Ok(true)
    }

    /// Scan all directories and index skills, embedding each through `port`.
    /// Falls back to keyword-only entries when the port has no backend.
    pub async fn reindex_with_embeddings(&mut self, port: &EmbeddingPort) -> Result<usize> {
        let mut indexed_count = 0;

        for scan_path in &self.scan_paths.clone() {
            if !scan_path.exists() {
                debug!("Skill scan path does not exist: {:?}", scan_path);
                continue;
            }

            for path in collect_skill_files(scan_path) {
                match self.index_skill_file_with_port(&path, Some(port)).await {
                    Ok(true) => indexed_count += 1,
                    Ok(false) => {} // Already up to date
                    Err(e) => warn!("Failed to index skill at {:?}: {}", path, e),
                }
            }
        }

        let backfilled = self.embed_missing(port).await;
        info!(
            "Skill registry reindex complete: {} skills indexed, {} backfilled, {} total",
            indexed_count,
            backfilled,
            self.skills.len()
        );
        Ok(indexed_count)
    }

    /// Embed indexed skills that lack a vector. Returns newly embedded count.
    pub async fn embed_missing(&mut self, port: &EmbeddingPort) -> usize {
        let pending: Vec<(String, String)> = self
            .skills
            .values()
            .filter(|skill| skill.embedding.as_ref().is_none_or(Vec::is_empty))
            .map(|skill| {
                (
                    skill.name.clone(),
                    skill_embed_text(&skill.name, &skill.description),
                )
            })
            .collect();

        let mut embedded = 0;
        for (name, text) in pending {
            match port.embed(&text).await {
                Ok(vector) if !vector.is_empty() => {
                    if let Some(skill) = self.skills.get_mut(&name) {
                        skill.embedding = Some(vector);
                        embedded += 1;
                    }
                }
                Ok(_) => debug!("Empty embedding for skill {}", name),
                Err(e) => debug!("Embedding failed for skill {}: {}", name, e),
            }
        }
        embedded
    }

    /// Set a skill vector directly (tests, eval harness, offline backfill).
    pub fn set_skill_embedding(&mut self, name: &str, embedding: Vec<f32>) -> bool {
        if let Some(skill) = self.skills.get_mut(name) {
            skill.embedding = Some(embedding);
            true
        } else {
            false
        }
    }

    /// Number of skills holding a non-empty vector.
    pub fn vector_count(&self) -> usize {
        self.skills
            .values()
            .filter(|skill| skill.embedding.as_ref().is_some_and(|v| !v.is_empty()))
            .count()
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

    /// Rank by cosine similarity to a precomputed query vector (top-K).
    /// Keyword `score_skill_match` breaks ties; confidence is the measured
    /// cosine clamped to [0, 1]. Falls back to keyword [`search`](Self::search)
    /// when the vector store is empty, dims mismatch, or nothing scores > 0.
    pub fn search_with_vector(
        &self,
        query: &str,
        query_vector: &[f32],
        top_k: usize,
    ) -> Vec<(f32, &IndexedSkill)> {
        if query_vector.is_empty() {
            return self.search(query, top_k);
        }
        let Some(rows) = self.vector_rows(query, query_vector) else {
            return self.search(query, top_k);
        };
        let ranked = rank_vectors(rows, query_vector.to_vec(), top_k);
        if ranked.is_empty() {
            return self.search(query, top_k);
        }
        ranked
            .into_iter()
            .filter_map(|(name, score)| self.skills.get(&name).map(|skill| (score, skill)))
            .collect()
    }

    /// Embed `query` through `port`, then rank by cosine similarity.
    /// Fully fail-open: any embedding failure degrades to keyword search.
    /// The cosine loop runs in `spawn_blocking` (Tokio + Rayon golden rule).
    pub async fn search_semantic(
        &self,
        query: &str,
        top_k: usize,
        port: &EmbeddingPort,
    ) -> Vec<(f32, &IndexedSkill)> {
        let query_vector = match port.embed(query).await {
            Ok(vector) if !vector.is_empty() => vector,
            _ => return self.search(query, top_k),
        };
        let Some(rows) = self.vector_rows(query, &query_vector) else {
            return self.search(query, top_k);
        };
        let ranked = tokio::task::spawn_blocking(move || rank_vectors(rows, query_vector, top_k))
            .await
            .unwrap_or_else(|_| Vec::new());
        if ranked.is_empty() {
            return self.search(query, top_k);
        }
        ranked
            .into_iter()
            .filter_map(|(name, score)| self.skills.get(&name).map(|skill| (score, skill)))
            .collect()
    }

    /// Snapshot of embeddable skills: owned vectors + keyword tiebreak scores.
    /// Returns `None` when no skill holds a vector matching `query_vector`.
    fn vector_rows(&self, query: &str, query_vector: &[f32]) -> Option<Vec<SkillVectorRow>> {
        let query_lower = query.to_lowercase();
        let query_terms: Vec<&str> = query_lower.split_whitespace().collect();
        let rows: Vec<SkillVectorRow> = self
            .skills
            .values()
            .filter_map(|skill| {
                let vector = skill.embedding.as_ref()?;
                if vector.is_empty() || vector.len() != query_vector.len() {
                    return None;
                }
                Some(SkillVectorRow {
                    name: skill.name.clone(),
                    vector: vector.clone(),
                    keyword: score_skill_match(skill, &query_lower, &query_terms),
                })
            })
            .collect();
        if rows.is_empty() {
            None
        } else {
            Some(rows)
        }
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

/// Text embedded at index time: skill name plus first 200 chars of description.
pub fn skill_embed_text(name: &str, description: &str) -> String {
    let snippet: String = description.chars().take(200).collect();
    format!("{name} {snippet}")
}

/// Cosine similarity clamped to [0, 1].
/// Returns 0.0 on empty inputs, dimension mismatch, or zero-norm vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0_f32;
    let mut norm_a = 0.0_f32;
    let mut norm_b = 0.0_f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    if norm_a <= 0.0 || norm_b <= 0.0 {
        return 0.0;
    }
    (dot / (norm_a.sqrt() * norm_b.sqrt())).clamp(0.0, 1.0)
}

/// Owned ranking row: moved into `spawn_blocking` for the cosine loop.
struct SkillVectorRow {
    name: String,
    vector: Vec<f32>,
    keyword: f32,
}

/// Order rows by cosine desc, keyword tiebreak, name for determinism.
/// Returns `(name, cosine)` pairs with cosine > 0, truncated to `top_k`.
fn rank_vectors(
    rows: Vec<SkillVectorRow>,
    query_vector: Vec<f32>,
    top_k: usize,
) -> Vec<(String, f32)> {
    let mut scored: Vec<(String, f32, f32)> = rows
        .into_iter()
        .map(|row| {
            let cosine = cosine_similarity(&row.vector, &query_vector);
            (row.name, cosine, row.keyword)
        })
        .filter(|(_, cosine, _)| *cosine > 0.0)
        .collect();
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal))
            .then(a.0.cmp(&b.0))
    });
    scored.truncate(top_k);
    scored
        .into_iter()
        .map(|(name, cosine, _)| (name, cosine))
        .collect()
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
            embedding: None,
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

    // --- feat-skill-semantic-rank (#302) ---

    /// Pure offline mock: deterministic vectors, zero network I/O.
    struct MockEmbedder {
        vector: Vec<f32>,
    }

    #[async_trait::async_trait]
    impl crate::embedding::Embedder for MockEmbedder {
        async fn encode(&self, _text: &str) -> Result<Vec<f32>, crate::embedding::EmbeddingError> {
            Ok(self.vector.clone())
        }

        fn dimension(&self) -> usize {
            self.vector.len()
        }
    }

    fn mock_port(vector: Vec<f32>) -> EmbeddingPort {
        EmbeddingPort::with_embedder(std::sync::Arc::new(MockEmbedder { vector }))
    }

    fn vector_skill(name: &str, description: &str, embedding: Vec<f32>) -> IndexedSkill {
        IndexedSkill {
            name: name.to_string(),
            description: description.to_string(),
            domains: infer_domains(description, description),
            content_hash: format!("hash-{name}"),
            token_cost: 100,
            content: format!("# {name}\n{description}"),
            source_path: format!("test/{name}"),
            embedding: Some(embedding),
        }
    }

    fn ranked_registry() -> SkillRegistry {
        let mut registry = SkillRegistry::new(vec![]);
        for (name, desc, vec) in [
            (
                "git-pr-reviewer",
                "Verify GitHub pull requests with evidence",
                vec![0.9, 0.1, 0.0],
            ),
            (
                "rust-tester",
                "Run cargo test suites for Rust code",
                vec![0.0, 0.9, 0.1],
            ),
            (
                "deployer",
                "Publish releases to production hosting",
                vec![0.1, 0.0, 0.9],
            ),
        ] {
            registry
                .skills
                .insert(name.to_string(), vector_skill(name, desc, vec));
        }
        registry
    }

    #[test]
    fn test_cosine_similarity_properties() {
        assert!((cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0, 0.0]), 0.0);
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
        // Negative cosine clamps to 0.
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[-1.0, 0.0]), 0.0);
    }

    #[test]
    fn test_confidence_calibration_range() {
        let registry = ranked_registry();
        // Paraphrase queries plus degenerate inputs; every confidence must be a measured cosine in [0, 1].
        for (query, vector) in [
            ("review a pull request", vec![1.0, 0.0, 0.0]),
            ("cargo tests failing", vec![0.0, 1.0, 0.0]),
            ("ship to production", vec![0.0, 0.0, 1.0]),
            ("orthogonal nonsense", vec![-1.0, -1.0, -1.0]),
            ("zero query", vec![0.0, 0.0, 0.0]),
        ] {
            for (confidence, _) in registry.search_with_vector(query, &vector, 3) {
                assert!(
                    (0.0..=1.0).contains(&confidence),
                    "confidence {confidence} out of [0,1] for query {query:?}"
                );
                assert!(confidence.is_finite(), "confidence must be finite");
            }
        }
        // Exact self-match measures ~1.0, never a constant above it.
        let hits = registry.search_with_vector("pr", &[0.9, 0.1, 0.0], 1);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].0 > 0.99 && hits[0].0 <= 1.0);
    }

    #[tokio::test]
    async fn test_ranking_offline_no_network() {
        // Mock port performs pure in-memory encoding: no sockets, no env, no backend.
        let registry = ranked_registry();
        let port = mock_port(vec![1.0, 0.0, 0.0]);
        let hits = registry
            .search_semantic("review a pull request", 3, &port)
            .await;
        assert!(!hits.is_empty(), "mocked semantic search must rank");
        assert_eq!(hits[0].1.name, "git-pr-reviewer");
        for (confidence, _) in &hits {
            assert!((0.0..=1.0).contains(confidence));
        }
        // Unconfigured port attempts no I/O and degrades to keyword search.
        let offline = EmbeddingPort::new();
        let degraded = registry.search_semantic("cargo", 3, &offline).await;
        let keyword = registry.search("cargo", 3);
        assert_eq!(degraded.len(), keyword.len());
        for (a, b) in degraded.iter().zip(keyword.iter()) {
            assert_eq!(a.1.name, b.1.name);
        }
    }

    #[tokio::test]
    async fn test_keyword_fallback_when_vector_store_empty() {
        // Fresh temp registry: indexed without any port, so zero vectors.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("skills").join("fallback-skill");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: fallback-skill\ndescription: \"Skill fallback-skill for testing\"\n---\n\n# fallback-skill\n",
        )
        .unwrap();
        let mut registry = SkillRegistry::new(vec![tmp.path().join("skills")]);
        registry.reindex().await.unwrap();
        assert_eq!(registry.vector_count(), 0);

        let query = "fallback-skill testing";
        let keyword = registry.search(query, 3);
        assert!(!keyword.is_empty(), "keyword search must match");
        // Empty query vector, dim mismatch, and unconfigured port all degrade identically.
        let via_empty = registry.search_with_vector(query, &[], 3);
        let via_mismatch = registry.search_with_vector(query, &[1.0, 2.0], 3);
        let via_port = registry
            .search_semantic(query, 3, &EmbeddingPort::new())
            .await;
        for fallback in [via_empty, via_mismatch, via_port] {
            assert_eq!(fallback.len(), keyword.len());
            for (a, b) in fallback.iter().zip(keyword.iter()) {
                assert_eq!(a.1.name, b.1.name);
                assert!((a.0 - b.0).abs() < f32::EPSILON);
            }
        }
    }

    #[tokio::test]
    async fn test_reindex_with_embeddings_embeds_at_index() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("skills").join("embedded-skill");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: embedded-skill\ndescription: \"Skill embedded-skill for testing\"\n---\n\n# embedded-skill\n",
        )
        .unwrap();
        let mut registry = SkillRegistry::new(vec![tmp.path().join("skills")]);
        let port = mock_port(vec![0.5, 0.5, 0.5]);
        registry.reindex_with_embeddings(&port).await.unwrap();
        assert_eq!(registry.vector_count(), 1);
        let skill = registry.get("embedded-skill").unwrap();
        assert_eq!(skill.embedding.as_ref().unwrap(), &vec![0.5, 0.5, 0.5]);
        // Unchanged files are not re-embedded, but missing vectors backfill.
        registry.skills.get_mut("embedded-skill").unwrap().embedding = None;
        assert_eq!(registry.embed_missing(&port).await, 1);
        assert_eq!(registry.vector_count(), 1);
    }
}
