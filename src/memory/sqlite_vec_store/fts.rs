// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Full-text search for SQLite vector store
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use regex::Regex;
use std::collections::HashSet;
use std::sync::OnceLock;

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
