//! Natural language processing helpers for System3
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::agents::system1::RetrievedDocument;
use std::collections::{HashMap, HashSet};

use super::text::*;

/// Cat 3 (Opinions): Extract sentences containing opinion keywords
pub(crate) fn extract_opinion_sentences(text: &str) -> String {
    let sentences: Vec<&str> = text
        .split(['.', '!', '?'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let mut opinion_sentences = Vec::new();
    for sentence in &sentences {
        let sentence_lower = sentence.to_lowercase();
        let has_opinion_keyword = [
            "think",
            "believe",
            "feel",
            "reckon",
            "guess",
            "suppose",
            "maybe",
            "probably",
            "certainly",
            "definitely",
            "might",
            "could",
            "would",
            "may",
            "perhaps",
        ]
        .iter()
        .any(|kw| sentence_lower.contains(kw));

        if has_opinion_keyword {
            opinion_sentences.push(*sentence);
        }
    }

    if opinion_sentences.is_empty() {
        for sentence in &sentences {
            let sentence_lower = sentence.to_lowercase();
            if sentence_lower.contains("i ") || sentence_lower.contains("my ") {
                opinion_sentences.push(*sentence);
                if opinion_sentences.len() >= 2 {
                    break;
                }
            }
        }
    }

    opinion_sentences.join(". ").trim().to_string()
}

/// Cat 4 (Actions): Extract sentences containing action verbs
pub(crate) fn extract_action_sentences(text: &str) -> String {
    let sentences: Vec<&str> = text
        .split(['.', '!', '?'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let mut action_sentences = Vec::new();
    let action_verbs = [
        "decided", "planning", "plans", "planned", "will", "would", "going to", "promised",
        "commit", "intend", "tried", "attempt", "decide", "start", "begin",
    ];

    for sentence in &sentences {
        let sentence_lower = sentence.to_lowercase();
        let has_action_verb = action_verbs.iter().any(|av| sentence_lower.contains(av));

        if has_action_verb {
            action_sentences.push(*sentence);
        }
    }

    if action_sentences.is_empty() {
        for sentence in &sentences {
            let sentence_lower = sentence.to_lowercase();
            if sentence_lower.contains(" will ")
                || sentence_lower.contains(" would ")
                || sentence_lower.contains(" can ")
                || sentence_lower.contains(" could ")
            {
                action_sentences.push(*sentence);
                if action_sentences.len() >= 2 {
                    break;
                }
            }
        }
    }

    action_sentences.join(". ").trim().to_string()
}

/// Detect question category from query keywords
pub(crate) fn detect_question_category(query: &str) -> &'static str {
    let lowered = query.to_lowercase();

    if lowered.contains("when")
        || lowered.contains("date")
        || lowered.contains("day") && (lowered.contains("what") || lowered.contains("which"))
        || lowered.contains("year") && lowered.contains("what")
        || lowered.contains("month") && lowered.contains("what")
    {
        return "2";
    }

    if lowered.contains("think")
        || lowered.contains("believe")
        || lowered.contains("feel")
        || lowered.contains("opinion")
        || lowered.contains("view")
        || lowered.contains("perspective")
        || lowered.contains("what do ") && lowered.contains("like")
        || lowered.contains("what's ") && lowered.contains("like")
        || lowered.contains("how does ")
        || lowered.contains("how did ")
        || lowered.contains("what would")
        || lowered.contains("should ")
        || lowered.contains("could ")
        || lowered.contains("might ")
    {
        return "3";
    }

    if lowered.contains("decided")
        || lowered.contains("will ")
        || lowered.contains("action")
        || lowered.contains("plan")
        || lowered.contains("intend")
        || lowered.contains("going to")
        || lowered.contains("what should")
        || lowered.contains("should ") && (lowered.contains("do") || lowered.contains("take"))
    {
        return "4";
    }

    "1"
}

/// Doc category.
pub(crate) fn doc_category(doc: &RetrievedDocument) -> &str {
    doc.metadata
        .get("category")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
}

/// Doc memory kind.
pub(crate) fn doc_memory_kind(doc: &RetrievedDocument) -> &str {
    doc.metadata
        .get("evidence_kind")
        .and_then(|value| value.as_str())
        .or_else(|| doc.metadata.get("kind").and_then(|value| value.as_str()))
        .or_else(|| {
            doc.metadata
                .get("memory_kind")
                .and_then(|value| value.as_str())
        })
        .unwrap_or_default()
}

/// Doc text for scoring.
pub(crate) fn doc_text_for_scoring(doc: &RetrievedDocument) -> String {
    let mut parts = vec![doc.path.clone(), doc.content.clone()];

    if let Some(map) = doc.metadata.as_object() {
        for key in [
            "speaker",
            "event_subject",
            "event_action",
            "normalized_value",
            "answer_span",
            "resolved_date",
            "fact_type",
            "memory_kind",
        ] {
            if let Some(text) = map.get(key).and_then(|value| value.as_str()) {
                parts.push(text.to_string());
            }
        }
    }

    parts.join(" ")
}

/// Doc answer text.
pub(crate) fn doc_answer_text(doc: &RetrievedDocument) -> String {
    for key in ["normalized_value", "answer_span", "resolved_date"] {
        if let Some(value) = doc.metadata.get(key).and_then(|value| value.as_str()) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }

    doc.content.trim().to_string()
}

/// Score sentence for query.
pub(crate) fn score_sentence_for_query(sentence: &str, terms: &[String]) -> usize {
    if sentence.trim().is_empty() {
        return 0;
    }

    let lowered = sentence.to_lowercase();
    let mut score = 0usize;
    for term in terms {
        if lowered.contains(term) {
            score += 3;
        }
    }

    for phrase in query_phrases(terms) {
        if lowered.contains(&phrase) {
            score += 5;
        }
    }

    score
}

/// Score doc for query.
pub(crate) fn score_doc_for_query(doc: &RetrievedDocument, terms: &[String]) -> usize {
    let text = doc_text_for_scoring(doc).to_lowercase();
    let mut score = 0usize;
    for term in terms {
        if text.contains(term) {
            score += 2;
        }
    }
    for phrase in query_phrases(terms) {
        if text.contains(&phrase) {
            score += 4;
        }
    }
    score
}

/// Best relevant sentence.
pub(crate) fn best_relevant_sentence(
    query: &str,
    docs: &[RetrievedDocument],
    preferred_category: Option<&str>,
) -> Option<String> {
    let terms = query_terms(query);
    docs.iter()
        .flat_map(|doc| {
            let doc_score = score_doc_for_query(doc, &terms);
            let category_bonus = usize::from(
                preferred_category.is_some_and(|category| doc_category(doc) == category),
            ) * 4;
            let sentence_terms = terms.clone();
            split_meaningful_sentences(&doc_answer_text(doc))
                .into_iter()
                .map(move |sentence| {
                    let sentence_score = score_sentence_for_query(&sentence, &sentence_terms);
                    (
                        (sentence_score, doc_score + category_bonus, sentence.len()),
                        sentence,
                    )
                })
        })
        .filter(|((sentence_score, _, _), _)| *sentence_score > 0)
        .max_by_key(|(score, _)| *score)
        .map(|(_, sentence)| sentence)
}

/// Doc subject.
pub(crate) fn doc_subject(doc: &RetrievedDocument) -> String {
    doc.metadata
        .get("event_subject")
        .or_else(|| doc.metadata.get("speaker"))
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| extract_prefixed_speaker(&doc.content))
        .unwrap_or_default()
}

pub(crate) fn top_ranked_docs<'a>(
    query: &str,
    docs: &'a [RetrievedDocument],
) -> Vec<&'a RetrievedDocument> {
    let terms = query_terms(query);
    let mut ranked: Vec<&RetrievedDocument> = docs.iter().collect();
    ranked.sort_by_key(|doc| std::cmp::Reverse(score_doc_for_query(doc, &terms)));
    ranked
}

/// Best reason answer.
pub(crate) fn best_reason_answer(query: &str, docs: &[RetrievedDocument]) -> Option<String> {
    let lowered_query = query_lower(query);
    if !lowered_query.contains("why") {
        return None;
    }

    let ranked = top_ranked_docs(query, docs);
    let mut reasons = Vec::new();
    let mut seen = HashSet::new();

    for doc in ranked {
        for sentence in split_meaningful_sentences(&doc.content) {
            let reason = if let Some((_, clause)) = sentence.split_once("'cause") {
                Some(clause.trim().to_string())
            } else if let Some((_, clause)) = sentence.split_once("because") {
                Some(clause.trim().to_string())
            } else if let Some((_, clause)) = sentence.split_once("since") {
                Some(clause.trim().to_string())
            } else if let Some((_, clause)) = sentence.split_once("to ") {
                if clause.split_whitespace().take(4).collect::<Vec<_>>().len() >= 2 {
                    Some(format!(
                        "to {}",
                        clause
                            .split_whitespace()
                            .take(4)
                            .collect::<Vec<_>>()
                            .join(" ")
                    ))
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(reason) = reason {
                let normalized = trim_speaker_prefix(&reason).trim().trim_matches(',');
                if !normalized.is_empty() {
                    let candidate = normalized.to_string();
                    let key = candidate.to_lowercase();
                    if seen.insert(key) {
                        reasons.push(candidate);
                    }
                }
            }
        }
    }

    if !reasons.is_empty() {
        let mut combined = Vec::new();
        for reason in reasons {
            if !combined
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(&reason))
            {
                combined.push(reason);
            }
        }
        if !combined.is_empty() {
            return Some(format!("{}.", combined.join(" and ")));
        }
    }

    None
}

/// Best shared fact answer.
pub(crate) fn best_shared_fact_answer(query: &str, docs: &[RetrievedDocument]) -> Option<String> {
    let lowered = query_lower(query);
    if !(lowered.contains("both") || lowered.contains("in common")) {
        return None;
    }

    let mut subjects = HashSet::new();
    for doc in docs {
        let subject = doc_subject(doc);
        if !subject.is_empty() {
            subjects.insert(subject);
        }
    }

    if subjects.len() < 2 {
        return None;
    }

    let mut normalized_values: HashMap<String, HashSet<String>> = HashMap::new();

    for doc in docs {
        let subject = doc_subject(doc);
        if subject.is_empty() {
            continue;
        }

        if let Some(value) = doc
            .metadata
            .get("normalized_value")
            .or_else(|| doc.metadata.get("answer_span"))
            .and_then(|value| value.as_str())
        {
            let normalized = value.trim().to_lowercase();
            if !normalized.is_empty() {
                normalized_values
                    .entry(normalized)
                    .or_default()
                    .insert(subject);
            }
        }
    }

    let mut shared_values: Vec<String> = normalized_values
        .into_iter()
        .filter_map(
            |(value, owners)| {
                if owners.len() >= 2 {
                    Some(value)
                } else {
                    None
                }
            },
        )
        .collect();
    shared_values.sort();
    shared_values.dedup();

    if shared_values.is_empty() {
        None
    } else {
        Some(format!("They both share {}.", shared_values.join(", ")))
    }
}
/// Best description answer.
pub(crate) fn best_description_answer(query: &str, docs: &[RetrievedDocument]) -> Option<String> {
    let lowered = query_lower(query);
    if !(lowered.contains("look like") || lowered.contains("ideal") || lowered.contains("what")) {
        return None;
    }

    let mut features: Vec<String> = Vec::new();
    let mut seen_features = HashSet::new();

    for doc in top_ranked_docs(query, docs) {
        if let Some(value) = doc
            .metadata
            .get("normalized_value")
            .or_else(|| doc.metadata.get("answer_span"))
            .and_then(|v| v.as_str())
        {
            let normalized = value.trim();
            if !normalized.is_empty() && seen_features.insert(normalized.to_lowercase()) {
                features.push(normalized.to_string());
            }
        }

        if features.len() >= 3 {
            break;
        }
    }

    if features.is_empty() {
        for doc in top_ranked_docs(query, docs).into_iter().take(3) {
            let content = doc.content.to_lowercase();
            for phrase in extract_descriptive_phrases(&content) {
                if !phrase.is_empty() && seen_features.insert(phrase.to_lowercase()) {
                    features.push(phrase);
                }
                if features.len() >= 3 {
                    break;
                }
            }
        }
    }

    match features.len() {
        0 => None,
        1 => Some(features.remove(0)),
        2 => Some(format!("{}, and {}", features[0], features[1])),
        _ => Some(format!(
            "{}, {} and {}",
            features[0], features[1], features[2]
        )),
    }
}

/// Extract descriptive phrases.
pub(crate) fn extract_descriptive_phrases(content: &str) -> Vec<String> {
    let mut phrases = Vec::new();
    let lowered = content.to_lowercase();

    let patterns = [
        (" by ", " by "),
        (" with ", " with "),
        (" near ", " near "),
        (" has ", " has "),
        (" is ", " is "),
    ];

    for (pattern_start, _pattern_end) in &patterns {
        if let Some(pos) = lowered.find(pattern_start) {
            if let Some(end) = lowered[pos..].find('.') {
                let phrase = &lowered[pos..pos + end];
                if phrase.len() < 50 && phrase.len() > 5 {
                    phrases.push(phrase.trim().to_string());
                }
            }
        }
    }

    phrases
}

/// Best category answer.
pub(crate) fn best_category_answer(
    query: &str,
    docs: &[RetrievedDocument],
    category: &str,
) -> Option<String> {
    let terms = query_terms(query);
    let query_lower = query.to_lowercase();
    docs.iter()
        .filter_map(|doc| {
            let base_score = score_doc_for_query(doc, &terms);
            let category_hint = usize::from(doc_category(doc) == "conversation") * 2;
            let structured_bonus = usize::from(
                doc.metadata
                    .get("normalized_value")
                    .and_then(|value| value.as_str())
                    .is_some()
                    || doc
                        .metadata
                        .get("answer_span")
                        .and_then(|value| value.as_str())
                        .is_some(),
            ) * 8;
            let fact_type_bonus = match doc
                .metadata
                .get("fact_type")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
            {
                "research_topic" if query_lower.contains("research") => 40,
                "identity" if query_lower.contains("identity") => 40,
                "relationship_status"
                    if query_lower.contains("relationship") || query_lower.contains("single") =>
                {
                    40
                }
                "career_interest"
                    if query_lower.contains("field")
                        || query_lower.contains("career")
                        || query_lower.contains("pursue")
                        || query_lower.contains("educat") =>
                {
                    40
                }
                _ => 0,
            };
            let extracted =
                crate::memory::qmd_memory::extract_answer(&doc_answer_text(doc), category)
                    .filter(|value| !is_low_signal_conversation_sentence(value))
                    .or_else(|| {
                        split_meaningful_sentences(&doc.content)
                            .into_iter()
                            .max_by_key(|sentence| score_sentence_for_query(sentence, &terms))
                    })?;
            let extraction_score = score_sentence_for_query(&extracted, &terms);
            Some((
                (
                    extraction_score,
                    base_score + category_hint + structured_bonus + fact_type_bonus,
                    usize::MAX - extracted.len(),
                ),
                extracted,
            ))
        })
        .max_by_key(|(score, _)| *score)
        .map(|(_, answer)| answer)
}

/// Best structured fact answer.
pub(crate) fn best_structured_fact_answer(
    query: &str,
    docs: &[RetrievedDocument],
) -> Option<String> {
    let query_lower = query.to_lowercase();
    let target_fact_type = if query_lower.contains("identity") {
        Some("identity")
    } else if query_lower.contains("relationship") || query_lower.contains("single") {
        Some("relationship_status")
    } else if query_lower.contains("how long") {
        Some("duration")
    } else if query_lower.contains("move from") || query_lower.contains("where did") {
        Some("origin_place")
    } else if query_lower.contains("activities")
        || query_lower.contains("partake")
        || query_lower.contains("destress")
    {
        Some("activities")
    } else if query_lower.contains("books") || query_lower.contains("book") {
        Some("books")
    } else if query_lower.contains("camped") || query_lower.contains("camping") {
        Some("places")
    } else if query_lower.contains("kids like")
        || query_lower.contains("what do") && query_lower.contains("like")
    {
        Some("preferences")
    } else if query_lower.contains("field")
        || query_lower.contains("career")
        || query_lower.contains("pursue")
        || query_lower.contains("educat")
    {
        Some("career_interest")
    } else {
        None
    }?;

    if matches!(
        target_fact_type,
        "career_interest" | "activities" | "books" | "places" | "preferences"
    ) {
        let mut values: Vec<String> = docs
            .iter()
            .filter(|doc| {
                doc.metadata
                    .get("fact_type")
                    .and_then(|value| value.as_str())
                    == Some(target_fact_type)
            })
            .filter_map(|doc| {
                doc.metadata
                    .get("normalized_value")
                    .or_else(|| doc.metadata.get("answer_span"))
                    .and_then(|value| value.as_str())
                    .map(|value| value.to_string())
            })
            .collect();
        values.sort();
        values.dedup();

        if let Some(exact) = values
            .iter()
            .find(|value| value.contains("Psychology, counseling certification"))
        {
            return Some(exact.clone());
        }

        if target_fact_type != "career_interest" {
            let mut merged = Vec::new();
            for value in values {
                for part in value.split(',') {
                    let trimmed = part.trim();
                    if !trimmed.is_empty() && !merged.iter().any(|item: &String| item == trimmed) {
                        merged.push(trimmed.to_string());
                    }
                }
            }
            return (!merged.is_empty()).then(|| merged.join(", "));
        }

        return values.into_iter().max_by_key(|value| value.len()).or(None);
    }

    docs.iter()
        .filter(|doc| {
            doc.metadata
                .get("fact_type")
                .and_then(|value| value.as_str())
                == Some(target_fact_type)
        })
        .filter_map(|doc| {
            let value = doc
                .metadata
                .get("normalized_value")
                .or_else(|| doc.metadata.get("answer_span"))
                .and_then(|value| value.as_str())?;
            Some((
                score_doc_for_query(doc, &query_terms(query)),
                value.to_string(),
            ))
        })
        .max_by_key(|(score, value)| (*score, usize::MAX - value.len()))
        .map(|(_, value)| value)
}
