//! Date and time helpers for System3
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use chrono::{Datelike, Duration, NaiveDate};
use regex::Regex;
use std::sync::OnceLock;

use super::nlp::*;
use super::text::*;
use crate::agents::system1::RetrievedDocument;

/// Clean date.
pub(crate) fn clean_date(text: &str) -> String {
    let trimmed = text.trim();

    if let Some((_, after_on)) = trimmed.rsplit_once(" on ") {
        let year = trimmed
            .split(',')
            .nth(1)
            .map(str::trim)
            .filter(|part| !part.is_empty());

        return match year {
            Some(year) if !after_on.contains(year) => format!("{} {}", after_on.trim(), year),
            _ => after_on.trim().to_string(),
        };
    }

    if let Some((before_comma, after_comma)) = trimmed.split_once(',') {
        let before = before_comma.trim();
        let after = after_comma.trim();
        if before.chars().any(|ch| ch.is_ascii_digit())
            && after.contains(':')
            && after
                .chars()
                .all(|ch| ch.is_ascii_digit() || ch == ':' || ch.is_whitespace())
        {
            return before.to_string();
        }
        if before.chars().all(|ch| ch.is_ascii_digit())
            || before.chars().all(|ch| ch.is_alphabetic())
        {
            return format!("{before}, {after}");
        }
    }

    trimmed.to_string()
}

/// Date patterns.
pub(crate) fn date_patterns() -> &'static [Regex] {
    static DATE_PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    DATE_PATTERNS
        .get_or_init(|| {
            vec![
                Regex::new(r"(?i)\b\d{1,2}\s+[A-Za-z]+\s+\d{4}\b").expect("day month year regex"),
                Regex::new(r"(?i)\b[A-Za-z]+\s+\d{1,2},\s+\d{4}\b").expect("month day year regex"),
                Regex::new(r"\b(19|20)\d{2}\b").expect("year regex"),
            ]
        })
        .as_slice()
}

/// Extract date answer.
pub(crate) fn extract_date_answer(text: &str) -> Option<String> {
    static EXPANDED_DATE_PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    let expanded_patterns = EXPANDED_DATE_PATTERNS
        .get_or_init(|| {
            vec![
                Regex::new(r"(?i)\b\d{1,2}\s+[A-Za-z]+\s+\d{4}\b")
                    .expect("invalid regex: day month year"),
                Regex::new(r"(?i)\b[A-Za-z]+\s+\d{1,2},\s+\d{4}\b")
                    .expect("invalid regex: month day year"),
                Regex::new(r"\b\d{4}-\d{2}-\d{2}\b").expect("invalid regex: ISO date"),
                Regex::new(r"(?i)\b[A-Za-z]+\s+\d{4}\b").expect("invalid regex: month year"),
                Regex::new(r"(?i)\b(yesterday|last\s+(week|month|year))\b")
                    .expect("invalid regex: relative date"),
                Regex::new(r"\b(19|20)\d{2}\b").expect("invalid regex: year"),
            ]
        })
        .as_slice();

    for pattern in date_patterns() {
        if let Some(found) = pattern.find(text) {
            return Some(clean_date(found.as_str()));
        }
    }

    for pattern in expanded_patterns {
        if let Some(found) = pattern.find(text) {
            let date_str = found.as_str();
            let cleaned = clean_date(date_str);
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
        }
    }
    None
}

/// Date granularity rank.
pub(crate) fn date_granularity_rank(text: &str) -> usize {
    static DAY_MONTH_YEAR: OnceLock<Regex> = OnceLock::new();
    static MONTH_DAY_YEAR: OnceLock<Regex> = OnceLock::new();
    static ISO_DATE: OnceLock<Regex> = OnceLock::new();
    static MONTH_YEAR: OnceLock<Regex> = OnceLock::new();
    static YEAR_ONLY: OnceLock<Regex> = OnceLock::new();

    let trimmed = text.trim();
    if DAY_MONTH_YEAR
        .get_or_init(|| {
            Regex::new(r"(?i)\b\d{1,2}\s+[A-Za-z]+\s+\d{4}\b")
                .expect("invalid regex: day month year")
        })
        .is_match(trimmed)
        || MONTH_DAY_YEAR
            .get_or_init(|| {
                Regex::new(r"(?i)\b[A-Za-z]+\s+\d{1,2},\s+\d{4}\b")
                    .expect("invalid regex: month day year")
            })
            .is_match(trimmed)
        || ISO_DATE
            .get_or_init(|| Regex::new(r"\b\d{4}-\d{2}-\d{2}\b").expect("invalid regex: ISO date"))
            .is_match(trimmed)
    {
        return 3;
    }

    if MONTH_YEAR
        .get_or_init(|| {
            Regex::new(r"(?i)\b[A-Za-z]+\s+\d{4}\b").expect("invalid regex: month year")
        })
        .is_match(trimmed)
    {
        return 2;
    }

    if YEAR_ONLY
        .get_or_init(|| Regex::new(r"\b(19|20)\d{2}\b").expect("invalid regex: year"))
        .is_match(trimmed)
    {
        return 1;
    }

    0
}

/// Parse session date.
pub(crate) fn parse_session_date(session_time: &str) -> Option<NaiveDate> {
    let date_text = session_time
        .rsplit_once(" on ")
        .map(|(_, date_text)| date_text.trim())
        .unwrap_or_else(|| session_time.trim());

    for format in ["%e %B, %Y", "%d %B, %Y", "%B %d, %Y", "%d %B %Y"] {
        if let Ok(date) = NaiveDate::parse_from_str(date_text, format) {
            return Some(date);
        }
    }

    None
}

/// Format date.
pub(crate) fn format_date(date: NaiveDate) -> String {
    date.format("%-d %B %Y").to_string()
}

/// Extract relative date answer.
pub(crate) fn extract_relative_date_answer(text: &str, session_time: &str) -> Option<String> {
    let lowered = text.to_lowercase();
    let session_date = parse_session_date(session_time)?;

    if lowered.contains("yesterday") {
        return Some(format_date(session_date - Duration::days(1)));
    }

    if lowered.contains("last year") {
        return Some((session_date.year() - 1).to_string());
    }

    if lowered.contains("last week") {
        return Some(format!(
            "The week before {}",
            session_date.format("%-d %B %Y")
        ));
    }

    if lowered.contains("last friday") {
        return Some(format!(
            "The friday before {}",
            session_date.format("%-d %B %Y")
        ));
    }

    if lowered.contains("last saturday") {
        return Some(format!(
            "The saturday before {}",
            session_date.format("%-d %B %Y")
        ));
    }

    if lowered.contains("last sunday") || lowered.contains("sunday before") {
        return Some(format!(
            "The sunday before {}",
            session_date.format("%-d %B %Y")
        ));
    }

    if lowered.contains("this month") {
        return Some(session_date.format("%B, %Y").to_string());
    }

    if lowered.contains("next month") {
        let (year, month) = if session_date.month() == 12 {
            (session_date.year() + 1, 1)
        } else {
            (session_date.year(), session_date.month() + 1)
        };
        let date = NaiveDate::from_ymd_opt(year, month, 1)?;
        return Some(date.format("%B %Y").to_string());
    }

    None
}

/// Temporal score containing exponential decay and recency boost factors for time-aware retrieval reranking.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TemporalScore {
    /// Decay factor (0.0 to 1.0) calculated via exponential decay `exp(-age_hours / 72.0)`.
    pub decay: f32,
    /// Recency boost factor derived from temporal signals in text (e.g. 1.5 when present, 1.0 otherwise).
    pub recency_boost: f32,
}

/// Computes a [`TemporalScore`] containing decay factor and recency boost for text and optional document age.
///
/// # Temporal Decay Formula
/// The temporal decay factor is calculated using exponential decay with a 72-hour half-life:
/// ```text
/// decay = exp(-age_hours / 72.0)
/// ```
/// where `age_hours` represents the age of the document or memory in hours (defaulting to 0.0 if not provided).
///
/// If temporal signals (such as explicit date formats or keywords like "yesterday", "last year",
/// "last month", or "last week") are present in `text`, a `recency_boost` of `1.5` is assigned;
/// otherwise `1.0`.
pub(crate) fn compute_temporal_score(text: &str, age_hours: Option<f32>) -> TemporalScore {
    let lowered = text.to_lowercase();
    let has_signal = extract_date_answer(text).is_some()
        || lowered.contains("yesterday")
        || lowered.contains("last year")
        || lowered.contains("last month")
        || lowered.contains("last week");

    let age = age_hours.unwrap_or(0.0).max(0.0);
    let half_life_hours = 72.0f32;
    let decay = (-age / half_life_hours).exp();
    let recency_boost = if has_signal { 1.5 } else { 1.0 };

    TemporalScore {
        decay,
        recency_boost,
    }
}

/// Computes temporal signals and decay score from text input.
///
/// Returns a structured [`TemporalScore`] containing decay factor and recency boost multiplier.
/// See [`compute_temporal_score`] for details on the decay formula used.
#[allow(dead_code, reason = "reserved for temporal reranking pipeline")]
pub(crate) fn has_temporal_signal(text: &str) -> TemporalScore {
    compute_temporal_score(text, None)
}

/// Term overlap in content.
pub(crate) fn term_overlap_in_content(doc: &RetrievedDocument, terms: &[String]) -> usize {
    let content_lower = doc.content.to_lowercase();
    terms
        .iter()
        .filter(|term| content_lower.contains(term.as_str()))
        .count()
}

/// Best date answer.
pub(crate) fn best_date_answer(query: &str, docs: &[RetrievedDocument]) -> Option<String> {
    let terms = query_terms(query);
    let phrases = query_phrases(&terms);
    let query_lower = query.to_lowercase();

    if let Some((_, resolved)) = docs
        .iter()
        .filter_map(|doc| {
            let resolved = doc
                .metadata
                .get("resolved_date")
                .and_then(|value| value.as_str())?;
            let resolved_granularity = doc
                .metadata
                .get("resolved_granularity")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let explicit_answer = extract_date_answer(&doc.content)
                .or_else(|| extract_date_answer(&doc_answer_text(doc)));
            let explicit_granularity = explicit_answer
                .as_deref()
                .map(date_granularity_rank)
                .unwrap_or_default();
            let action = doc
                .metadata
                .get("event_action")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_lowercase();
            let subject = doc
                .metadata
                .get("event_subject")
                .or_else(|| doc.metadata.get("speaker"))
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_lowercase();
            let memory_kind = doc_memory_kind(doc);
            let action_phrase_score = phrases
                .iter()
                .filter(|phrase| {
                    action.contains(phrase.as_str())
                        || doc.content.to_lowercase().contains(phrase.as_str())
                })
                .count();
            let action_score = terms
                .iter()
                .filter(|term| {
                    action.contains(term.as_str())
                        || doc.content.to_lowercase().contains(term.as_str())
                })
                .count();
            let subject_score = usize::from(!subject.is_empty() && query_lower.contains(&subject));
            let resolved_granularity_score = match resolved_granularity {
                "full_date" => 3usize,
                "month_year" => 2usize,
                "year" => 1usize,
                _ => 0usize,
            };
            let best_answer = if explicit_granularity > resolved_granularity_score {
                explicit_answer.unwrap_or_else(|| resolved.to_string())
            } else {
                resolved.to_string()
            };
            let granularity_score = resolved_granularity_score.max(explicit_granularity);
            let source_score = match memory_kind {
                "temporal_event" => 4usize,
                _ if doc_category(doc) == "conversation" => 2usize,
                _ => 0usize,
            };
            let category_penalty = usize::from(doc_category(doc) == "session_summary");
            let aligned = subject_score > 0 || action_score > 0 || action_phrase_score > 0;
            Some((
                (
                    usize::from(aligned),
                    subject_score,
                    action_phrase_score,
                    action_score,
                    source_score,
                    granularity_score,
                    score_doc_for_query(doc, &terms),
                    usize::MAX - category_penalty,
                ),
                best_answer,
            ))
        })
        .max_by_key(|(score, _)| *score)
    {
        return Some(resolved);
    }

    // Unified pass: Extract all potential (score, answer) pairs from all documents
    let best_extracted = docs
        .iter()
        .filter_map(|doc| {
            let answer_text = doc_answer_text(doc);
            let category_priority = match doc_category(doc) {
                "conversation" => 2usize,
                "observation" => 1usize,
                _ => 0usize,
            };

            let term_overlap = term_overlap_in_content(doc, &terms);
            let global_score = score_doc_for_query(doc, &terms);

            // Candidate 2: Relative date resolved against session time
            let session_time = doc.metadata.get("session_time").and_then(|v| v.as_str());
            let relative_answer =
                session_time.and_then(|st| extract_relative_date_answer(&doc.content, st));

            // Candidate 1: Explicit date in content
            let explicit_answer =
                extract_date_answer(&doc.content).or_else(|| extract_date_answer(&answer_text));

            let (answer, is_resolved) = match (relative_answer, explicit_answer) {
                (Some(rel), _) => (Some(rel), true),
                (None, Some(exp)) => (Some(exp), false),
                (None, None) => (None, false),
            };

            answer.map(|a| {
                (
                    (
                        category_priority,
                        term_overlap,
                        global_score,
                        usize::from(is_resolved),
                    ),
                    a,
                )
            })
        })
        .max_by_key(|(score, _)| *score);

    if let Some((_, answer)) = best_extracted {
        return Some(answer);
    }

    // Fallback
    let best_doc = docs
        .iter()
        .max_by_key(|doc| {
            let category_priority = match doc_category(doc) {
                "conversation" => 2usize,
                "observation" => 1usize,
                _ => 0usize,
            };

            (
                category_priority,
                term_overlap_in_content(doc, &terms),
                score_doc_for_query(doc, &terms),
            )
        })
        .or_else(|| docs.first())?;

    best_doc
        .metadata
        .get("session_time")
        .and_then(|value| value.as_str())
        .map(clean_date)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_temporal_score_with_signals() {
        let score = has_temporal_signal("The incident occurred yesterday.");
        assert_eq!(score.decay, 1.0);
        assert_eq!(score.recency_boost, 1.5);
    }

    #[test]
    fn test_temporal_score_without_signals() {
        let score = has_temporal_signal("The system operates normally.");
        assert_eq!(score.decay, 1.0);
        assert_eq!(score.recency_boost, 1.0);
    }

    #[test]
    fn test_compute_temporal_score_decay_formula() {
        let score = compute_temporal_score("meeting last week", Some(72.0));
        assert!((score.decay - (-1.0f32).exp()).abs() < 1e-4);
        assert_eq!(score.recency_boost, 1.5);
    }
}
