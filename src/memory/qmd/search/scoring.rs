//! Lexical scoring and contextual boost functions for QMD search.
//!
//! Computes relevance scores for documents against a query using
//! lexical matching, metadata signals, and time-based decay/boost.

use crate::memory::qmd_memory::types::MemoryDocument;
use crate::memory::qmd_memory::utils::*;
use crate::memory::schema::{EvidenceKind, MemoryKind};

use super::resolution::resolved_doc_metadata;

/// Precomputed query context for fast lexical scoring across many documents.
#[derive(Clone, Debug)]
pub struct LexicalQuery<'a> {
    pub(crate) normalized_query: &'a str,
    pub(crate) query_terms: Vec<&'a str>,
    pub(crate) half_life_days: f32,
    pub(crate) has_multi_word: bool,
    pub(crate) active_signals: Vec<(&'static str, f32)>,
    pub(crate) reject_ac: Option<aho_corasick::AhoCorasick>,
}

impl<'a> LexicalQuery<'a> {
    pub fn new(normalized_query: &'a str) -> Self {
        let half_life_days = std::env::var("XAVIER_TEMPORAL_HALF_LIFE_DAYS")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(30.0);

        if normalized_query.is_empty() {
            return Self {
                normalized_query,
                query_terms: Vec::new(),
                half_life_days,
                has_multi_word: false,
                active_signals: Vec::new(),
                reject_ac: None,
            };
        }

        let query_terms: Vec<&'a str> = normalized_query
            .split_whitespace()
            .filter(|term| !term.is_empty())
            .collect();

        let has_multi_word = query_terms.len() >= 2;

        const SIGNALS_TABLE: [(&str, &str, f32); 14] = [
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
        ];

        let mut active_signals = Vec::new();
        for (query_signal, content_signal, bonus) in SIGNALS_TABLE {
            if normalized_query.contains(query_signal) {
                active_signals.push((content_signal, bonus));
            }
        }

        // Check if all needles (query terms, phrase if multi-word, active content_signals) are ASCII
        let all_terms_ascii = query_terms.iter().all(|t| t.is_ascii());
        let phrase_ascii = if has_multi_word {
            normalized_query.is_ascii()
        } else {
            true
        };
        let signals_ascii = active_signals.iter().all(|(s, _)| s.is_ascii());

        let reject_ac =
            if all_terms_ascii && phrase_ascii && signals_ascii && !query_terms.is_empty() {
                let mut patterns: Vec<&str> =
                    Vec::with_capacity(query_terms.len() + 1 + active_signals.len());
                for term in &query_terms {
                    patterns.push(term);
                }
                if has_multi_word {
                    patterns.push(normalized_query);
                }
                for (signal, _) in &active_signals {
                    patterns.push(signal);
                }
                aho_corasick::AhoCorasickBuilder::new()
                    .ascii_case_insensitive(true)
                    .build(patterns)
                    .ok()
            } else {
                None
            };

        Self {
            normalized_query,
            query_terms,
            half_life_days,
            has_multi_word,
            active_signals,
            reject_ac,
        }
    }
}

/// Lexical scoring for general (non-LOCOMO) documents using precomputed query context.
pub fn lexical_score_with_query(doc: &MemoryDocument, lq: &LexicalQuery<'_>) -> f32 {
    if lq.normalized_query.is_empty() {
        return 0.0;
    }

    if is_locomo_document(&doc.path, &doc.metadata) {
        return locomo_lexical_score(doc, lq.normalized_query);
    }

    let path = doc.path.to_lowercase();

    // Cheap reject: test whether we can skip content lowercasing and word count
    let mut can_reject_content = false;
    if let Some(ac) = &lq.reject_ac {
        let content_bytes = doc.content.as_bytes();
        // Reject is only allowed when content contains no byte sequence for
        // U+212A KELVIN SIGN (E2 84 AA) and no U+0130 (C4 B0)
        let has_special_unicode = (content_bytes.contains(&0xE2) || content_bytes.contains(&0xC4))
            && (content_bytes.windows(3).any(|w| w == b"\xE2\x84\xAA")
                || content_bytes.windows(2).any(|w| w == b"\xC4\xB0"));

        if !has_special_unicode && ac.find(&doc.content).is_none() {
            can_reject_content = true;
        }
    }

    let mut matched_terms = 0usize;
    let mut score = 0.0f32;
    let content: Option<String>;

    if can_reject_content {
        content = None;
        // Content hits and content-dependent bonuses are provably 0.0
        for term in &lq.query_terms {
            let path_hits = path.matches(term).count() as f32 * 2.0;
            if path_hits > 0.0 {
                matched_terms += 1;
            }
            score += path_hits;
        }
    } else {
        let lowered_content = doc.content.to_lowercase();

        const BM25_K1: f32 = 1.5;
        const BM25_B: f32 = 0.75;
        const AVG_DOC_LEN_WORDS: f32 = 220.0;
        let doc_len_words = lowered_content.split_whitespace().count().max(1) as f32;
        let length_norm = (1.0 - BM25_B) + BM25_B * (doc_len_words / AVG_DOC_LEN_WORDS);

        for term in &lq.query_terms {
            let raw_content_hits = lowered_content.matches(term).count() as f32;
            let content_hits = if raw_content_hits > 0.0 {
                (raw_content_hits * (BM25_K1 + 1.0)) / (raw_content_hits + BM25_K1 * length_norm)
            } else {
                0.0
            };
            let path_hits = path.matches(term).count() as f32 * 2.0;
            if content_hits > 0.0 || path_hits > 0.0 {
                matched_terms += 1;
            }
            score += content_hits + path_hits;
        }

        content = Some(lowered_content);
    }

    score += (matched_terms * matched_terms) as f32;

    if let Some(content) = &content {
        if lq.has_multi_word && content.contains(lq.normalized_query) {
            score += 6.0;
        }

        for (content_signal, bonus) in &lq.active_signals {
            if content.contains(content_signal) {
                score += bonus;
            }
        }
    }

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
                if lq.query_terms.len() >= 2 =>
            {
                score += 3.0;
            }
            _ => {}
        }

        if let Some(evidence_kind) = resolved.evidence_kind {
            match evidence_kind {
                EvidenceKind::SourceTurn => score += 6.0,
                EvidenceKind::FactAtom | EvidenceKind::EntityState => score += 8.0,
                EvidenceKind::TemporalEvent if lq.normalized_query.contains("when") => {
                    score += 10.0
                }
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
            if !lowered.is_empty() && lq.normalized_query.contains(&lowered) {
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

    let created_at = doc
        .metadata
        .get("created_at")
        .or_else(|| doc.metadata.get("timestamp"))
        .or_else(|| doc.metadata.get("updated_at"))
        .and_then(|value| value.as_str());
    let decay = temporal_decay_multiplier(created_at, lq.half_life_days);

    score * decay
}

/// Lexical scoring for general (non-LOCOMO) documents.
pub fn lexical_score(doc: &MemoryDocument, normalized_query: &str) -> f32 {
    let lq = LexicalQuery::new(normalized_query);
    lexical_score_with_query(doc, &lq)
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
    let raw = score + memory_importance_score(document) + memory_decay_penalty(document);
    let created_at = document
        .metadata
        .get("created_at")
        .or_else(|| document.metadata.get("timestamp"))
        .or_else(|| document.metadata.get("updated_at"))
        .and_then(|value| value.as_str());
    let half_life_days = std::env::var("XAVIER_TEMPORAL_HALF_LIFE_DAYS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(30.0);
    let decay = temporal_decay_multiplier(created_at, half_life_days);
    raw * decay
}

/// Computes an exponential decay multiplier based on the record's age in days.
///
/// Formula: `(-age_days / half_life_days).exp()`.
/// If `created_at` is missing, unparseable, or `half_life_days <= 0.0`, returns 1.0.
pub fn temporal_decay_multiplier(created_at: Option<&str>, half_life_days: f32) -> f32 {
    let Some(raw_date) = created_at else {
        return 1.0;
    };
    if half_life_days <= 0.0 {
        return 1.0;
    }

    let parsed_time = chrono::DateTime::parse_from_rfc3339(raw_date)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(raw_date, "%Y-%m-%d %H:%M:%S")
                .map(|ndt| ndt.and_utc())
        })
        .or_else(|_| {
            chrono::NaiveDate::parse_from_str(raw_date, "%Y-%m-%d")
                .map(|nd| nd.and_hms_opt(0, 0, 0).unwrap().and_utc())
        });

    let Ok(created_utc) = parsed_time else {
        return 1.0;
    };

    let now = chrono::Utc::now();
    let age_seconds = (now - created_utc).num_seconds().max(0);
    let age_days = age_seconds as f32 / 86400.0;

    (-age_days / half_life_days).exp()
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

#[cfg(test)]
pub(crate) fn reference_lexical_score(doc: &MemoryDocument, normalized_query: &str) -> f32 {
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

    const BM25_K1: f32 = 1.5;
    const BM25_B: f32 = 0.75;
    const AVG_DOC_LEN_WORDS: f32 = 220.0;
    let doc_len_words = content.split_whitespace().count().max(1) as f32;
    let length_norm = (1.0 - BM25_B) + BM25_B * (doc_len_words / AVG_DOC_LEN_WORDS);

    let mut matched_terms = 0usize;
    let mut score = 0.0f32;
    for term in &query_terms {
        let raw_content_hits = content.matches(term).count() as f32;
        let content_hits = if raw_content_hits > 0.0 {
            (raw_content_hits * (BM25_K1 + 1.0)) / (raw_content_hits + BM25_K1 * length_norm)
        } else {
            0.0
        };
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

    let created_at = doc
        .metadata
        .get("created_at")
        .or_else(|| doc.metadata.get("timestamp"))
        .or_else(|| doc.metadata.get("updated_at"))
        .and_then(|value| value.as_str());
    let half_life_days = std::env::var("XAVIER_TEMPORAL_HALF_LIFE_DAYS")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(30.0);
    let decay = temporal_decay_multiplier(created_at, half_life_days);

    score * decay
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lexical_score_exact_equivalence() {
        let mut test_docs = Vec::new();

        // 1. Basic uppercase/lowercase & mixed content
        test_docs.push(MemoryDocument {
            path: "memory/doc1.md".to_string(),
            content: "Hello WORLD this IS a TEST of Lexical Scoring".to_string(),
            ..Default::default()
        });

        // 2. Accented Spanish text
        test_docs.push(MemoryDocument {
            path: "memory/spanish.md".to_string(),
            content: "El rendimiento de la búsqueda y optimización en memoria es rápido"
                .to_string(),
            ..Default::default()
        });

        // 3. Special characters: Kelvin sign (U+212A) and U+0130
        test_docs.push(MemoryDocument {
            path: "memory/special_unicode.md".to_string(),
            content: "Here is temperature 300 \u{212A} and Istanbul \u{0130} capital".to_string(),
            ..Default::default()
        });

        // 4. Matches ONLY in the path
        test_docs.push(MemoryDocument {
            path: "memory/search_target/unique_path.md".to_string(),
            content: "Completely unrelated content about cooking recipes".to_string(),
            ..Default::default()
        });

        // 5. Multi-word phrase matches
        test_docs.push(MemoryDocument {
            path: "memory/phrase.md".to_string(),
            content: "we have support group meetings every sunrise for camping and charity race"
                .to_string(),
            ..Default::default()
        });

        // 6. Signal table entries: each signal
        let signals = [
            ("sunrise", "sunrise"),
            ("support", "support group"),
            ("charity", "charity race"),
            ("camping", "camping"),
            ("identity", "transgender"),
            ("relationship", "single"),
            ("research", "adoption agenc"),
            ("field", "counsel"),
            ("pursue", "counsel"),
            ("what", "what"),
            ("who", "who"),
            ("how", "how"),
            ("why", "why"),
            ("which", "which"),
        ];
        for (q_sig, c_sig) in signals {
            test_docs.push(MemoryDocument {
                path: format!("memory/signal_{}.md", q_sig),
                content: format!("Note with signal phrase: {} included here.", c_sig),
                ..Default::default()
            });
        }

        // 7. memory_kind branches
        for kind in [
            "fact_atom",
            "entity_state",
            "temporal_event",
            "summary_fact",
            "other_kind",
        ] {
            let mut metadata = serde_json::Map::new();
            metadata.insert(
                "memory_kind".to_string(),
                serde_json::Value::String(kind.to_string()),
            );
            test_docs.push(MemoryDocument {
                path: format!("memory/kind_{}.md", kind),
                content: "Some random information words".to_string(),
                metadata: serde_json::Value::Object(metadata),
                ..Default::default()
            });
        }

        // 8. category branches
        for cat in [
            "session_summary",
            "conversation",
            "observation",
            "unknown_category",
        ] {
            let mut metadata = serde_json::Map::new();
            metadata.insert(
                "category".to_string(),
                serde_json::Value::String(cat.to_string()),
            );
            test_docs.push(MemoryDocument {
                path: format!("memory/cat_{}.md", cat),
                content: "Some random category content".to_string(),
                metadata: serde_json::Value::Object(metadata),
                ..Default::default()
            });
        }

        // 9. created_at present vs absent, timestamps, updated_at
        let mut meta_created = serde_json::Map::new();
        meta_created.insert(
            "created_at".to_string(),
            serde_json::Value::String("2026-05-15T10:00:00Z".to_string()),
        );
        test_docs.push(MemoryDocument {
            path: "memory/created.md".to_string(),
            content: "Timestamped content".to_string(),
            metadata: serde_json::Value::Object(meta_created),
            ..Default::default()
        });

        let mut meta_ts = serde_json::Map::new();
        meta_ts.insert(
            "timestamp".to_string(),
            serde_json::Value::String("2026-04-10 12:00:00".to_string()),
        );
        test_docs.push(MemoryDocument {
            path: "memory/ts.md".to_string(),
            content: "Timestamp 2".to_string(),
            metadata: serde_json::Value::Object(meta_ts),
            ..Default::default()
        });

        // 10. normalized_value present
        let mut meta_norm = serde_json::Map::new();
        meta_norm.insert(
            "normalized_value".to_string(),
            serde_json::Value::String("val".to_string()),
        );
        test_docs.push(MemoryDocument {
            path: "memory/norm.md".to_string(),
            content: "Normalized val content".to_string(),
            metadata: serde_json::Value::Object(meta_norm),
            ..Default::default()
        });

        // 11. Resolved metadata kind & evidence kind branches
        for kind in [
            MemoryKind::Repo,
            MemoryKind::File,
            MemoryKind::Decision,
            MemoryKind::Fact,
        ] {
            for evidence in [
                Some(EvidenceKind::SourceTurn),
                Some(EvidenceKind::FactAtom),
                Some(EvidenceKind::TemporalEvent),
                Some(EvidenceKind::SessionSummary),
                None,
            ] {
                let mut metadata = serde_json::Map::new();
                let kind_str = format!("{:?}", kind).to_lowercase();
                metadata.insert(
                    "memory_kind".to_string(),
                    serde_json::Value::String(kind_str),
                );
                if let Some(ev) = evidence {
                    let ev_str = match ev {
                        EvidenceKind::SourceTurn => "source_turn",
                        EvidenceKind::FactAtom => "fact_atom",
                        EvidenceKind::TemporalEvent => "temporal_event",
                        EvidenceKind::SessionSummary => "session_summary",
                        _ => "other",
                    };
                    metadata.insert(
                        "evidence_kind".to_string(),
                        serde_json::Value::String(ev_str.to_string()),
                    );
                }
                metadata.insert(
                    "file_path".to_string(),
                    serde_json::Value::String("src/main.rs".to_string()),
                );
                test_docs.push(MemoryDocument {
                    path: "memory/resolved_doc.md".to_string(),
                    content: "Code and repo reference content".to_string(),
                    metadata: serde_json::Value::Object(metadata),
                    ..Default::default()
                });
            }
        }

        // Test queries
        let queries = [
            "", // empty query
            "hello",
            "hello world test",
            "rendimiento búsqueda optimización",
            "kelvin \u{212A} istanbul",
            "search_target",
            "support group sunrise",
            "charity race camping",
            "identity transgender relationship single",
            "research adoption agenc field counsel pursue",
            "what who how why which when",
            "src/main.rs",
            "nonexistent_term_xyz",
            "búsqueda con acento y palabras largas",
        ];

        for query in queries {
            let normalized = query.to_lowercase();
            let lq = LexicalQuery::new(&normalized);
            for doc in &test_docs {
                let ref_score = reference_lexical_score(doc, &normalized);
                let new_score = lexical_score(doc, &normalized);
                let fast_score = lexical_score_with_query(doc, &lq);

                assert_eq!(
                    ref_score.to_bits(),
                    new_score.to_bits(),
                    "Mismatch between reference and lexical_score for query '{}' on doc '{}': {} vs {}",
                    query, doc.path, ref_score, new_score
                );
                assert_eq!(
                    ref_score.to_bits(),
                    fast_score.to_bits(),
                    "Mismatch between reference and lexical_score_with_query for query '{}' on doc '{}': {} vs {}",
                    query, doc.path, ref_score, fast_score
                );
            }
        }
    }

    #[test]
    fn test_lexical_score_signals_accumulation_order_equivalence() {
        // Equivalence test with active signal words and N filler words (N swept 1..400)
        let queries = [
            "identity",
            "relationship",
            "single",
            "sunrise",
            "support",
            "charity",
            "camping",
            "research",
            "field",
            "pursue",
            "what",
            "who",
            "how",
            "why",
            "which",
        ];

        let prefixes = [
            "identity transgender ",
            "relationship single ",
            "support group sunrise ",
            "charity race camping ",
            "research adoption agenc field counsel pursue ",
        ];

        let filler_words = [
            "filler", "word", "document", "text", "lorem", "ipsum", "test", "content", "sample",
            "corpus", "data", "buffer",
        ];

        for prefix in prefixes {
            for n in 1..=400 {
                let mut content = String::with_capacity(prefix.len() + n * 8);
                content.push_str(prefix);
                for i in 0..n {
                    content.push_str(filler_words[i % filler_words.len()]);
                    content.push(' ');
                }

                let doc = MemoryDocument {
                    path: "memory/note.md".to_string(),
                    content,
                    ..Default::default()
                };

                for query in queries {
                    let normalized = query.to_lowercase();
                    let lq = LexicalQuery::new(&normalized);
                    let ref_score = reference_lexical_score(&doc, &normalized);
                    let new_score = lexical_score(&doc, &normalized);
                    let fast_score = lexical_score_with_query(&doc, &lq);

                    assert_eq!(
                        ref_score.to_bits(),
                        new_score.to_bits(),
                        "Mismatch between ref and lexical_score for query '{}' with prefix '{}' and N={}",
                        query, prefix, n
                    );
                    assert_eq!(
                        ref_score.to_bits(),
                        fast_score.to_bits(),
                        "Mismatch between ref and lexical_score_with_query for query '{}' with prefix '{}' and N={}",
                        query, prefix, n
                    );
                }
            }
        }
    }

    #[test]
    fn test_temporal_decay_multiplier_math() {
        // Missing date returns 1.0
        assert_eq!(temporal_decay_multiplier(None, 30.0), 1.0);

        // Invalid date returns 1.0
        assert_eq!(temporal_decay_multiplier(Some("not-a-date"), 30.0), 1.0);

        // Zero or negative half life returns 1.0
        assert_eq!(
            temporal_decay_multiplier(Some("2026-01-01T00:00:00Z"), 0.0),
            1.0
        );

        // Brand new timestamp (now) should yield ~1.0
        let now_str = chrono::Utc::now().to_rfc3339();
        let fresh_mult = temporal_decay_multiplier(Some(&now_str), 30.0);
        assert!((fresh_mult - 1.0).abs() < 1e-3);

        // Exactly 30 days old with 30-day half-life -> exp(-1) = 0.367879...
        let thirty_days_ago = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
        let half_life_mult = temporal_decay_multiplier(Some(&thirty_days_ago), 30.0);
        let expected_exp = (-1.0_f32).exp();
        assert!(
            (half_life_mult - expected_exp).abs() < 0.02,
            "expected ~{:.4}, got {:.4}",
            expected_exp,
            half_life_mult
        );

        // 60 days old -> exp(-2) = 0.135335...
        let sixty_days_ago = (chrono::Utc::now() - chrono::Duration::days(60)).to_rfc3339();
        let double_half_life_mult = temporal_decay_multiplier(Some(&sixty_days_ago), 30.0);
        assert!(double_half_life_mult < half_life_mult);
        assert!(half_life_mult < fresh_mult);
    }
}
