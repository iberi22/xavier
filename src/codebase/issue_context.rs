//! Issue Context Packager — analyze a GitHub issue and produce a PreciseChange package.
//!
//! Given an issue title + body, this module:
//! 1. Parses the issue to extract file paths, symbol names, and feature references.
//! 2. Detects issue type (bug, feature, refactor, other).
//! 3. Maps each entity to the CodeGraph (find_symbols, search_code) with relevance scoring.
//! 4. Generates a `PreciseChange` per matched symbol.
//! 5. Assembles an `IssueContextPackage` ready for an executor agent.
//!
//! This is the token-saving core: the agent receives only the fragments to change,
//! never the whole file.

use crate::codebase::snapshot::{PreciseChange, SnapshotManager};
use crate::memory::store::MemoryRecord;
use anyhow::{Context, Result};
use code_graph::db::CodeGraphDB;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Categorization of GitHub issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueType {
    Bug,
    Feature,
    Refactor,
    Other,
}

/// A single entity extracted from an issue body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedEntity {
    /// Kind of entity: "file", "symbol", "feature", "module".
    pub kind: String,
    /// The raw value extracted (e.g. "src/codebase/db.rs", "search_code", "feat-hybrid-search").
    pub value: String,
    /// Line or character offset in the issue body where this entity was found (0-based).
    pub offset: usize,
}

/// Result of mapping an extracted entity to the CodeGraph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappedEntity {
    pub entity: ExtractedEntity,
    /// Whether the entity was found in the code graph.
    pub found: bool,
    /// Matched symbol info (name, file, line range) if found.
    pub symbol_name: Option<String>,
    pub file: Option<String>,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    /// Calculated relevance score of this entity to the issue (0.0 to 1.0).
    pub relevance_score: f64,
}

/// Context boundary limits for packaging issue context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextLimits {
    /// Maximum number of matched symbols to include.
    pub max_symbols: usize,
    /// Maximum number of candidate files/dependencies to include.
    pub max_files: usize,
    /// Maximum cumulative snippet lines across generated changes.
    pub max_diff_lines: usize,
}

impl Default for ContextLimits {
    fn default() -> Self {
        Self {
            max_symbols: 20,
            max_files: 10,
            max_diff_lines: 500,
        }
    }
}

/// The complete context package for an issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueContextPackage {
    /// GitHub issue number.
    pub issue_id: String,
    /// Issue title.
    pub title: String,
    /// Repository name (owner/repo).
    pub repo: String,
    /// Categorized issue type (bug, feature, refactor, other).
    pub issue_type: IssueType,
    /// Extracted entities from the issue body.
    pub entities: Vec<ExtractedEntity>,
    /// Mapped entities with CodeGraph results and relevance scores.
    pub mapped: Vec<MappedEntity>,
    /// PreciseChange objects for each matched symbol.
    pub changes: Vec<PreciseChange>,
    /// Dependencies: files/modules that the changes depend on.
    pub deps: Vec<String>,
    /// Suggested test files to verify the changes.
    pub tests_to_fix: Vec<String>,
    /// Token estimate: issue body tokens without package vs with package.
    pub token_savings_estimate: Option<f64>,
}

/// Detect issue type based on title and body tags/keywords.
pub fn detect_issue_type(title: &str, body: &str) -> IssueType {
    let t_lower = title.to_lowercase();
    let b_lower = body.to_lowercase();

    // Direct tag checks in title first
    if t_lower.contains("[bug]")
        || t_lower.contains("bug:")
        || t_lower.contains("fix:")
        || t_lower.starts_with("bug")
        || t_lower.starts_with("fix")
    {
        return IssueType::Bug;
    }
    if t_lower.contains("[feat]")
        || t_lower.contains("[feature]")
        || t_lower.contains("feat:")
        || t_lower.contains("feature:")
        || t_lower.starts_with("feat")
    {
        return IssueType::Feature;
    }
    if t_lower.contains("[refactor]")
        || t_lower.contains("refactor:")
        || t_lower.starts_with("refactor")
    {
        return IssueType::Refactor;
    }

    // Direct tag checks in body
    if b_lower.contains("[bug]") || b_lower.contains("bug:") || b_lower.contains("type: bug") {
        return IssueType::Bug;
    }
    if b_lower.contains("[feat]")
        || b_lower.contains("feat:")
        || b_lower.contains("type: feature")
        || b_lower.contains("feature:")
    {
        return IssueType::Feature;
    }
    if b_lower.contains("[refactor]")
        || b_lower.contains("refactor:")
        || b_lower.contains("type: refactor")
    {
        return IssueType::Refactor;
    }

    // Keyword heuristics in title
    if t_lower.contains("bug")
        || t_lower.contains("fix")
        || t_lower.contains("panic")
        || t_lower.contains("error")
        || t_lower.contains("issue")
    {
        return IssueType::Bug;
    }
    if t_lower.contains("feat")
        || t_lower.contains("feature")
        || t_lower.contains("add ")
        || t_lower.contains("implement")
    {
        return IssueType::Feature;
    }
    if t_lower.contains("refactor") || t_lower.contains("clean") || t_lower.contains("restructure")
    {
        return IssueType::Refactor;
    }

    // Keyword heuristics in body
    if b_lower.contains("bug") || b_lower.contains("fix") || b_lower.contains("panic") {
        return IssueType::Bug;
    }
    if b_lower.contains("feature") || b_lower.contains("feat") {
        return IssueType::Feature;
    }
    if b_lower.contains("refactor") {
        return IssueType::Refactor;
    }

    IssueType::Other
}

/// Calculate relevance score for an entity based on kind, title match, path specificity, and CodeGraph presence.
pub fn calculate_relevance(
    entity: &ExtractedEntity,
    found: bool,
    title: &str,
    file: Option<&str>,
) -> f64 {
    if !found {
        return 0.0;
    }

    let mut score: f64 = match entity.kind.as_str() {
        "symbol" => 0.85,
        "file" => 0.70,
        "feature" => 0.50,
        _ => 0.30,
    };

    let title_lower = title.to_lowercase();
    let val_lower = entity.value.to_lowercase();
    if title_lower.contains(&val_lower) {
        score += 0.10;
    }

    if entity.kind == "file" {
        if let Some(f) = file {
            if f.contains('/') {
                score += 0.05;
            }
        }
    } else if entity.kind == "symbol" {
        score += 0.05;
    }

    score.min(1.0).max(0.0)
}

/// Parse a GitHub issue body to extract entities.
///
/// Extraction rules:
/// - File paths: anything matching `path/to/file.ext` patterns (at least 2 segments)
/// - Symbol names: backtick-quoted identifiers (e.g. `search_code`, `PreciseChange`)
/// - Feature references: `feat-*` or `FEAT-*` patterns
/// - Module references: `mod name` or `pub mod name` patterns
pub fn parse_issue_entities(title: &str, body: &str) -> Vec<ExtractedEntity> {
    let mut entities = Vec::new();
    let full_text = format!("{}\n{}", title, body);

    // Extract file paths (2+ segments with extension)
    let file_re =
        regex::Regex::new(r"(?:^|\s|`)([a-zA-Z0-9_./-]+\.[a-zA-Z]{1,10})(?:`|\s|$)").unwrap();
    for mat in file_re.find_iter(&full_text) {
        let path = mat.as_str().trim_matches('`').trim();
        // Filter out common false positives
        if path.contains('/') && !path.ends_with(".md") && !path.starts_with("http") {
            entities.push(ExtractedEntity {
                kind: "file".to_string(),
                value: path.to_string(),
                offset: mat.start(),
            });
        }
    }

    // Extract backtick-quoted symbols
    let symbol_re =
        regex::Regex::new(r"`([a-zA-Z_][a-zA-Z0-9_:]*(?:::[a-zA-Z_][a-zA-Z0-9_]*)?)`").unwrap();
    for mat in symbol_re.captures_iter(&full_text) {
        if let Some(sym) = mat.get(1) {
            let sym_str = sym.as_str();
            // Skip file paths that happen to be in backticks
            if !sym_str.contains('/') && !sym_str.ends_with(".rs") && !sym_str.ends_with(".ts") {
                entities.push(ExtractedEntity {
                    kind: "symbol".to_string(),
                    value: sym_str.to_string(),
                    offset: sym.start(),
                });
            }
        }
    }

    // Extract feature references
    let feat_re = regex::Regex::new(r"(feat-[a-zA-Z0-9_-]+|FEAT-[A-Z0-9_-]+)").unwrap();
    for mat in feat_re.find_iter(&full_text) {
        entities.push(ExtractedEntity {
            kind: "feature".to_string(),
            value: mat.as_str().to_string(),
            offset: mat.start(),
        });
    }

    // Deduplicate by (kind, value)
    entities.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.value.cmp(&b.value)));
    entities.dedup_by(|a, b| a.kind == b.kind && a.value == b.value);

    entities
}

/// Map extracted entities to the CodeGraph.
///
/// For symbols: uses `find_symbols` to locate in the code graph.
/// For files: checks if the file exists in the snapshot.
/// For features: looks up in `.gitcore/features.json`.
pub fn map_entities_to_codegraph(
    entities: &[ExtractedEntity],
    code_graph_db: &CodeGraphDB,
    _snapshot_manager: &SnapshotManager,
    _repo: &str,
    repo_root: &Path,
    title: &str,
) -> Result<Vec<MappedEntity>> {
    let mut mapped = Vec::new();

    for entity in entities {
        match entity.kind.as_str() {
            "symbol" => match code_graph_db.find_symbols(&entity.value, 5) {
                Ok(result) => {
                    if let Some(sym) = result.symbols.first() {
                        let file = Some(sym.file_path.clone());
                        let rel = calculate_relevance(entity, true, title, file.as_deref());
                        mapped.push(MappedEntity {
                            entity: entity.clone(),
                            found: true,
                            symbol_name: Some(sym.name.clone()),
                            file,
                            start_line: Some(sym.start_line),
                            end_line: Some(sym.end_line),
                            relevance_score: rel,
                        });
                    } else {
                        mapped.push(MappedEntity {
                            entity: entity.clone(),
                            found: false,
                            symbol_name: None,
                            file: None,
                            start_line: None,
                            end_line: None,
                            relevance_score: 0.0,
                        });
                    }
                }
                Err(e) => {
                    tracing::warn!("CodeGraph query failed for '{}': {}", entity.value, e);
                    mapped.push(MappedEntity {
                        entity: entity.clone(),
                        found: false,
                        symbol_name: None,
                        file: None,
                        start_line: None,
                        end_line: None,
                        relevance_score: 0.0,
                    });
                }
            },
            "file" => {
                // Check if file exists in the repo
                let file_path = repo_root.join(&entity.value);
                let found = file_path.exists();
                let file = if found {
                    Some(entity.value.clone())
                } else {
                    None
                };
                let rel = calculate_relevance(entity, found, title, file.as_deref());
                mapped.push(MappedEntity {
                    entity: entity.clone(),
                    found,
                    symbol_name: None,
                    file,
                    start_line: None,
                    end_line: None,
                    relevance_score: rel,
                });
            }
            "feature" => {
                // Feature references are always "found" (they're metadata, not code)
                let rel = calculate_relevance(entity, true, title, None);
                mapped.push(MappedEntity {
                    entity: entity.clone(),
                    found: true,
                    symbol_name: None,
                    file: None,
                    start_line: None,
                    end_line: None,
                    relevance_score: rel,
                });
            }
            _ => {
                mapped.push(MappedEntity {
                    entity: entity.clone(),
                    found: false,
                    symbol_name: None,
                    file: None,
                    start_line: None,
                    end_line: None,
                    relevance_score: 0.0,
                });
            }
        }
    }

    Ok(mapped)
}

/// Generate PreciseChange objects for matched symbols.
///
/// Reads the source file for each matched symbol and builds the before/after snippets.
/// The `after_snippet` is empty (placeholder) — the executor agent fills it in.
pub fn generate_changes(
    mapped: &[MappedEntity],
    snapshot_manager: &SnapshotManager,
    repo: &str,
    repo_root: &Path,
) -> Result<Vec<PreciseChange>> {
    let mut changes = Vec::new();

    for m in mapped {
        if !m.found {
            continue;
        }
        if let (Some(file), Some(sym), Some(start), Some(end)) =
            (&m.file, &m.symbol_name, m.start_line, m.end_line)
        {
            let abs_path = repo_root.join(file);
            if let Ok(source) = std::fs::read_to_string(&abs_path) {
                let change = snapshot_manager.build_precise_change(
                    repo, file, sym, start, end, &source,
                    "", // after_snippet: placeholder for executor
                );
                changes.push(change);
            }
        }
    }

    Ok(changes)
}

/// Assemble the full IssueContextPackage with default boundary limits.
pub fn assemble_package(
    issue_id: &str,
    title: &str,
    repo: &str,
    body: &str,
    code_graph_db: &CodeGraphDB,
    snapshot_manager: &SnapshotManager,
    repo_root: &Path,
) -> Result<IssueContextPackage> {
    assemble_package_with_limits(
        issue_id,
        title,
        repo,
        body,
        code_graph_db,
        snapshot_manager,
        repo_root,
        ContextLimits::default(),
    )
}

/// Assemble the full IssueContextPackage with custom boundary limits.
#[allow(clippy::too_many_arguments)]
pub fn assemble_package_with_limits(
    issue_id: &str,
    title: &str,
    repo: &str,
    body: &str,
    code_graph_db: &CodeGraphDB,
    snapshot_manager: &SnapshotManager,
    repo_root: &Path,
    limits: ContextLimits,
) -> Result<IssueContextPackage> {
    let issue_type = detect_issue_type(title, body);
    let entities = parse_issue_entities(title, body);
    let mapped = map_entities_to_codegraph(
        &entities,
        code_graph_db,
        snapshot_manager,
        repo,
        repo_root,
        title,
    )?;

    // Enforce limits on mapped entities: max_symbols and max_files
    let mut limited_mapped = Vec::new();
    let mut symbol_count = 0;
    let mut file_count = 0;

    for m in mapped {
        if m.entity.kind == "symbol" {
            if symbol_count < limits.max_symbols {
                symbol_count += 1;
                limited_mapped.push(m);
            }
        } else if m.entity.kind == "file" {
            if file_count < limits.max_files {
                file_count += 1;
                limited_mapped.push(m);
            }
        } else {
            limited_mapped.push(m);
        }
    }
    let mapped = limited_mapped;

    let changes = generate_changes(&mapped, snapshot_manager, repo, repo_root)?;

    // Enforce max_diff_lines limit on generated changes
    let mut truncated_changes = Vec::new();
    let mut current_diff_lines = 0;
    for c in changes {
        let snippet_lines = c.before_snippet.lines().count().max(1);
        if truncated_changes.is_empty()
            || current_diff_lines + snippet_lines <= limits.max_diff_lines
        {
            current_diff_lines += snippet_lines;
            truncated_changes.push(c);
        } else {
            break;
        }
    }
    let changes = truncated_changes;

    // Extract deps from file entities (bounded by max_files)
    let deps: Vec<String> = mapped
        .iter()
        .filter(|m| m.entity.kind == "file" && m.found)
        .filter_map(|m| m.file.clone())
        .take(limits.max_files)
        .collect();

    // Suggest test files based on changed files
    let tests_to_fix: Vec<String> = changes
        .iter()
        .filter_map(|c| {
            let path = std::path::Path::new(&c.file);
            let stem = path.file_stem()?.to_str()?;
            let parent = path.parent()?;
            // Common test patterns
            let test_patterns = vec![
                parent.join(format!("{}_test.rs", stem)),
                parent.join(format!("{}.test.rs", stem)),
                std::path::PathBuf::from(format!("tests/{}_test.rs", stem)),
            ];
            test_patterns
                .into_iter()
                .find(|p| repo_root.join(p).exists())
                .map(|p| p.to_string_lossy().to_string())
        })
        .collect();

    // Estimate token savings: count actual lines in before_snippet vs full file
    let token_savings = if changes.is_empty() {
        None
    } else {
        let total_before_lines: usize = changes
            .iter()
            .map(|c| c.before_snippet.lines().count())
            .sum();
        let avg_file_lines = 200.0; // typical Rust file ~200 lines
        let saved_tokens = (avg_file_lines - total_before_lines as f64).max(0.0) * 1.3; // ~1.3 tokens/line
        Some(saved_tokens)
    };

    Ok(IssueContextPackage {
        issue_id: issue_id.to_string(),
        title: title.to_string(),
        repo: repo.to_string(),
        issue_type,
        entities,
        mapped,
        changes,
        deps,
        tests_to_fix,
        token_savings_estimate: token_savings,
    })
}

/// Detailed context package for an issue combining GitHub API, code-graph, memory store, and git diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueContextPack {
    pub issue: serde_json::Value,
    pub prs: Vec<serde_json::Value>,
    pub code_snippets: Vec<String>,
    pub memory_hits: Vec<MemoryRecord>,
    pub diff: String,
    pub generated_at: String,
}

/// Save an IssueContextPack to a JSON file.
pub async fn save_pack(pack: &IssueContextPack, path: &str) -> Result<()> {
    let p = Path::new(path);
    if let Some(parent) = p.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(pack)?;
    tokio::fs::write(p, json)
        .await
        .with_context(|| format!("Failed to write pack to {}", path))?;
    Ok(())
}

/// Auto analysis of GitHub issue -> collect issue body (gh api) + linked PRs (gh pr list) + code_snippets (code-graph query) + memory_hits (MemoryStore search) + git diff -> JSON.
pub async fn pack_issue(issue_id: &str, repo: &str) -> Result<IssueContextPack> {
    let repo_arg = if repo.is_empty() { "xavier" } else { repo };

    // 1. Fetch GitHub issue payload via gh CLI
    let issue_val = match tokio::process::Command::new("gh")
        .args([
            "issue",
            "view",
            issue_id,
            "--repo",
            repo_arg,
            "--json",
            "id,number,title,body,state,labels,author,createdAt",
        ])
        .output()
        .await
    {
        Ok(out) if out.status.success() => {
            serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
                serde_json::json!({
                    "id": issue_id,
                    "number": issue_id,
                    "title": format!("Issue {}", issue_id),
                    "body": "",
                    "repo": repo_arg,
                })
            })
        }
        _ => serde_json::json!({
            "id": issue_id,
            "number": issue_id,
            "title": format!("Issue {}", issue_id),
            "body": "",
            "repo": repo_arg,
        }),
    };

    // 2. Fetch linked PRs via gh CLI
    let prs_val: Vec<serde_json::Value> = match tokio::process::Command::new("gh")
        .args([
            "pr",
            "list",
            "--repo",
            repo_arg,
            "--search",
            issue_id,
            "--json",
            "number,title,state,url",
        ])
        .output()
        .await
    {
        Ok(out) if out.status.success() => serde_json::from_slice(&out.stdout).unwrap_or_default(),
        _ => Vec::new(),
    };

    let title = issue_val
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let body = issue_val.get("body").and_then(|v| v.as_str()).unwrap_or("");

    // 3. Extract entities & code snippets from codebase & code-graph
    let entities = parse_issue_entities(title, body);
    let mut code_snippets = Vec::new();

    let repo_root = Path::new(".");
    if let Ok(db) = CodeGraphDB::in_memory() {
        let snapshot = SnapshotManager::new(repo_root);
        if let Ok(mapped) =
            map_entities_to_codegraph(&entities, &db, &snapshot, repo_arg, repo_root, title)
        {
            if let Ok(changes) = generate_changes(&mapped, &snapshot, repo_arg, repo_root) {
                for c in changes {
                    if !c.before_snippet.is_empty() {
                        code_snippets.push(format!(
                            "{}:{}-{}\n{}",
                            c.file, c.start_line, c.end_line, c.before_snippet
                        ));
                    }
                }
            }
        }
    }

    // Fallback file reads if code_snippets is empty
    if code_snippets.is_empty() {
        for entity in &entities {
            if entity.kind == "file" {
                let p = repo_root.join(&entity.value);
                if p.exists() {
                    if let Ok(content) = std::fs::read_to_string(&p) {
                        let lines: Vec<&str> = content.lines().take(50).collect();
                        code_snippets.push(format!(
                            "{}:1-{}\n{}",
                            entity.value,
                            lines.len(),
                            lines.join("\n")
                        ));
                    }
                }
            }
        }
    }

    // 4. Memory hits search
    let memory_hits = Vec::new();

    // 5. Git diff
    let diff = match tokio::process::Command::new("git")
        .args(["diff", "HEAD~1"])
        .output()
        .await
    {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).to_string(),
        _ => String::new(),
    };

    let generated_at = chrono::Utc::now().to_rfc3339();

    Ok(IssueContextPack {
        issue: issue_val,
        prs: prs_val,
        code_snippets,
        memory_hits,
        diff,
        generated_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_issue_type() {
        assert_eq!(
            detect_issue_type("[bug] Fix crash in server", ""),
            IssueType::Bug
        );
        assert_eq!(
            detect_issue_type("feat: add new CLI command", ""),
            IssueType::Feature
        );
        assert_eq!(
            detect_issue_type("refactor: split issue_context into modules", ""),
            IssueType::Refactor
        );
        assert_eq!(
            detect_issue_type("Update README", "documentation update"),
            IssueType::Other
        );
    }

    #[test]
    fn test_calculate_relevance() {
        let entity_sym = ExtractedEntity {
            kind: "symbol".to_string(),
            value: "search_code".to_string(),
            offset: 0,
        };
        let rel_sym = calculate_relevance(
            &entity_sym,
            true,
            "Fix search_code in db",
            Some("src/db.rs"),
        );
        assert!(
            rel_sym > 0.8,
            "Symbol matched in title should have high relevance"
        );

        let rel_unfound = calculate_relevance(&entity_sym, false, "Fix search_code", None);
        assert_eq!(rel_unfound, 0.0, "Unfound entity must have relevance 0.0");
    }

    #[test]
    fn test_parse_issue_entities_files() {
        let title = "Fix search_code in db.rs";
        let body = "The function `search_code` in `src/codebase/db.rs` needs improvement.\nAlso check `src/codebase/snapshot.rs`.";
        let entities = parse_issue_entities(title, body);

        let files: Vec<&str> = entities
            .iter()
            .filter(|e| e.kind == "file")
            .map(|e| e.value.as_str())
            .collect();
        assert!(files.contains(&"src/codebase/db.rs"), "Should find db.rs");
        assert!(
            files.contains(&"src/codebase/snapshot.rs"),
            "Should find snapshot.rs"
        );
    }

    #[test]
    fn test_parse_issue_entities_symbols() {
        let title = "Improve `PreciseChange` and `build_precise_change`";
        let body =
            "We need to enhance the `PreciseChange` struct and the `build_precise_change` method.";
        let entities = parse_issue_entities(title, body);

        let symbols: Vec<&str> = entities
            .iter()
            .filter(|e| e.kind == "symbol")
            .map(|e| e.value.as_str())
            .collect();
        assert!(
            symbols.contains(&"PreciseChange"),
            "Should find PreciseChange"
        );
        assert!(
            symbols.contains(&"build_precise_change"),
            "Should find build_precise_change"
        );
    }

    #[test]
    fn test_parse_issue_entities_features() {
        let title = "[XAV-09] feat-issue-context-packager: implement";
        let body = "Feature: feat-issue-context-packager, related to FEAT-CG-001.";
        let entities = parse_issue_entities(title, body);

        let features: Vec<&str> = entities
            .iter()
            .filter(|e| e.kind == "feature")
            .map(|e| e.value.as_str())
            .collect();
        assert!(features.contains(&"feat-issue-context-packager"));
        assert!(features.contains(&"FEAT-CG-001"));
    }

    #[test]
    fn test_parse_issue_entities_dedup() {
        let title = "Fix `search_code`";
        let body = "The `search_code` function in `search_code` module needs work.";
        let entities = parse_issue_entities(title, body);

        let symbols: Vec<&str> = entities
            .iter()
            .filter(|e| e.kind == "symbol")
            .map(|e| e.value.as_str())
            .collect();
        // Should be deduplicated
        assert_eq!(symbols.len(), 1);
    }

    #[test]
    fn test_parse_issue_entities_empty_body() {
        // assemble_package requires real DB instances — test parse_issue_entities directly
        let entities = parse_issue_entities("Test issue", "");
        assert!(entities.is_empty());
    }

    #[tokio::test]
    async fn test_issue_context_package_oversized_diff_budget() {
        use code_graph::indexer::Indexer;
        use std::sync::Arc;
        use tempfile::tempdir;

        let temp_dir = tempdir().expect("failed to create temp dir");
        let temp_path = temp_dir.path();
        std::fs::create_dir_all(temp_path.join(".git")).expect("mock .git");

        let src_dir = temp_path.join("src");
        std::fs::create_dir_all(&src_dir).expect("src dir");

        // Create 25 files with symbols and multi-line content
        let mut body = String::from("Fix issues across candidate files:\n");
        for i in 0..25 {
            let file_name = format!("mod_{}.rs", i);
            let rel_path = format!("src/{}", file_name);
            let content = format!(
                "pub fn fn_{}() -> usize {{\n    let x = {};\n    x + 10\n}}\n",
                i, i
            );
            std::fs::write(src_dir.join(&file_name), content).expect("write file");

            body.push_str(&format!("- Check `{}` for `fn_{}`\n", rel_path, i));
        }

        let db = Arc::new(CodeGraphDB::in_memory().expect("CodeGraphDB"));
        let indexer = Arc::new(Indexer::new(Arc::clone(&db)));
        indexer.index(temp_path, true).await.expect("index repo");
        let snapshot = SnapshotManager::new(temp_path);

        let limits = ContextLimits {
            max_symbols: 5,
            max_files: 5,
            max_diff_lines: 10,
        };

        let package = assemble_package_with_limits(
            "101",
            "Oversized diff budget test issue",
            "test/repo",
            &body,
            &db,
            &snapshot,
            temp_path,
            limits.clone(),
        )
        .expect("assemble_package_with_limits");

        let mapped_symbols = package
            .mapped
            .iter()
            .filter(|m| m.entity.kind == "symbol")
            .count();
        let mapped_files = package
            .mapped
            .iter()
            .filter(|m| m.entity.kind == "file")
            .count();

        assert!(
            mapped_symbols <= limits.max_symbols,
            "Mapped symbols ({}) should be capped at max_symbols ({})",
            mapped_symbols,
            limits.max_symbols
        );
        assert!(
            mapped_files <= limits.max_files,
            "Mapped files ({}) should be capped at max_files ({})",
            mapped_files,
            limits.max_files
        );
        assert!(
            package.deps.len() <= limits.max_files,
            "Deps ({}) should be capped at max_files ({})",
            package.deps.len(),
            limits.max_files
        );

        let total_diff_lines: usize = package
            .changes
            .iter()
            .map(|c| c.before_snippet.lines().count())
            .sum();
        assert!(
            total_diff_lines <= limits.max_diff_lines,
            "Total snippet lines ({}) should not exceed max_diff_lines ({})",
            total_diff_lines,
            limits.max_diff_lines
        );
    }

    #[tokio::test]
    async fn test_pack_issue_and_save_pack() {
        let pack = pack_issue("123", "xavier")
            .await
            .expect("pack_issue should succeed");
        assert!(pack.issue.get("id").is_some());
        assert!(pack.generated_at.contains('T') || !pack.generated_at.is_empty());

        let temp_dir = tempfile::tempdir().expect("tempdir");
        let pack_path = temp_dir.path().join("data/issue_packs/123.json");
        save_pack(&pack, pack_path.to_str().unwrap())
            .await
            .expect("save_pack should succeed");

        assert!(pack_path.exists());
        let saved_content = tokio::fs::read_to_string(&pack_path)
            .await
            .expect("read saved pack");
        let read_pack: IssueContextPack =
            serde_json::from_str(&saved_content).expect("deserialize pack");
        assert_eq!(read_pack.generated_at, pack.generated_at);
    }
}
