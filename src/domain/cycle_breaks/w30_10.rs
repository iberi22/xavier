//! Shared conversation types, pure helpers, and connection tuning for ADR-033 Wave 0.

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::OnceLock;

/// A single message within a conversation thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub thread_id: String,
    pub role: String,
    pub content: String,
    pub tool_calls: Option<String>,
    pub openui_lang: Option<String>,
    pub xui_json: Option<String>,
    pub metadata: Option<String>,
    pub created_at: DateTime<Utc>,
    pub tokens: Option<i64>,
}

/// Connection tuning implemented by the storage adapter.
pub trait ConnectionTuning {
    type Error;

    fn apply_connection_pragmas(&self) -> Result<(), Self::Error>;
    fn apply_acquire_pragmas(&self) -> Result<(), Self::Error>;
    fn apply_pragmas(&self) -> Result<(), Self::Error>;
    fn maybe_wal_checkpoint(&self) -> Result<bool, Self::Error>;
}

/// Serialize embedding.
pub fn serialize_embedding(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Search tokens.
pub fn search_tokens(query: &str) -> Vec<String> {
    static TOKEN_RE: OnceLock<Option<Regex>> = OnceLock::new();
    let Some(re) = TOKEN_RE.get_or_init(|| Regex::new(r"[A-Za-z0-9][A-Za-z0-9._:/#-]{1,}").ok())
    else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    re.find_iter(query)
        .filter_map(|m| {
            let token = m.as_str().trim_matches('"').trim().to_string();
            if token.len() < 2 {
                return None;
            }
            let lowered = token.to_ascii_lowercase();
            if seen.insert(lowered) {
                Some(token)
            } else {
                None
            }
        })
        .collect()
}

/// Build fts query.
pub fn build_fts_query(query: &str) -> Option<String> {
    let mut tokens = search_tokens(query);
    tokens.extend(code_tokens(query));
    if tokens.is_empty() {
        return None;
    }

    Some(
        tokens
            .into_iter()
            .filter_map(|token| {
                let escaped = token
                    .chars()
                    .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/'))
                    .collect::<String>();
                if escaped.is_empty() {
                    None
                } else {
                    Some(format!("{escaped}*"))
                }
            })
            .collect::<Vec<_>>()
            .join(" OR "),
    )
}

/// Code tokens.
pub fn code_tokens(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut expanded = Vec::new();
    for token in search_tokens(text) {
        for segment in token
            .split(|ch: char| ['_', '-', '/', '.', ':'].contains(&ch))
            .filter(|segment| !segment.is_empty())
        {
            for part in split_camel_case(segment) {
                if part.len() > 1 && seen.insert(part.clone()) {
                    expanded.push(part);
                }
            }
        }
    }
    expanded
}

/// Split camel case.
pub fn split_camel_case(token: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut previous_lower = false;
    for ch in token.chars() {
        if !ch.is_ascii_alphanumeric() {
            if !current.is_empty() {
                words.push(current.clone());
                current.clear();
            }
            previous_lower = false;
            continue;
        }

        let is_upper = ch.is_ascii_uppercase();
        if is_upper && previous_lower && !current.is_empty() {
            words.push(current.clone());
            current.clear();
        }
        previous_lower = ch.is_ascii_lowercase();
        current.push(ch.to_ascii_lowercase());
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// Generates a non-LLM extractive summary of a conversation session
/// using the first and last N messages plus key topics/keywords.
/// Kept under ~400 tokens (~1600 characters) to ensure efficiency.
pub fn summarize_session_extractive(messages: &[Message]) -> String {
    if messages.is_empty() {
        return "### Extractive Session Summary\n\n(No messages in conversation thread)"
            .to_string();
    }

    let mut summary = String::new();
    summary.push_str("### Extractive Session Summary\n\n");

    let total_messages = messages.len();

    // 1. Context flow/previews
    if total_messages <= 6 {
        summary.push_str("**Conversation Flow:**\n");
        for msg in messages {
            let content_clean = msg.content.trim().replace('\n', " ");
            let truncated = if content_clean.len() > 120 {
                format!("{}...", &content_clean[..120])
            } else {
                content_clean
            };
            summary.push_str(&format!("- **{}**: {}\n", msg.role, truncated));
        }
    } else {
        summary.push_str("**First Messages:**\n");
        for msg in &messages[0..3] {
            let content_clean = msg.content.trim().replace('\n', " ");
            let truncated = if content_clean.len() > 120 {
                format!("{}...", &content_clean[..120])
            } else {
                content_clean
            };
            summary.push_str(&format!("- **{}**: {}\n", msg.role, truncated));
        }
        summary.push_str("\n**Last Messages:**\n");
        for msg in &messages[total_messages - 3..] {
            let content_clean = msg.content.trim().replace('\n', " ");
            let truncated = if content_clean.len() > 120 {
                format!("{}...", &content_clean[..120])
            } else {
                content_clean
            };
            summary.push_str(&format!("- **{}**: {}\n", msg.role, truncated));
        }
    }

    // 2. Keyword bullets (bullet points extracted from entire conversation)
    summary.push_str("\n**Key Topics & Keywords:**\n");
    let keywords = extract_keywords(messages);
    if keywords.is_empty() {
        summary.push_str("- None identified\n");
    } else {
        for kw in keywords {
            summary.push_str(&format!("- {}\n", kw));
        }
    }

    // Enforce token/char limit (saturating at 1600 chars (~400 tokens))
    if summary.len() > 1600 {
        summary.truncate(1597);
        summary.push_str("...");
    }

    summary
}

/// Simple stopword-filtered keyword extraction
fn extract_keywords(messages: &[Message]) -> Vec<String> {
    use std::collections::HashMap;
    use std::collections::HashSet;

    let stop_words: HashSet<&str> = [
        // English stopwords
        "the",
        "a",
        "an",
        "and",
        "or",
        "but",
        "if",
        "then",
        "else",
        "with",
        "from",
        "for",
        "to",
        "in",
        "on",
        "at",
        "by",
        "this",
        "that",
        "these",
        "those",
        "is",
        "are",
        "was",
        "were",
        "be",
        "been",
        "have",
        "has",
        "had",
        "do",
        "does",
        "did",
        "of",
        "not",
        "your",
        "mine",
        "about",
        "what",
        "where",
        "when",
        "how",
        "who",
        "which",
        "will",
        "would",
        "should",
        "could",
        "there",
        "their",
        "them",
        "they",
        "some",
        "any",
        "all",
        "more",
        "most",
        "than",
        "user",
        "assistant",
        "system",
        "please",
        "thanks",
        "thank",
        // Spanish stopwords
        "el",
        "la",
        "los",
        "las",
        "un",
        "una",
        "unos",
        "unas",
        "y",
        "o",
        "pero",
        "si",
        "entonces",
        "con",
        "de",
        "desde",
        "para",
        "por",
        "en",
        "sobre",
        "este",
        "esta",
        "estos",
        "estas",
        "ese",
        "esa",
        "esos",
        "esas",
        "es",
        "son",
        "era",
        "eran",
        "ser",
        "sido",
        "haber",
        "tiene",
        "tienen",
        "tenia",
        "hacer",
        "hace",
        "hacen",
        "no",
        "tu",
        "mio",
        "que",
        "donde",
        "cuando",
        "como",
        "quien",
        "cual",
        "sera",
        "seria",
        "deberia",
        "podria",
        "alli",
        "su",
        "sus",
        "ellos",
        "ellas",
        "algun",
        "algunos",
        "todo",
        "todos",
        "mas",
        "hola",
        "buenos",
        "dias",
        "tarde",
        "noches",
    ]
    .iter()
    .cloned()
    .collect();

    let mut counts = HashMap::new();
    for msg in messages {
        for word in msg.content.split_whitespace() {
            let cleaned: String = word
                .chars()
                .filter(|c| c.is_alphabetic())
                .collect::<String>()
                .to_lowercase();
            if cleaned.len() >= 4 && !stop_words.contains(cleaned.as_str()) {
                *counts.entry(cleaned).or_insert(0) += 1;
            }
        }
    }

    let mut sorted_counts: Vec<(String, usize)> = counts.into_iter().collect();
    sorted_counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    sorted_counts.into_iter().take(8).map(|(w, _)| w).collect()
}
