//! # Memory Scanner — Layer 3: Evidence from Sessions & Usages
//!
//! Scans Xavier's session database, memory store, and source code to
//! determine how much real-world evidence exists for each feature.
//!
//! Timing target: ~5s.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

/// Memory evidence for one feature.
#[derive(Debug, Clone)]
pub struct MemoryEvidence {
    /// Number of source files that reference this feature
    pub source_hits: usize,
    /// Number of sessions (if session DB available) that mention this feature
    pub session_mentions: usize,
    /// Number of errors found related to this feature
    pub error_count: usize,
    /// Number of imports/usages found
    pub usage_count: usize,
    /// Evidence ratio: 0.0 - 1.0 (for scoring)
    pub ratio: f64,
}

/// Full memory scan result.
#[derive(Debug, Clone)]
pub struct MemoryScanResult {
    pub feature_evidence: HashMap<String, MemoryEvidence>,
    pub errors: Vec<String>,
    pub timing_ms: u64,
}

/// Keywords that help identify each feature in code and sessions.
/// These should match the memory_keywords from the anchors manifest.
fn get_feature_keywords(codebase_root: &str) -> HashMap<String, Vec<String>> {
    let anchor_path = Path::new(codebase_root).join(".xavier/maturity-anchors.json");
    let mut map: HashMap<String, Vec<String>> = HashMap::new();

    if let Ok(content) = std::fs::read_to_string(&anchor_path) {
        if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(features) = manifest.get("features").and_then(|f| f.as_array()) {
                for feat in features {
                    let id = feat.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
                    use std::collections::HashSet;
                    let mut keywords: HashSet<String> = HashSet::new();

                    // Collect from memory_keywords field
                    if let Some(subs) = feat.get("subcomponents").and_then(|s| s.as_array()) {
                        for sub in subs {
                            if let Some(kws) = sub.get("memory_keywords").and_then(|k| k.as_array())
                            {
                                for kw in kws {
                                    if let Some(k) = kw.as_str() {
                                        keywords.insert(k.to_string());
                                    }
                                }
                            }
                            // Also extract keywords from subcomponent names
                            if let Some(name) = sub.get("name").and_then(|v| v.as_str()) {
                                for word in name.split_whitespace() {
                                    if word.len() > 3 {
                                        keywords.insert(word.to_lowercase());
                                    }
                                }
                            }
                        }
                    }

                    // Add feature id as keyword
                    keywords.insert(id.to_string());
                    // Add common variations
                    keywords.insert(id.replace("-", "_"));
                    keywords.insert(id.replace("-", ""));

                    map.insert(id.to_string(), keywords.into_iter().collect());
                }
            }
        }
    }
    map
}

/// Count how many source .rs files reference a set of keywords.
fn count_source_hits(root: &str, keywords: &[String]) -> usize {
    let mut hits = 0usize;
    let walker = walkdir::WalkDir::new(root)
        .max_depth(8)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().map_or(false, |ext| ext == "rs")
                && !e.path().to_string_lossy().contains("target")
        })
        .take(300);

    for entry in walker {
        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            if keywords.iter().any(|kw| content.contains(kw.as_str())) {
                hits += 1;
            }
        }
    }
    hits
}

/// Count module declarations and `use` statements for feature keywords.
fn count_usages(root: &str, keywords: &[String]) -> usize {
    let mut count = 0usize;
    let walker = walkdir::WalkDir::new(root)
        .max_depth(8)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().map_or(false, |ext| ext == "rs")
                && !e.path().to_string_lossy().contains("target")
        })
        .take(200);

    for entry in walker {
        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            for kw in keywords {
                // Search for "mod kw", "use kw", "pub use kw"
                if content.contains(&format!("mod {}", kw))
                    || content.contains(&format!("use {}", kw))
                    || content.contains(&format!("use crate::{}", kw))
                    || content.contains(&format!("pub(crate) use {}", kw))
                {
                    count += 1;
                }
            }
        }
    }
    count
}

/// Try to scan Xavier's session database for feature mentions.
fn scan_sessions(root: &str, keywords: &[String]) -> usize {
    let session_db_path = format!("{}/.xavier/sessions.db", root);
    if !Path::new(&session_db_path).exists() {
        return 0;
    }

    let mut mentions = 0usize;

    // Try to open SQLite sessions DB
    // We use a simple grep approach on the SQLite file
    // (proper SQLite query would need rusqlite, which may not be available in maturity module)
    if let Ok(content) = std::fs::read_to_string(&session_db_path) {
        for kw in keywords {
            // Count occurrences (rough proxy for session mentions)
            mentions += content.matches(kw.as_str()).count();
        }
    } else {
        // SQLite binary format — try with strings-like approach
        let output = std::process::Command::new("cmd")
            .args([
                "/C",
                &format!(
                    "findstr /M \"{}\" \"{}\" 2>nul",
                    keywords.join(" "),
                    session_db_path
                ),
            ])
            .output()
            .ok();
        if let Some(out) = output {
            let stdout = String::from_utf8_lossy(&out.stdout);
            mentions = stdout.lines().count();
        }
    }

    mentions
}

/// Count TODO/FIXME/HACK comments related to each feature in the codebase.
fn count_errors(root: &str, keywords: &[String]) -> usize {
    let mut count = 0usize;
    let patterns = ["TODO", "FIXME", "HACK", "XXX", "unimplemented", "panic!"];

    let walker = walkdir::WalkDir::new(root)
        .max_depth(8)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().map_or(false, |ext| ext == "rs")
                && !e.path().to_string_lossy().contains("target")
        })
        .take(200);

    for entry in walker {
        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            let has_keyword = keywords.iter().any(|kw| content.contains(kw.as_str()));
            if has_keyword {
                for pattern in &patterns {
                    if content.contains(pattern) {
                        count += 1;
                    }
                }
            }
        }
    }
    count
}

/// Main entry: scan memory evidence for all features.
pub fn scan_memory(codebase_root: &str) -> MemoryScanResult {
    let start = Instant::now();
    let errors: Vec<String> = Vec::new();
    let feature_keywords = get_feature_keywords(codebase_root);
    let mut feature_evidence: HashMap<String, MemoryEvidence> = HashMap::new();

    for (feat_id, keywords) in &feature_keywords {
        let source_hits = count_source_hits(codebase_root, keywords);
        let usage_count = count_usages(codebase_root, keywords);
        let session_mentions = scan_sessions(codebase_root, keywords);
        let error_count = count_errors(codebase_root, keywords);

        // Calculate ratio: higher is better
        // Source hits: up to 20 files = 1.0
        let source_ratio = (source_hits as f64 / 20.0).min(1.0);
        // Usage: up to 10 usage statements = 1.0
        let usage_ratio = (usage_count as f64 / 10.0).min(1.0);
        // Sessions: up to 50 mentions = 1.0
        let session_ratio = (session_mentions as f64 / 50.0).min(1.0);
        // Errors: invert — fewer errors is better (up to 10 errors = 0.0)
        let error_ratio = (1.0 - (error_count as f64 / 10.0).min(1.0)).max(0.0);

        // Combined: weight source + usage heavily (40% each), session 10%, error inverted 10%
        let ratio =
            source_ratio * 0.4 + usage_ratio * 0.4 + session_ratio * 0.1 + error_ratio * 0.1;

        feature_evidence.insert(
            feat_id.clone(),
            MemoryEvidence {
                source_hits,
                session_mentions,
                error_count,
                usage_count,
                ratio,
            },
        );
    }

    let timing_ms = start.elapsed().as_millis() as u64;

    MemoryScanResult {
        feature_evidence,
        errors,
        timing_ms,
    }
}
