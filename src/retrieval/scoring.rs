use crate::memory::qmd_memory::MemoryDocument;
use crate::memory::schema::ContextZone;
use crate::retrieval::config;
use crate::search::rrf::ScoredResult;
use crate::retrieval::gating::SessionSummary;
use crate::memory::entity_graph::EntityRecord;

pub struct WorkingScoringParams<'a> {
    pub query_lower: &'a str,
    pub query_terms: &'a [&'a str],
    pub active_zones: Option<&'a Vec<ContextZone>>,
    pub zone_boost_multiplier: f32,
    pub zone_penalty_multiplier: f32,
    pub now: chrono::DateTime<chrono::Utc>,
    pub recency_weight: f32,
    pub half_life_hours: f32,
}

pub fn calculate_recency_boost_factor(
    updated_at_ms: Option<i64>,
    now: chrono::DateTime<chrono::Utc>,
    recency_weight: f32,
    half_life_hours: f32,
) -> f32 {
    if recency_weight <= 0.0 { return 1.0; }
    let Some(updated_at_ms) = updated_at_ms else { return 1.0; };
    let updated_at = chrono::DateTime::from_timestamp_millis(updated_at_ms).unwrap_or(now);
    let age_hours = (now - updated_at).num_hours() as f32;
    let age_hours = age_hours.max(0.0);
    if half_life_hours <= 0.0 { return 1.0 + recency_weight; }
    1.0 + (recency_weight * (-age_hours / half_life_hours).exp())
}

pub fn score_single_working(doc: &MemoryDocument, params: &WorkingScoringParams<'_>) -> Option<ScoredResult> {
    let content_lower = doc.content.to_lowercase();
    let mut score = 0.0_f32;
    if content_lower.contains(params.query_lower) { score += config::EXACT_PHRASE_MATCH_BONUS; }
    for term in params.query_terms {
        if content_lower.contains(term) {
            score += config::TERM_MATCH_BONUS;
            let count = content_lower.matches(term).count() as f32;
            score += (count * config::TERM_OCCURRENCE_BONUS).min(config::MAX_TERM_OCCURRENCE_BONUS);
        }
    }
    if score > 0.0 {
        let doc_zone = doc.metadata.get("zone").and_then(|v| v.as_str()).map(ContextZone::parse).unwrap_or(ContextZone::Atomic);
        let mut final_score = score.min(1.0);
        if let Some(active) = params.active_zones { if active.contains(&doc_zone) { final_score *= params.zone_boost_multiplier; } else { final_score *= params.zone_penalty_multiplier; } }
        let updated_at_ms = doc.metadata.get("updated_at").and_then(|v| v.as_str()).and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|dt| dt.timestamp_millis());
        let recency = calculate_recency_boost_factor(updated_at_ms, params.now, params.recency_weight, params.half_life_hours);
        final_score *= recency;
        Some(ScoredResult { id: doc.id.clone().unwrap_or_default(), content: doc.content.clone(), score: final_score, source: "working".to_string(), path: doc.path.clone(), updated_at: updated_at_ms })
    } else { None }
}

pub fn score_single_episodic(session: &SessionSummary, query_lower: &str, query_terms: &[&str], now: chrono::DateTime<chrono::Utc>, recency_weight: f32, half_life_hours: f32) -> Option<ScoredResult> {
    let summary_lower = session.summary.to_lowercase();
    let mut score = 0.0_f32;
    if summary_lower.contains(query_lower) { score += config::EXACT_PHRASE_MATCH_BONUS; }
    for term in query_terms {
        if summary_lower.contains(term) {
            score += config::TERM_MATCH_BONUS;
            let count = summary_lower.matches(term).count() as f32;
            score += (count * config::TERM_OCCURRENCE_BONUS).min(config::MAX_TERM_OCCURRENCE_BONUS);
        }
    }
    for event in &session.key_events {
        let event_lower = event.description.to_lowercase();
        if event_lower.contains(query_lower) { score += config::EVENT_PHRASE_MATCH_BONUS; }
        for term in query_terms { if event_lower.contains(term) { score += config::EVENT_TERM_MATCH_BONUS; } }
    }
    if score > 0.0 {
        let mut final_score = score.min(1.0);
        let updated_at_ms = Some(session.start_time.timestamp_millis());
        let recency = calculate_recency_boost_factor(updated_at_ms, now, recency_weight, half_life_hours);
        final_score *= recency;
        Some(ScoredResult { id: session.session_id.clone(), content: session.summary.clone(), score: final_score, source: "episodic".to_string(), path: format!("sessions/{}", session.session_id), updated_at: updated_at_ms })
    } else { None }
}

pub fn score_single_semantic(entity: &EntityRecord, query_lower: &str, now: chrono::DateTime<chrono::Utc>, recency_weight: f32, half_life_hours: f32) -> Option<ScoredResult> {
    let name_lower = entity.name.to_lowercase();
    let normalized_lower = entity.normalized_name.to_lowercase();
    let mut score = 0.0_f32;
    if name_lower == query_lower || normalized_lower == query_lower { score = config::EXACT_ENTITY_MATCH_SCORE; }
    else if name_lower.contains(query_lower) || query_lower.contains(&name_lower) { score = config::PARTIAL_ENTITY_MATCH_SCORE; }
    else if let Some(desc) = &entity.description {
        let desc_lower = desc.to_lowercase();
        if desc_lower.contains(query_lower) { score = config::ENTITY_DESCRIPTION_MATCH_SCORE; }
        let query_terms: Vec<&str> = query_lower.split_whitespace().collect();
        for term in &query_terms { if desc_lower.contains(term) { score += config::ENTITY_DESCRIPTION_TERM_BONUS; } }
    }
    else { for alias in &entity.aliases { if alias.to_lowercase().contains(query_lower) { score = config::ENTITY_ALIAS_MATCH_SCORE; break; } } }
    let mut final_score = (score * config::SEMANTIC_CONFIDENCE_MULTIPLIER).min(1.0);
    if final_score > 0.0 {
        let updated_at_ms = Some(entity.last_seen.timestamp_millis());
        let recency = calculate_recency_boost_factor(updated_at_ms, now, recency_weight, half_life_hours);
        final_score *= recency;
        Some(ScoredResult { id: entity.id.clone(), content: entity.name.clone(), score: final_score, source: "semantic".to_string(), path: format!("entities/{}", entity.id), updated_at: updated_at_ms })
    } else { None }
}
