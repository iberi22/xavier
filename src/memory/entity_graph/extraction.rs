//! Entity extraction from memory content.
//!
//! Implements pattern-based and NLP-based entity extraction,
//! identifying named entities, relationships, and topics from
//! unstructured memory text for the entity graph.

use super::types::*;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

pub(super) static CANDIDATE_ENTITY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:[A-Z]{2,}(?:[A-Z0-9_-]*[A-Z0-9])?|[A-Z][a-z0-9]+(?:[A-Z][A-Za-z0-9_-]*)+(?:\s+[A-Z][A-Za-z0-9_-]*)*|[A-Z][a-z0-9]+(?:\s+[A-Z][a-z0-9]+)*|[A-Za-z]+[0-9]+[A-Za-z0-9_-]*)\b")
        .expect("valid entity regex")
});
pub(super) static EMAIL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\w.+-]+@[\w-]+\.[\w.-]+").expect("valid email regex"));
pub(super) static URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://[^\s]+").expect("valid URL regex"));

pub(super) static RELATION_PATTERNS: &[(&str, &str, f32)] = &[
    (
        r"(?i)\b(?P<source>[A-Z][\w-]*(?:\s+[A-Z][\w-]*)*)\s+works?\s+at\s+(?P<target>[A-Z][\w-]*(?:\s+[A-Z][\w-]*)*)",
        "works_at",
        0.95,
    ),
    (
        r"(?i)\b(?P<source>[A-Z][\w-]*(?:\s+[A-Z][\w-]*)*)\s+knows?\s+(?P<target>[A-Z][\w-]*(?:\s+[A-Z][\w-]*)*)",
        "knows",
        0.9,
    ),
    (
        r"(?i)\b(?P<source>[A-Z][\w-]*(?:\s+[A-Z][\w-]*)*)\s+uses?\s+(?P<target>[A-Z][\w-]*(?:\s+[A-Z][\w-]*)*)",
        "uses",
        0.85,
    ),
    (
        r"(?i)\b(?P<source>[A-Z][\w-]*(?:\s+[A-Z][\w-]*)*)\s+is\s+a[n]?\s+(?P<target>[A-Z][\w-]*(?:\s+[A-Z][\w-]*)*)",
        "is_a",
        0.8,
    ),
    (
        r"(?i)\b(?P<source>[A-Z][\w-]*(?:\s+[A-Z][\w-]*)*)\s+part\s+of\s+(?P<target>[A-Z][\w-]*(?:\s+[A-Z][\w-]*)*)",
        "part_of",
        0.9,
    ),
    (
        r"(?i)\b(?P<source>[A-Z][\w-]*(?:\s+[A-Z][\w-]*)*)\s+located\s+in\s+(?P<target>[A-Z][\w-]*(?:\s+[A-Z][\w-]*)*)",
        "located_in",
        0.9,
    ),
    (
        r"(?i)\b(?P<source>[A-Z][\w-]*(?:\s+[A-Z][\w-]*)*)\s+related\s+to\s+(?P<target>[A-Z][\w-]*(?:\s+[A-Z][\w-]*)*)",
        "related_to",
        0.7,
    ),
];

pub(super) static COMMON_WORDS: &[&str] = &[
    "the", "this", "that", "these", "those", "and", "or", "but", "for", "with", "from", "into",
    "onto", "your", "our", "their", "his", "her", "its", "in", "on", "at", "by", "to", "of",
];

pub(super) fn extract_entities(text: &str) -> Vec<ExtractedEntity> {
    let mut seen = HashSet::new();
    let mut entities = Vec::new();
    let explicit_relations = extract_relation_candidates(text);
    let mut subject_names = HashSet::new();
    let mut object_names = HashSet::new();
    for relation in &explicit_relations {
        subject_names.insert(normalize_name(&relation.source));
        object_names.insert(normalize_name(&relation.target));
    }

    for mat in EMAIL_RE.find_iter(text) {
        let name = mat.as_str().trim().to_string();
        let key = format!("{}|{:?}", normalize_name(&name), EntityType::Concept);
        if seen.insert(key) {
            entities.push(ExtractedEntity {
                name,
                entity_type: EntityType::Concept,
                span: (mat.start(), mat.end()),
            });
        }
    }

    for mat in URL_RE.find_iter(text) {
        let name = mat.as_str().trim().to_string();
        let key = format!("{}|{:?}", normalize_name(&name), EntityType::Product);
        if seen.insert(key) {
            entities.push(ExtractedEntity {
                name,
                entity_type: EntityType::Product,
                span: (mat.start(), mat.end()),
            });
        }
    }

    for mat in CANDIDATE_ENTITY_RE.find_iter(text) {
        let name = mat
            .as_str()
            .trim()
            .trim_matches(|c: char| {
                matches!(
                    c,
                    ',' | '.' | ';' | ':' | '"' | '\'' | '(' | ')' | '[' | ']'
                )
            })
            .to_string();
        if is_common_word(&name) {
            continue;
        }
        let normalized = normalize_name(&name);
        let entity_type = guess_entity_type(&name, &subject_names, &object_names);
        let key = format!("{}|{:?}", normalized, entity_type);
        if seen.insert(key) {
            entities.push(ExtractedEntity {
                name,
                entity_type,
                span: (mat.start(), mat.end()),
            });
        }
    }

    entities
}

pub(super) fn guess_entity_type(
    name: &str,
    subject_names: &HashSet<String>,
    object_names: &HashSet<String>,
) -> EntityType {
    let normalized = normalize_name(name);
    let lowered = normalized.to_ascii_lowercase();

    if subject_names.contains(&normalized) {
        return EntityType::Person;
    }
    if object_names.contains(&normalized) {
        if looks_like_location(&lowered) {
            return EntityType::Location;
        }
        if looks_like_organization(name) {
            return EntityType::Organization;
        }
    }
    if looks_like_location(&lowered) {
        return EntityType::Location;
    }
    if looks_like_organization(name) {
        return EntityType::Organization;
    }
    if looks_like_product(name) {
        return EntityType::Product;
    }
    if looks_like_person(name) {
        return EntityType::Person;
    }
    EntityType::Concept
}

pub(super) fn extract_relation_candidates(text: &str) -> Vec<RawRelation> {
    let entities = extract_entities_without_relations(text);
    let mut relations = Vec::new();
    for (pattern, relation_type, score) in RELATION_PATTERNS {
        let re = Regex::new(pattern).expect("valid relation pattern regex");
        for cap in re.captures_iter(text) {
            let Some(source) = cap.name("source").map(|m| m.as_str().trim()) else {
                continue;
            };
            let Some(target) = cap.name("target").map(|m| m.as_str().trim()) else {
                continue;
            };
            let source = best_match(source, &entities).unwrap_or_else(|| source.to_string());
            let target = best_match(target, &entities).unwrap_or_else(|| target.to_string());
            relations.push(RawRelation {
                source,
                target,
                relation_type: relation_type.to_string(),
                score: *score,
            });
        }
    }
    relations
}

pub(super) fn extract_entities_without_relations(text: &str) -> Vec<String> {
    CANDIDATE_ENTITY_RE
        .find_iter(text)
        .map(|mat| {
            mat.as_str()
                .trim()
                .trim_matches(|c: char| {
                    matches!(
                        c,
                        ',' | '.' | ';' | ':' | '"' | '\'' | '(' | ')' | '[' | ']'
                    )
                })
                .to_string()
        })
        .filter(|name| !is_common_word(name))
        .collect()
}

pub(super) fn best_match(candidate: &str, entities: &[String]) -> Option<String> {
    let normalized = normalize_name(candidate);
    entities
        .iter()
        .find(|entity| normalize_name(entity) == normalized)
        .cloned()
}

pub(super) fn co_occurrence_score(entity_count: usize) -> f32 {
    match entity_count {
        0 | 1 => 0.0,
        2 => 0.55,
        3 => 0.65,
        4 => 0.75,
        _ => 0.85,
    }
}

pub(super) fn normalize_name(name: &str) -> String {
    name.split_whitespace()
        .map(|part| {
            part.trim_matches(|c: char| {
                matches!(
                    c,
                    ',' | '.' | ';' | ':' | '"' | '\'' | '(' | ')' | '[' | ']'
                )
            })
        })
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn entity_lookup_key(normalized_name: &str, entity_type: EntityType) -> String {
    format!("{}|{}", normalized_name, entity_type.as_str())
}

pub(super) fn relation_lookup_key(source: &str, target: &str, relation_type: &str) -> String {
    format!("{}|{}|{}", source, target, relation_type)
}

pub(super) fn is_common_word(value: &str) -> bool {
    COMMON_WORDS
        .iter()
        .any(|word| word.eq_ignore_ascii_case(value))
}

pub(super) fn looks_like_organization(name: &str) -> bool {
    let lowered = name.to_ascii_lowercase();
    let org_markers = [
        " inc",
        " corp",
        " llc",
        " ltd",
        " company",
        " co",
        " labs",
        " lab",
        " systems",
        " studio",
        " platform",
        " foundation",
        " university",
        " institute",
        " agency",
        " team",
    ];
    name.chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-' || c == '_')
        || org_markers
            .iter()
            .any(|marker| lowered.ends_with(marker) || lowered.contains(marker))
}

pub(super) fn looks_like_location(lowered: &str) -> bool {
    let location_markers = [
        " city",
        " town",
        " village",
        " province",
        " state",
        " country",
        " park",
        " valley",
        " mountain",
        " river",
        " lake",
        " bay",
        " beach",
        " street",
        " avenue",
    ];
    location_markers
        .iter()
        .any(|marker| lowered.ends_with(marker) || lowered.contains(marker))
}

pub(super) fn looks_like_product(name: &str) -> bool {
    let lowered = name.to_ascii_lowercase();
    lowered.chars().any(|c| c.is_ascii_digit())
        || lowered.contains("model")
        || lowered.contains("platform")
        || lowered.contains("engine")
        || lowered.contains("sdk")
        || lowered.contains("api")
}

pub(super) fn looks_like_person(name: &str) -> bool {
    let tokens: Vec<_> = name.split_whitespace().collect();
    (tokens.len() <= 3
        && tokens.iter().all(|token| {
            token.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                || token.chars().all(|c| c.is_ascii_uppercase())
        }))
        || name.len() <= 8
}
