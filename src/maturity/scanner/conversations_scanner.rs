// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! # Conversations Scanner — Layer 4: Project Intelligence
//!
//! Analyzes discussions, issues, and PRs by examining project artifacts:
//! - `.gitcore/features.json` — existing feature tracking
//! - Memory files mentioning features
//! - TODO/FIXME/HACK patterns in feature-related code
//!
//! Timing target: ~5s.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

/// Conversation evidence for one feature.
#[derive(Debug, Clone)]
pub struct ConversationEvidence {
    /// Number of memory/discussion files mentioning this feature
    pub memory_mentions: usize,
    /// Number of TODO items found for this feature
    pub todo_count: usize,
    /// Number of FIXME items found for this feature
    pub fixme_count: usize,
    /// Whether a features.json entry exists for this feature
    pub has_features_entry: bool,
    /// Evidence ratio: 0.0 - 1.0 (for scoring)
    pub ratio: f64,
}

/// Full conversations scan result.
#[derive(Debug, Clone)]
pub struct ConversationScanResult {
    pub feature_evidence: HashMap<String, ConversationEvidence>,
    pub errors: Vec<String>,
    pub timing_ms: u64,
}

/// Load feature keywords from anchors manifest.
fn get_feature_keywords(codebase_root: &str) -> HashMap<String, Vec<String>> {
    let anchor_path = Path::new(codebase_root).join(".xavier/maturity-anchors.json");
    let mut map: HashMap<String, Vec<String>> = HashMap::new();

    if let Ok(content) = std::fs::read_to_string(&anchor_path) {
        if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(features) = manifest.get("features").and_then(|f| f.as_array()) {
                for feat in features {
                    let id = feat.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
                    let mut keywords = Vec::new();

                    // Primary identifier variants
                    keywords.push(id.to_string());
                    keywords.push(id.replace("-", "_"));
                    keywords.push(id.replace("-", " "));

                    // Also collect from subcomponent names
                    if let Some(subs) = feat.get("subcomponents").and_then(|s| s.as_array()) {
                        for sub in subs {
                            if let Some(name) = sub.get("name").and_then(|v| v.as_str()) {
                                for word in name.split_whitespace() {
                                    let clean = word
                                        .trim_matches(|c: char| c.is_ascii_punctuation())
                                        .to_lowercase();
                                    if clean.len() > 3 && !keywords.contains(&clean) {
                                        keywords.push(clean);
                                    }
                                }
                            }
                        }
                    }

                    map.insert(id.to_string(), keywords);
                }
            }
        }
    }
    map
}

/// Check .gitcore/features.json for existing feature tracking entries.
fn check_features_entry(root: &str, feat_id: &str) -> bool {
    let features_path = Path::new(root).join(".gitcore/features.json");
    if !features_path.exists() {
        return false;
    }

    if let Ok(content) = std::fs::read_to_string(&features_path) {
        content.contains(feat_id) || content.contains(&feat_id.replace("-", "_"))
    } else {
        false
    }
}

/// Count mentions in memory/ directory files.
fn count_memory_mentions(root: &str, keywords: &[String]) -> usize {
    let mut count = 0usize;

    // Check memory directory at project root
    let memory_dir = Path::new(root).join("memory");
    if memory_dir.exists() {
        let walker = walkdir::WalkDir::new(&memory_dir)
            .max_depth(2)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"));

        for entry in walker {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                for kw in keywords {
                    count += content.matches(kw.as_str()).count();
                }
            }
        }
    }

    // Also check ~/.xavier/ directory for discussion files
    let xavier_dir = format!("{}/.xavier", root);
    let xavier_path = Path::new(&xavier_dir);
    if xavier_path.exists() {
        let walker = walkdir::WalkDir::new(xavier_path)
            .max_depth(2)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .is_some_and(|ext| ext == "md" || ext == "json")
            });

        for entry in walker {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                for kw in keywords {
                    count += content.matches(kw.as_str()).count();
                }
            }
        }
    }

    count
}

/// Count TODO and FIXME items in feature-related source files.
fn count_todos_and_fixmes(root: &str, keywords: &[String]) -> (usize, usize) {
    let mut todos = 0usize;
    let mut fixmes = 0usize;

    let walker = walkdir::WalkDir::new(root)
        .max_depth(8)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().is_some_and(|ext| ext == "rs")
                && !e.path().to_string_lossy().contains("target")
        })
        .take(200);

    for entry in walker {
        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            let has_keyword = keywords.iter().any(|kw| content.contains(kw.as_str()));
            if has_keyword {
                for line in content.lines() {
                    let lower = line.to_lowercase();
                    if lower.contains("todo") {
                        todos += 1;
                    }
                    if lower.contains("fixme") {
                        fixmes += 1;
                    }
                }
            }
        }
    }

    (todos, fixmes)
}

/// Main entry: scan conversation evidence for all features.
pub fn scan_conversations(codebase_root: &str) -> ConversationScanResult {
    let start = Instant::now();
    let feature_keywords = get_feature_keywords(codebase_root);
    let errors: Vec<String> = Vec::new();
    let mut feature_evidence: HashMap<String, ConversationEvidence> = HashMap::new();

    for (feat_id, keywords) in &feature_keywords {
        let memory_mentions = count_memory_mentions(codebase_root, keywords);
        let (todos, fixmes) = count_todos_and_fixmes(codebase_root, keywords);
        let has_features_entry = check_features_entry(codebase_root, feat_id);

        // Calculate ratio:
        // Memory mentions: up to 10 = 1.0
        let mention_ratio = (memory_mentions as f64 / 10.0).min(1.0);
        // TODO/FIXME: fewer is better (up to 5 = 0.0)
        let todo_penalty = (todos as f64 / 5.0).min(1.0) * 0.5;
        let fixme_penalty = (fixmes as f64 / 5.0).min(1.0) * 0.5;
        let todo_ratio = (1.0 - todo_penalty - fixme_penalty).max(0.0);
        // Features entry: yes = 1.0, no = 0.0
        let entry_bonus = if has_features_entry { 1.0 } else { 0.0 };

        // Combined: mentions 30%, todo health 30%, entry bonus 40%
        let ratio = mention_ratio * 0.3 + todo_ratio * 0.3 + entry_bonus * 0.4;

        feature_evidence.insert(
            feat_id.clone(),
            ConversationEvidence {
                memory_mentions,
                todo_count: todos,
                fixme_count: fixmes,
                has_features_entry,
                ratio,
            },
        );
    }

    let timing_ms = start.elapsed().as_millis() as u64;

    ConversationScanResult {
        feature_evidence,
        errors,
        timing_ms,
    }
}
