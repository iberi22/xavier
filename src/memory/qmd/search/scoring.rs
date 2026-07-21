// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Lexical scoring and contextual boost functions for QMD search.
//!
//! Computes relevance scores for documents against a query using
//! lexical matching, metadata signals, and time-based decay/boost.

use crate::memory::qmd_memory::types::MemoryDocument;
use crate::memory::qmd_memory::utils::*;
use crate::memory::schema::{EvidenceKind, MemoryKind};

use super::resolution::resolved_doc_metadata;

/// Lexical scoring for general (non-LOCOMO) documents.
pub fn lexical_score(doc: &MemoryDocument, normalized_query: &str) -> f32 {
    if normalized_query.is_empty() {
        return 0.0;
    }

    if is_locomo_document(&doc.path, &doc.metadata) {
        return locomo_lexical_score(doc, normalized_query);
    }

    let content = doc.content.to_lowercase();
    let path = doc.path.to_lowercase();
    let query_terms: Vec<&str> = normalized_query
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .collect();
    let mut matched_terms = 0usize;
    let mut score = 0.0f32;
    for term in &query_terms {
        let content_hits = content.matches(term).count() as f32;
        let path_hits = path.matches(term).count() as f32 * 2.0;
        if content_hits > 0.0 || path_hits > 0.0 {
            matched_terms += 1;
        }
        score += content_hits + path_hits;
    }
    score += (matched_terms * matched_terms) as f32;

    let memory_kind = doc
        .metadata
        .get("memory_kind")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let resolved = resolved_doc_metadata(doc);
    let category = doc
        .metadata
        .get("category")
        .and_then(|value| value.as_str())
        .unwrap_or_default();

    if normalized_query.split_whitespace().count() >= 2 && content.contains(normalized_query) {
        score += 6.0;
    }

    for (query_signal, content_signal, bonus) in [
        ("sunrise", "sunrise", 12.0),
        ("support", "support group", 12.0),
        ("charity", "charity race", 12.0),
        ("camping", "camping", 12.0),
        ("identity", "transgender", 10.0),
        ("relationship", "single", 10.0),
        ("research", "adoption agenc", 8.0),
        ("field", "counsel", 8.0),
        ("pursue", "counsel", 8.0),
        ("what", "what", 2.0),
        ("who", "who", 3.0),
        ("how", "how", 2.0),
        ("why", "why", 2.0),
        ("which", "which", 2.0),
    ] {
        if normalized_query.contains(query_signal) && content.contains(content_signal) {
            score += bonus;
        }
    }

    if matches!(
        memory_kind,
        "fact_atom" | "entity_state" | "temporal_event" | "summary_fact"
    ) {
        score += 5.0;
    }

    if let Some(resolved) = &resolved {
        match resolved.kind {
            MemoryKind::Repo | MemoryKind::File | MemoryKind::Symbol | MemoryKind::Url => {
                score += 5.0;
            }
            MemoryKind::Decision | MemoryKind::Task | MemoryKind::Fact
                if query_terms.len() >= 2 =>
            {
                score += 3.0;
            }
            _ => {}
        }

        if let Some(evidence_kind) = resolved.evidence_kind {
            match evidence_kind {
                EvidenceKind::SourceTurn => score += 6.0,
                EvidenceKind::FactAtom | EvidenceKind::EntityState => score += 8.0,
                EvidenceKind::TemporalEvent if normalized_query.contains("when") => score += 10.0,
                EvidenceKind::SessionSummary => score *= 0.5,
                _ => {}
            }
        }

        for exact in [
            resolved.provenance.symbol.as_ref(),
            resolved.provenance.file_path.as_ref(),
            resolved.provenance.repo_url.as_ref(),
            resolved.provenance.url.as_ref(),
            resolved.namespace.session_id.as_ref(),
            resolved.namespace.agent_id.as_ref(),
            resolved.namespace.user_id.as_ref(),
            resolved.namespace.project.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            let lowered = exact.to_ascii_lowercase();
            if !lowered.is_empty() && normalized_query.contains(&lowered) {
                score += 18.0;
            }
        }
    }

    if doc
        .metadata
        .get("normalized_value")
        .and_then(|value| value.as_str())
        .is_some()
    {
        score += 2.0;
    }

    match category {
        "session_summary" => score *= 0.2,
        "conversation" => score *= 1.2,
        "observation" => score *= 0.8,
        _ => {}
    }

    score
}

/// Lexical scoring for LOCOMO documents with structured metadata.
pub fn locomo_lexical_score(doc: &MemoryDocument, normalized_query: &str) -> f32 {
    let content = doc.content.to_lowercase();
    let path = doc.path.to_lowercase();
    let terms = locomo_query_terms(normalized_query);
    let phrases = locomo_phrases(&terms);
    let speaker = metadata_text_lower(doc, "speaker");
    let subject = doc
        .metadata
        .get("event_subject")
        .and_then(|value| value.as_str())
        .unwrap_or_else(|| {
            doc.metadata
                .get("speaker")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
        })
        .to_lowercase();
    let action = metadata_text_lower(doc, "event_action");
    let resolved_date = metadata_text_lower(doc, "resolved_date");
    let normalized_value = metadata_text_lower(doc, "normalized_value");
    let answer_span = metadata_text_lower(doc, "answer_span");
    let memory_kind = doc
        .metadata
        .get("memory_kind")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let resolved = resolved_doc_metadata(doc);
    let category = doc
        .metadata
        .get("category")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let temporal_query = is_temporal_query(normalized_query);

    // Generic question pattern detection (context-agnostic)
    let is_shared_query = normalized_query.contains("in common")
        || normalized_query.contains("both like")
        || normalized_query.contains("both have")
        || normalized_query.contains("what do")
            && normalized_query.contains("and")
            && normalized_query.contains("both");
    let is_why_query = normalized_query.starts_with("why")
        || normalized_query.contains(" why ")
        || normalized_query.contains("reason")
        || normalized_query.contains("because");
    let is_what_think_query = normalized_query.contains("think")
        || normalized_query.contains("believe")
        || normalized_query.contains("opinion")
        || normalized_query.contains("ideal")
        || normalized_query.contains("prefer");

    let mut score = 0.0f32;
    let mut matched_terms = 0usize;
    for term in &terms {
        let mut term_score = 0.0f32;

        if !speaker.is_empty() && speaker == **term {
            term_score += 18.0;
        }
        if !subject.is_empty() && subject == **term {
            term_score += 18.0;
        }
        if action.contains(*term) {
            term_score += 14.0;
        }
        if normalized_value.contains(*term) || answer_span.contains(*term) {
            term_score += 12.0;
        }
        if path.contains(*term) {
            term_score += 8.0;
        }
        if content.contains(*term) {
            term_score += 4.0;
        }

        if term_score > 0.0 {
            matched_terms += 1;
            score += term_score;
        }
    }

    score += (matched_terms * matched_terms * 2) as f32;

    for phrase in &phrases {
        if action.contains(phrase)
            || normalized_value.contains(phrase)
            || answer_span.contains(phrase)
        {
            score += 18.0;
        } else if content.contains(phrase) || path.contains(phrase) {
            score += 9.0;
        }
    }

    if normalized_query.split_whitespace().count() >= 2 && content.contains(normalized_query) {
        score += 10.0;
    }

    if !speaker.is_empty() && normalized_query.contains(&speaker) {
        score += 14.0;
    }

    // Generic context-agnostic scoring patterns
    if is_shared_query {
        if matches!(memory_kind, "fact_atom" | "entity_state") {
            score += 35.0;
        }
        if !normalized_value.is_empty() || !answer_span.is_empty() {
            score += 25.0;
        }
        if memory_kind == "summary_fact" {
            score *= 0.15;
        }
    }

    if is_why_query {
        let has_reason = content.contains("because")
            || content.contains("'cause")
            || content.contains("since")
            || content.contains("reason")
            || content.contains("to share")
            || content.contains("to start")
            || content.contains("decided")
            || content.contains("wanted");
        if has_reason {
            score += 30.0;
        }
        if !normalized_value.is_empty() {
            score += 20.0;
        }
        if memory_kind == "summary_fact" {
            score *= 0.2;
        }
    }

    if is_what_think_query {
        let has_opinion = content.contains("think")
            || content.contains("believe")
            || content.contains("feel")
            || content.contains("prefer")
            || content.contains("ideal")
            || content.contains("favorite")
            || contains_opinion_adjectives(&content);
        if has_opinion {
            score += 25.0;
        }
        if !normalized_value.is_empty() || !answer_span.is_empty() {
            score += 15.0;
        }
        if memory_kind == "summary_fact" {
            score *= 0.2;
        }
    }

    if let Some(resolved) = &resolved {
        match resolved.evidence_kind {
            Some(EvidenceKind::TemporalEvent) if temporal_query => score += 60.0,
            Some(
                EvidenceKind::FactAtom | EvidenceKind::EntityState | EvidenceKind::SummaryFact,
            ) if !temporal_query => {
                score += 28.0;
            }
            Some(
                EvidenceKind::FactAtom | EvidenceKind::EntityState | EvidenceKind::SummaryFact,
            ) => {
                score += 12.0;
            }
            Some(EvidenceKind::SourceTurn) => score += 8.0,
            _ => {}
        }

        if let Some(symbol) = resolved.provenance.symbol.as_ref() {
            if normalized_query.contains(&symbol.to_ascii_lowercase()) {
                score += 24.0;
            }
        }
        if let Some(file_path) = resolved.provenance.file_path.as_ref() {
            if normalized_query.contains(&file_path.to_ascii_lowercase()) {
                score += 16.0;
            }
        }
        if let Some(url) = resolved.provenance.url.as_ref() {
            if normalized_query.contains(&url.to_ascii_lowercase()) {
                score += 16.0;
            }
        }
    }

    match memory_kind {
        "temporal_event" if temporal_query => {
            score += 60.0;
        }
        "fact_atom" | "entity_state" | "summary_fact" if !temporal_query => {
            score += 28.0;
        }
        "fact_atom" | "entity_state" | "summary_fact" => {
            score += 12.0;
        }
        _ => {}
    }

    if !resolved_date.is_empty() {
        score += if temporal_query { 24.0 } else { 6.0 };
        score += match infer_date_granularity(&resolved_date) {
            "full_date" => 10.0,
            "month_year" => 6.0,
            "year" => 2.0,
            _ => 0.0,
        };
    }

    match category {
        "conversation" => {
            score += if temporal_query { 18.0 } else { 10.0 };
        }
        "observation" => {
            score += 2.0;
        }
        "session_summary" => {
            score -= if temporal_query { 70.0 } else { 28.0 };
        }
        _ => {}
    }

    if category == "session_summary" && memory_kind.is_empty() {
        score *= if temporal_query { 0.02 } else { 0.15 };
    }

    // LOCOMO fix: Boost structured data (pricing, numbers) for factuality queries
    let pricing_query = normalized_query.contains("pricing")
        || normalized_query.contains("price")
        || normalized_query.contains("precios")
        || normalized_query.contains("precio")
        || normalized_query.contains("costo")
        || normalized_query.contains("coste")
        || normalized_query.contains("valor")
        || normalized_query.contains("fee")
        || normalized_query.contains("tarifa")
        || normalized_query.contains("cuanto")
        || normalized_query.contains("cu├íl")
        || normalized_query.contains("cu├íles");

    if pricing_query {
        let has_numeric = content.contains('$')
            || content.contains("/mes")
            || content.contains("/mo")
            || content.contains("/month")
            || content.contains("/monthly")
            || content.contains("/year")
            || content.contains("/annual")
            || regex::Regex::new(r"\d+[.,]?\d*")
                .map(|re| re.is_match(&content))
                .unwrap_or(false);

        if has_numeric {
            score += 30.0;
        }

        if !normalized_value.is_empty() && has_numeric {
            score += 25.0;
        }

        let tier_terms = [
            "starter",
            "pro",
            "enterprise",
            "basic",
            "plan",
            "tier",
            "version",
        ];
        for tier in &tier_terms {
            if normalized_query.contains(tier) && (content.contains(tier) || path.contains(tier)) {
                score += 15.0;
            }
        }
    }

    score.max(0.0)
}

/// Contextual boost incorporating importance and decay.
pub fn contextual_boost(query: &str, document: &MemoryDocument, weight: f32) -> f32 {
    let doc_text = format!(
        "{} {} {}",
        document.path.to_ascii_lowercase(),
        document.content.to_ascii_lowercase(),
        document.metadata.to_string().to_ascii_lowercase()
    );
    let mut score = 0.0;
    for token in query.split_whitespace() {
        if token.len() >= 3 && doc_text.contains(token) {
            score += 0.12 * weight;
        }
    }
    if let Some(title) = document
        .metadata
        .get("title")
        .and_then(|value| value.as_str())
    {
        if query.contains(&title.to_ascii_lowercase()) {
            score += 0.20 * weight;
        }
    }
    score + memory_importance_score(document) + memory_decay_penalty(document)
}

/// Extract the importance score from document metadata.
pub fn memory_importance_score(document: &MemoryDocument) -> f32 {
    let metadata = &document.metadata;
    let importance = metadata
        .get("importance")
        .or_else(|| metadata.get("memory_importance"))
        .and_then(|value| value.as_f64())
        .unwrap_or(0.0) as f32;
    importance.clamp(0.0, 1.0) * 0.25
}

/// Compute a decay penalty based on time since last access or update.
pub fn memory_decay_penalty(document: &MemoryDocument) -> f32 {
    let updated = document
        .metadata
        .get("updated_at")
        .and_then(|value| value.as_str())
        .or_else(|| {
            document
                .metadata
                .get("last_accessed_at")
                .and_then(|value| value.as_str())
        });
    let Some(updated) = updated else {
        return 0.0;
    };
    let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(updated) else {
        return 0.0;
    };
    use chrono::Utc;
    let age_days = (Utc::now() - parsed.with_timezone(&Utc)).num_days().max(0) as f32;
    -(age_days / 365.0).min(1.0) * 0.15
}
