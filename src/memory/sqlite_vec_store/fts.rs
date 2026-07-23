//! Full-text search for SQLite vector store
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use regex::Regex;
use std::collections::HashSet;
use std::sync::OnceLock;

/// Match mode for FTS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FtsMatchMode {
    All,
    Any,
}

/// Helper trait to handle single and tuple arguments for build_fts_query.
pub trait IntoFtsQueryArgs {
    fn into_args(self) -> (String, FtsMatchMode);
}

impl IntoFtsQueryArgs for &str {
    fn into_args(self) -> (String, FtsMatchMode) {
        let q = self.to_string();
        let mode = if q.contains(" OR ") {
            FtsMatchMode::Any
        } else {
            FtsMatchMode::All
        };
        (q, mode)
    }
}

impl IntoFtsQueryArgs for &String {
    fn into_args(self) -> (String, FtsMatchMode) {
        let q = self.to_string();
        let mode = if q.contains(" OR ") {
            FtsMatchMode::Any
        } else {
            FtsMatchMode::All
        };
        (q, mode)
    }
}

impl IntoFtsQueryArgs for String {
    fn into_args(self) -> (String, FtsMatchMode) {
        let q = self;
        let mode = if q.contains(" OR ") {
            FtsMatchMode::Any
        } else {
            FtsMatchMode::All
        };
        (q, mode)
    }
}

impl IntoFtsQueryArgs for (&str, FtsMatchMode) {
    fn into_args(self) -> (String, FtsMatchMode) {
        (self.0.to_string(), self.1)
    }
}

impl IntoFtsQueryArgs for (&String, FtsMatchMode) {
    fn into_args(self) -> (String, FtsMatchMode) {
        (self.0.to_string(), self.1)
    }
}

impl IntoFtsQueryArgs for (String, FtsMatchMode) {
    fn into_args(self) -> (String, FtsMatchMode) {
        (self.0, self.1)
    }
}

/// Escapes a single word/term for SQLite FTS5.
fn sanitize_word(word: &str) -> String {
    if word.is_empty() {
        return String::new();
    }

    // Check for prefix wildcard at the end
    let (base, is_prefix) = if word.ends_with('*') && word.len() > 1 {
        (&word[..word.len() - 1], true)
    } else {
        (word, false)
    };

    // Double any double quotes inside the term
    let escaped = base.replace('"', "\"\"");

    // Wrap the base term in double quotes
    let mut quoted = format!("\"{}\"", escaped);

    if is_prefix {
        quoted.push('*');
    }

    quoted
}

/// Helper function to sanitize a sub-query.
fn sanitize_subquery(query: &str, mode: FtsMatchMode) -> String {
    let mut words = Vec::new();
    for word in query.split_whitespace() {
        if word.is_empty() {
            continue;
        }

        // Check if the word itself is an operator keyword (AND, OR, NOT, NEAR).
        // If it is, we escape/quote it to prevent FTS5 syntax errors from misuse.
        let is_op = matches!(word, "AND" | "OR" | "NOT") || word.starts_with("NEAR");

        let escaped = if is_op {
            format!("\"{}\"", word.replace('"', "\"\""))
        } else {
            sanitize_word(word)
        };

        if !escaped.is_empty() {
            words.push(escaped);
        }
    }

    if words.is_empty() {
        return String::new();
    }

    let joiner = match mode {
        FtsMatchMode::All => " AND ",
        FtsMatchMode::Any => " OR ",
    };
    words.join(joiner)
}

/// Safely sanitizes an FTS query string to prevent SQLite syntax errors while preserving intent.
pub fn sanitize_fts(query: &str, mode: FtsMatchMode) -> String {
    if query.trim().is_empty() {
        return String::new();
    }

    // Parse with explicit operators (e.g. " OR ", " AND ") if present.
    // This allows the query to preserve user-provided boolean logic safely.
    let result = if query.contains(" OR ") {
        let parts: Vec<String> = query
            .split(" OR ")
            .map(|part| sanitize_subquery(part, mode))
            .filter(|s| !s.is_empty())
            .collect();
        parts.join(" OR ")
    } else if query.contains(" AND ") {
        let parts: Vec<String> = query
            .split(" AND ")
            .map(|part| sanitize_subquery(part, mode))
            .filter(|s| !s.is_empty())
            .collect();
        parts.join(" AND ")
    } else {
        sanitize_subquery(query, mode)
    };

    if result.is_empty() && !query.trim().is_empty() {
        return safe_fallback(query, mode);
    }

    result
}

/// Absolute fallback that extracts only completely safe characters.
fn safe_fallback(query: &str, mode: FtsMatchMode) -> String {
    let mut words = Vec::new();
    for word in query.split_whitespace() {
        let clean: String = word
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/'))
            .collect();
        if !clean.is_empty() {
            words.push(format!("\"{}\"", clean));
        }
    }
    let joiner = match mode {
        FtsMatchMode::All => " AND ",
        FtsMatchMode::Any => " OR ",
    };
    words.join(joiner)
}

/// Search tokens.
pub fn search_tokens(query: &str) -> Vec<String> {
    static TOKEN_RE: OnceLock<Regex> = OnceLock::new();
    let re = TOKEN_RE.get_or_init(|| {
        Regex::new(r"[A-Za-z0-9][A-Za-z0-9._:/#-]{1,}").expect("valid search token regex")
    });

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
pub fn build_fts_query<T: IntoFtsQueryArgs>(args: T) -> Option<String> {
    let (query, mode) = args.into_args();
    let sanitized = sanitize_fts(&query, mode);
    if sanitized.is_empty() {
        None
    } else {
        Some(sanitized)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enum_exists() {
        let mode = FtsMatchMode::All;
        assert_eq!(mode, FtsMatchMode::All);
    }

    #[test]
    fn test_sanitize_fts_basic() {
        let q = "rust python";
        assert_eq!(sanitize_fts(q, FtsMatchMode::All), "\"rust\" AND \"python\"");
        assert_eq!(sanitize_fts(q, FtsMatchMode::Any), "\"rust\" OR \"python\"");
    }

    #[test]
    fn test_sanitize_fts_explicit_or() {
        let q = "rust OR python";
        assert_eq!(sanitize_fts(q, FtsMatchMode::All), "\"rust\" OR \"python\"");
        assert_eq!(sanitize_fts(q, FtsMatchMode::Any), "\"rust\" OR \"python\"");
    }

    #[test]
    fn test_sanitize_fts_explicit_and() {
        let q = "rust AND python";
        assert_eq!(sanitize_fts(q, FtsMatchMode::All), "\"rust\" AND \"python\"");
        assert_eq!(sanitize_fts(q, FtsMatchMode::Any), "\"rust\" AND \"python\"");
    }

    #[test]
    fn test_sanitize_fts_special_chars() {
        let q = "error: expected fn";
        let sanitized = sanitize_fts(q, FtsMatchMode::All);
        assert_eq!(sanitized, "\"error:\" AND \"expected\" AND \"fn\"");

        let q2 = "^ * () + - ~ : ->";
        let sanitized2 = sanitize_fts(q2, FtsMatchMode::All);
        assert_eq!(
            sanitized2,
            "\"^\" AND \"*\" AND \"()\" AND \"+\" AND \"-\" AND \"~\" AND \":\" AND \"->\""
        );
    }

    #[test]
    fn test_sanitize_fts_prefix() {
        let q = "rust* python";
        assert_eq!(sanitize_fts(q, FtsMatchMode::All), "\"rust\"* AND \"python\"");
    }

    #[test]
    fn test_build_fts_query_overloading() {
        let q = "rust python";
        assert_eq!(build_fts_query(q), Some("\"rust\" AND \"python\"".to_string()));
        assert_eq!(
            build_fts_query((q, FtsMatchMode::Any)),
            Some("\"rust\" OR \"python\"".to_string())
        );

        let explicit_or_q = "rust OR python";
        assert_eq!(
            build_fts_query(explicit_or_q),
            Some("\"rust\" OR \"python\"".to_string())
        );
    }

    #[test]
    fn test_double_quotes_escaping() {
        let q = "some\"word";
        assert_eq!(sanitize_fts(q, FtsMatchMode::All), "\"some\"\"word\"");
    }
}
