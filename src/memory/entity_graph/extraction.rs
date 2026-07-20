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
        r"(?i)\b(?P<source>[A-Z][\w-]*(?:\s+[A-Z][\w-]*){0,3})\s+works?\s+at\s+(?P<target>[A-Z][\w-]*(?:\s+[A-Z][\w-]*){0,3})",
        "works_at",
        0.95,
    ),
    (
        r"(?i)\b(?P<source>[A-Z][\w-]*(?:\s+[A-Z][\w-]*){0,3})\s+knows?\s+(?P<target>[A-Z][\w-]*(?:\s+[A-Z][\w-]*){0,3})",
        "knows",
        0.9,
    ),
    (
        r"(?i)\b(?P<source>[A-Z][\w-]*(?:\s+[A-Z][\w-]*){0,3})\s+uses?\s+(?P<target>[A-Z][\w-]*(?:\s+[A-Z][\w-]*){0,3})",
        "uses",
        0.85,
    ),
    (
        r"(?i)\b(?P<source>[A-Z][\w-]*(?:\s+[A-Z][\w-]*){0,3})\s+is\s+a[n]?\s+(?P<target>[A-Z][\w-]*(?:\s+[A-Z][\w-]*){0,3})",
        "is_a",
        0.8,
    ),
    (
        r"(?i)\b(?P<source>[A-Z][\w-]*(?:\s+[A-Z][\w-]*){0,3})\s+part\s+of\s+(?P<target>[A-Z][\w-]*(?:\s+[A-Z][\w-]*){0,3})",
        "part_of",
        0.9,
    ),
    (
        r"(?i)\b(?P<source>[A-Z][\w-]*(?:\s+[A-Z][\w-]*){0,3})\s+located\s+in\s+(?P<target>[A-Z][\w-]*(?:\s+[A-Z][\w-]*){0,3})",
        "located_in",
        0.9,
    ),
    (
        r"(?i)\b(?P<source>[A-Z][\w-]*(?:\s+[A-Z][\w-]*){0,3})\s+related\s+to\s+(?P<target>[A-Z][\w-]*(?:\s+[A-Z][\w-]*){0,3})",
        "related_to",
        0.7,
    ),
];

pub(super) struct CompiledRelationPattern {
    pub regex: Regex,
    pub relation_type: String,
    pub score: f32,
}

pub(super) static COMPILED_RELATION_PATTERNS: LazyLock<Vec<CompiledRelationPattern>> = LazyLock::new(|| {
    RELATION_PATTERNS
        .iter()
        .map(|(pattern, relation_type, score)| CompiledRelationPattern {
            regex: Regex::new(pattern).expect("valid relation pattern regex"),
            relation_type: relation_type.to_string(),
            score: *score,
        })
        .collect()
});

pub(super) static COMMON_WORDS: &[&str] = &[
    "the", "this", "that", "these", "those", "and", "or", "but", "for", "with", "from", "into",
    "onto", "your", "our", "their", "his", "her", "its", "in", "on", "at", "by", "to", "of",
];

fn chunk_text_by_lines(text: &str) -> Vec<(&str, usize)> {
    let mut chunks = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            chunks.push((line, offset));
        }
        offset += line.len();
    }
    chunks
}

pub(super) fn extract_entities(text: &str) -> Vec<ExtractedEntity> {
    let mut entities = Vec::new();
    if text.len() > 500 {
        for (chunk, offset) in chunk_text_by_lines(text) {
            let mut chunk_entities = extract_entities_chunk(chunk);
            for ent in &mut chunk_entities {
                ent.span.0 += offset;
                ent.span.1 += offset;
            }
            entities.extend(chunk_entities);
        }
    } else {
        entities.extend(extract_entities_chunk(text));
    }

    let mut seen = HashSet::new();
    entities.retain(|ent| {
        let key = format!("{}|{:?}", normalize_name(&ent.name), ent.entity_type);
        seen.insert(key)
    });

    entities
}

fn extract_entities_chunk(text: &str) -> Vec<ExtractedEntity> {
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
    let mut relations = Vec::new();
    if text.len() > 500 {
        for (chunk, _) in chunk_text_by_lines(text) {
            relations.extend(extract_relation_candidates_chunk(chunk));
        }
    } else {
        relations.extend(extract_relation_candidates_chunk(text));
    }
    relations
}

fn extract_relation_candidates_chunk(text: &str) -> Vec<RawRelation> {
    let entities = extract_entities_without_relations(text);
    let mut relations = Vec::new();
    for item in COMPILED_RELATION_PATTERNS.iter() {
        for cap in item.regex.captures_iter(text) {
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
                relation_type: item.relation_type.clone(),
                score: item.score,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extraction_extremely_long_text() {
        // Generate a 150KB long string composed of repeating natural and realistic prose.
        let mut large_text = String::new();
        for _ in 0..600 {
            large_text.push_str("Xavier is an advanced cognitive memory manager and AI agent orchestrator. It helps developers organize information.\n");
            large_text.push_str("SWAL is a leading research institute located in Bogota. Leonardo works at SWAL as a principal scientist.\n");
            large_text.push_str("Alice works at Acme. Alice knows Leonardo.\n");
        }

        assert!(large_text.len() > 150_000, "Text size should be greater than 150KB");

        let start = std::time::Instant::now();
        let entities = extract_entities(&large_text);
        let duration_entities = start.elapsed();

        let start_relations = std::time::Instant::now();
        let relations = extract_relation_candidates(&large_text);
        let duration_relations = start_relations.elapsed();

        println!("Text length: {}, Entities count: {}, took {}ms", large_text.len(), entities.len(), duration_entities.as_millis());
        println!("Relations count: {}, took {}ms", relations.len(), duration_relations.as_millis());

        // Ensure parser is resilient and handles text efficiently (e.g., under 1 second for a huge 150KB doc)
        assert!(duration_entities.as_millis() < 1000, "Entity extraction took too long: {}ms", duration_entities.as_millis());
        assert!(duration_relations.as_millis() < 1000, "Relation extraction took too long: {}ms", duration_relations.as_millis());

        // Ensure we did find several matches and did not crash or stack overflow
        assert!(!entities.is_empty(), "Should extract entities successfully");
        assert!(!relations.is_empty(), "Should extract relations successfully");

        // Validate a few extracted samples
        let has_swal = entities.iter().any(|e| e.name.contains("SWAL"));
        let has_bogota = entities.iter().any(|e| e.name.contains("Bogota"));
        assert!(has_swal && has_bogota, "Expected core entities SWAL and Bogota to be extracted");
    }
}
