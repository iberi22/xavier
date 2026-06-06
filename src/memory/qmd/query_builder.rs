//! QMD query builder
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::memory::qmd_memory::config::{MAX_EXPANSIONS, SYNONYM_MAP};
use crate::memory::qmd_memory::types::QueryBundle;
use std::collections::HashMap;

/// Normalize a query by lowercasing, removing stop words, and cleaning tokens.
pub fn normalize_query(query_text: &str) -> String {
    query_text
        .split_whitespace()
        .map(normalize_token)
        .filter(|token| {
            !token.is_empty()
                && !matches!(
                    token.as_str(),
                    "when"
                        | "what"
                        | "where"
                        | "which"
                        | "who"
                        | "how"
                        | "why"
                        | "did"
                        | "does"
                        | "was"
                        | "were"
                        | "the"
                        | "and"
                        | "for"
                        | "with"
                        | "about"
                        | "into"
                        | "from"
                        | "that"
                        | "this"
                        | "your"
                        | "have"
                        | "had"
                )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Clean a single token to lowercase alphanumeric.
pub fn normalize_token(token: &str) -> String {
    token
        .chars()
        .filter(|char| char.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

/// Build a QueryBundle with expanded variants and weights.
pub fn build_query_bundle_internal(query_text: &str) -> QueryBundle {
    let normalized_query = normalize_query(query_text);
    let mut variants = vec![normalized_query.clone()];
    let mut weights = HashMap::from([(normalized_query.clone(), 1.0)]);

    let tokens = normalized_query
        .split_whitespace()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();

    for token in tokens.into_iter().take(MAX_EXPANSIONS) {
        if let Some(synonyms) = SYNONYM_MAP.get(token.as_str()) {
            for synonym in synonyms.iter().take(2) {
                let expanded = if normalized_query.is_empty() {
                    (*synonym).to_string()
                } else {
                    format!("{normalized_query} {synonym}")
                };
                if weights.contains_key(&expanded) {
                    continue;
                }
                variants.push(expanded.clone());
                weights.insert(expanded, 0.85);
            }
        }
    }

    if variants.len() == 1 {
        for token in query_text.split_whitespace().take(MAX_EXPANSIONS) {
            let cleaned = normalize_token(token);
            if cleaned.len() < 3 || cleaned == normalized_query {
                continue;
            }
            if let std::collections::hash_map::Entry::Vacant(entry) =
                weights.entry(format!("{normalized_query} {cleaned}"))
            {
                let expanded = entry.key().clone();
                variants.push(expanded.clone());
                entry.insert(0.8);
            }
        }
    }

    variants.truncate(5);

    QueryBundle {
        normalized_query,
        variants,
        weights,
    }
}

/// Extract candidate terms from text for multi-hop expansion.
pub fn extract_candidate_terms_internal(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(normalize_token)
        .filter(|token| token.len() >= 4)
        .filter(|token| {
            !matches!(
                token.as_str(),
                "with"
                    | "that"
                    | "this"
                    | "from"
                    | "have"
                    | "were"
                    | "when"
                    | "what"
                    | "where"
                    | "which"
                    | "would"
                    | "could"
            )
        })
        .collect()
}
