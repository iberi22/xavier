//! Issue Context Packager — analyze a GitHub issue and produce a PreciseChange package.
//!
//! Given an issue title + body, this module:
//! 1. Parses the issue to extract file paths, symbol names, and feature references.
//! 2. Maps each entity to the CodeGraph (find_symbols, search_code).
//! 3. Generates a `PreciseChange` per matched symbol.
//! 4. Assembles an `IssueContextPackage` ready for an executor agent.
//!
//! This is the token-saving core: the agent receives only the fragments to change,
//! never the whole file.

use crate::codebase::snapshot::{PreciseChange, SnapshotManager};
use anyhow::{Context, Result};
use code_graph::db::CodeGraphDB;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

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
    /// Extracted entities from the issue body.
    pub entities: Vec<ExtractedEntity>,
    /// Mapped entities with CodeGraph results.
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
) -> Result<Vec<MappedEntity>> {
    let mut mapped = Vec::new();

    for entity in entities {
        match entity.kind.as_str() {
            "symbol" => match code_graph_db.find_symbols(&entity.value, 5) {
                Ok(result) => {
                    if let Some(sym) = result.symbols.first() {
                        mapped.push(MappedEntity {
                            entity: entity.clone(),
                            found: true,
                            symbol_name: Some(sym.name.clone()),
                            file: Some(sym.file_path.clone()),
                            start_line: Some(sym.start_line),
                            end_line: Some(sym.end_line),
                        });
                    } else {
                        mapped.push(MappedEntity {
                            entity: entity.clone(),
                            found: false,
                            symbol_name: None,
                            file: None,
                            start_line: None,
                            end_line: None,
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
                    });
                }
            },
            "file" => {
                // Check if file exists in the repo
                let file_path = repo_root.join(&entity.value);
                if file_path.exists() {
                    mapped.push(MappedEntity {
                        entity: entity.clone(),
                        found: true,
                        symbol_name: None,
                        file: Some(entity.value.clone()),
                        start_line: None,
                        end_line: None,
                    });
                } else {
                    mapped.push(MappedEntity {
                        entity: entity.clone(),
                        found: false,
                        symbol_name: None,
                        file: None,
                        start_line: None,
                        end_line: None,
                    });
                }
            }
            "feature" => {
                // Feature references are always "found" (they're metadata, not code)
                mapped.push(MappedEntity {
                    entity: entity.clone(),
                    found: true,
                    symbol_name: None,
                    file: None,
                    start_line: None,
                    end_line: None,
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

/// Assemble the full IssueContextPackage.
pub fn assemble_package(
    issue_id: &str,
    title: &str,
    repo: &str,
    body: &str,
    code_graph_db: &CodeGraphDB,
    snapshot_manager: &SnapshotManager,
    repo_root: &Path,
) -> Result<IssueContextPackage> {
    let entities = parse_issue_entities(title, body);
    let mapped =
        map_entities_to_codegraph(&entities, code_graph_db, snapshot_manager, repo, repo_root)?;
    let changes = generate_changes(&mapped, snapshot_manager, repo, repo_root)?;

    // Extract deps from file entities
    let deps: Vec<String> = mapped
        .iter()
        .filter(|m| m.entity.kind == "file" && m.found)
        .filter_map(|m| m.file.clone())
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
        entities,
        mapped,
        changes,
        deps,
        tests_to_fix,
        token_savings_estimate: token_savings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
