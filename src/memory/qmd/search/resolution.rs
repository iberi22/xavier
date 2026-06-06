//! Metadata resolution and answer extraction.
//!
//! Resolves rich metadata from document path and metadata,
//! and extracts answer snippets from document content.

use regex::Regex;
use std::sync::LazyLock;

use crate::memory::qmd_memory::types::MemoryDocument;

/// Resolve rich metadata from a document.
pub fn resolved_doc_metadata(
    doc: &MemoryDocument,
) -> Option<crate::memory::schema::ResolvedMemoryMetadata> {
    let workspace_id = doc
        .metadata
        .get("namespace")
        .and_then(|value| value.get("workspace_id"))
        .and_then(|value| value.as_str())
        .or_else(|| {
            doc.metadata
                .get("workspace_id")
                .and_then(|value| value.as_str())
        })
        .unwrap_or("default");
    crate::memory::schema::resolve_metadata(&doc.path, &doc.metadata, workspace_id, None).ok()
}

/// Extract an answer snippet from content based on category.
///
/// Categories:
/// - `"2"` ÔåÆ Date extraction
/// - `"3"` ÔåÆ Opinion extraction
/// - `"4"` ÔåÆ Future plans extraction
/// - Default ÔåÆ first sentence
pub fn extract_answer(content: &str, category: &str) -> Option<String> {
    let text = content.trim();
    if text.is_empty() {
        return None;
    }

    match category {
        "2" => {
            static DATE_RE: LazyLock<Regex> = LazyLock::new(|| {
                Regex::new(r"(?i)\b(?:\d{1,2}\s+[A-Za-z]+\s+\d{4}|[A-Za-z]+\s+\d{1,2},\s+\d{4}|(19|20)\d{2})\b").expect("test assertion")
            });
            DATE_RE.find(text).map(|m| m.as_str().trim().to_string())
        }
        "3" => {
            let sentence = text
                .split(['.', '!', '?'])
                .map(str::trim)
                .find(|sentence| {
                    let lowered = sentence.to_lowercase();
                    [
                        "think",
                        "believe",
                        "feel",
                        "guess",
                        "suppose",
                        "probably",
                        "definitely",
                        "maybe",
                        "opinion",
                        "view",
                        "perspective",
                        "seems",
                        "appears",
                        "likely",
                        "certainly",
                        "perhaps",
                        "wonder",
                    ]
                    .iter()
                    .any(|keyword| lowered.contains(keyword))
                })
                .or_else(|| {
                    text.split(['.', '!', '?'])
                        .map(str::trim)
                        .find(|s| !s.is_empty())
                })?;
            Some(sentence.to_string())
        }
        "4" => {
            let sentence = text
                .split(['.', '!', '?'])
                .map(str::trim)
                .find(|sentence| {
                    let lowered = sentence.to_lowercase();
                    [
                        "decided",
                        "planning",
                        "planned",
                        "will",
                        "going to",
                        "intend",
                        "promised",
                        "try",
                        "started",
                        "beginning",
                        "began",
                        "going to start",
                        "want to",
                        "hoping to",
                        "aiming to",
                    ]
                    .iter()
                    .any(|keyword| lowered.contains(keyword))
                })
                .or_else(|| {
                    text.split(['.', '!', '?'])
                        .map(str::trim)
                        .find(|s| !s.is_empty())
                })?;
            Some(sentence.to_string())
        }
        _ => text
            .split(['.', '!', '?'])
            .map(str::trim)
            .find(|sentence| !sentence.is_empty())
            .map(|sentence| sentence.to_string()),
    }
}
