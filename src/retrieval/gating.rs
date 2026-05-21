//! Adaptive Retrieval Gating - Multi-layer memory retrieval with weighted fusion
//!
//! Implements adaptive gating that scores and fuses results from Working, Episodic,
//! and Semantic memory layers using RRF (Reciprocal Rank Fusion).

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::memory::entity_graph::EntityRecord;
use crate::memory::qmd_memory::MemoryDocument;
use crate::memory::schema::ContextZone;
use crate::retrieval::config;
use crate::search::rrf::{reciprocal_rank_fusion, ScoredResult};

/// Layer weights for multi-layer retrieval fusion.
/// These control how much each memory layer contributes to final results.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LayerWeights {
    /// Weight for working memory layer (default 0.3)
    pub working: f32,
    /// Weight for episodic memory layer (default 0.3)
    pub episodic: f32,
    /// Weight for semantic memory layer (default 0.4)
    pub semantic: f32,
}

impl Default for LayerWeights {
    fn default() -> Self {
        Self {
            working: config::DEFAULT_WORKING_WEIGHT,
            episodic: config::DEFAULT_EPISODIC_WEIGHT,
            semantic: config::DEFAULT_SEMANTIC_WEIGHT,
        }
    }
}

impl LayerWeights {
    pub fn new(working: f32, episodic: f32, semantic: f32) -> Self {
        Self {
            working,
            episodic,
            semantic,
        }
    }

    /// Validate that weights sum to approximately 1.0
    pub fn is_valid(&self) -> bool {
        let sum = self.working + self.episodic + self.semantic;
        (sum - 1.0).abs() < config::WEIGHT_SUM_TOLERANCE
    }

    /// Get weight for a specific layer by name
    pub fn weight_for(&self, layer: &str) -> f32 {
        match layer {
            "working" => self.working,
            "episodic" => self.episodic,
            "semantic" => self.semantic,
            _ => 0.0,
        }
    }
}

/// Configuration for adaptive gating
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatingConfig {
    /// Layer weights for fusion
    pub layer_weights: LayerWeights,
    /// Minimum relevance score threshold (0.0-1.0)
    pub relevance_threshold: f32,
    /// RRF k parameter (default 60)
    pub rrf_k: u32,
    /// Maximum results to return
    pub max_results: usize,
    /// Targeted zones for the retrieval
    pub active_zones: Option<Vec<ContextZone>>,
}

impl Default for GatingConfig {
    fn default() -> Self {
        Self {
            layer_weights: LayerWeights::default(),
            relevance_threshold: config::DEFAULT_RELEVANCE_THRESHOLD,
            rrf_k: config::DEFAULT_RRF_K,
            max_results: config::DEFAULT_MAX_RESULTS,
            active_zones: None,
        }
    }
}

/// Result from a single layer's search
#[derive(Debug, Clone)]
pub struct LayerSearchResult {
    pub layer: &'static str,
    pub results: Vec<ScoredResult>,
    pub scores: Vec<f32>,
}

/// Adaptive gating for multi-layer memory retrieval
#[derive(Debug, Clone)]
pub struct AdaptiveGating {
    config: GatingConfig,
}

/// Score a single working memory document
fn score_single_working(
    doc: &MemoryDocument,
    query_lower: &str,
    query_terms: &[&str],
    active_zones: Option<&Vec<ContextZone>>,
) -> Option<ScoredResult> {
    let content_lower = doc.content.to_lowercase();
    let mut score = 0.0_f32;

    // Exact phrase match bonus
    if content_lower.contains(query_lower) {
        score += config::EXACT_PHRASE_MATCH_BONUS;
    }

    // Term frequency scoring
    for term in query_terms {
        if content_lower.contains(term) {
            score += config::TERM_MATCH_BONUS;
            // Additional bonus for multiple occurrences
            let count = content_lower.matches(term).count() as f32;
            score += (count * config::TERM_OCCURRENCE_BONUS)
                .min(config::MAX_TERM_OCCURRENCE_BONUS);
        }
    }

    if score > 0.0 {
        let doc_zone = doc
            .metadata
            .get("zone")
            .and_then(|v| v.as_str())
            .map(ContextZone::parse)
            .unwrap_or(ContextZone::Atomic);

        let mut final_score = score.min(1.0);

        // Apply zone-based boosting
        if let Some(active) = active_zones {
            if active.contains(&doc_zone) {
                final_score *= 1.5; // Boost targeted zones
            } else {
                final_score *= 0.5; // Penalize non-targeted zones
            }
        }

        Some(ScoredResult {
            id: doc.id.clone().unwrap_or_default(),
            content: doc.content.clone(),
            score: final_score,
            source: "working".to_string(),
            path: doc.path.clone(),
            updated_at: doc
                .metadata
                .get("updated_at")
                .and_then(|v| v.as_str())
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.timestamp_millis()),
        })
    } else {
        None
    }
}

/// Score a single episodic memory session
fn score_single_episodic(
    session: &SessionSummary,
    query_lower: &str,
    query_terms: &[&str],
) -> Option<ScoredResult> {
    let summary_lower = session.summary.to_lowercase();
    let mut score = 0.0_f32;

    // Summary match
    if summary_lower.contains(query_lower) {
        score += config::EXACT_PHRASE_MATCH_BONUS;
    }

    // Term frequency in summary
    for term in query_terms {
        if summary_lower.contains(term) {
            score += config::TERM_MATCH_BONUS;
            let count = summary_lower.matches(term).count() as f32;
            score += (count * config::TERM_OCCURRENCE_BONUS)
                .min(config::MAX_TERM_OCCURRENCE_BONUS);
        }
    }

    // Event matching
    for event in &session.key_events {
        let event_lower = event.description.to_lowercase();
        if event_lower.contains(query_lower) {
            score += config::EVENT_PHRASE_MATCH_BONUS;
        }
        for term in query_terms {
            if event_lower.contains(term) {
                score += config::EVENT_TERM_MATCH_BONUS;
            }
        }
    }

    if score > 0.0 {
        Some(ScoredResult {
            id: session.session_id.clone(),
            content: session.summary.clone(),
            score: score.min(1.0),
            source: "episodic".to_string(),
            path: format!("sessions/{}", session.session_id),
            updated_at: Some(session.start_time.timestamp_millis()),
        })
    } else {
        None
    }
}

/// Score a single semantic memory entity
fn score_single_semantic(entity: &EntityRecord, query_lower: &str) -> Option<ScoredResult> {
    let name_lower = entity.name.to_lowercase();
    let normalized_lower = entity.normalized_name.to_lowercase();
    let mut score = 0.0_f32;

    // Exact name match
    if name_lower == query_lower || normalized_lower == query_lower {
        score = config::EXACT_ENTITY_MATCH_SCORE;
    }
    // Partial name match
    else if name_lower.contains(query_lower) || query_lower.contains(&name_lower) {
        score = config::PARTIAL_ENTITY_MATCH_SCORE;
    }
    // Description match
    else if let Some(desc) = &entity.description {
        let desc_lower = desc.to_lowercase();
        if desc_lower.contains(query_lower) {
            score = config::ENTITY_DESCRIPTION_MATCH_SCORE;
        }
        let query_terms: Vec<&str> = query_lower.split_whitespace().collect();
        for term in &query_terms {
            if desc_lower.contains(term) {
                score += config::ENTITY_DESCRIPTION_TERM_BONUS;
            }
        }
    }
    // Alias matching
    else {
        for alias in &entity.aliases {
            if alias.to_lowercase().contains(query_lower) {
                score = config::ENTITY_ALIAS_MATCH_SCORE;
                break;
            }
        }
    }

    // Boost by confirmation count (normalized)
    let final_score = score * config::SEMANTIC_CONFIDENCE_MULTIPLIER;

    if final_score > 0.0 {
        Some(ScoredResult {
            id: entity.id.clone(),
            content: entity.name.clone(),
            score: final_score.min(1.0),
            source: "semantic".to_string(),
            path: format!("entities/{}", entity.id),
            updated_at: Some(entity.last_seen.timestamp_millis()),
        })
    } else {
        None
    }
}

impl AdaptiveGating {
    pub fn new(config: GatingConfig) -> Self {
        Self { config }
    }

    pub fn with_defaults() -> Self {
        Self {
            config: GatingConfig::default(),
        }
    }

    /// Retrieve from all memory layers and fuse results
    pub async fn retrieve(
        &self,
        working: &[MemoryDocument],
        episodic: &[SessionSummary],
        semantic: &[EntityRecord],
        query: &str,
    ) -> Vec<ScoredResult> {
        // 1. Score each layer independently
        let working_results = self.score_working_layer(working, query).await;
        let episodic_results = self.score_episodic_layer(episodic, query).await;
        let semantic_results = self.score_semantic_layer(semantic, query).await;

        // 2. Apply layer weights to scores
        let weighted_working =
            self.apply_weights(working_results, self.config.layer_weights.working);
        let weighted_episodic =
            self.apply_weights(episodic_results, self.config.layer_weights.episodic);
        let weighted_semantic =
            self.apply_weights(semantic_results, self.config.layer_weights.semantic);

        // 3. Fuse with RRF
        let fused = reciprocal_rank_fusion(
            vec![weighted_working, weighted_episodic, weighted_semantic],
            self.config.rrf_k,
        );

        // 4. Filter by threshold and limit results
        fused
            .into_iter()
            .filter(|r| r.score >= self.config.relevance_threshold)
            .take(self.config.max_results)
            .collect()
    }

    /// Retrieve only from working memory
    pub async fn retrieve_working(&self, working: &[MemoryDocument], query: &str) -> Vec<ScoredResult> {
        self.score_working_layer(working, query).await
    }

    /// Retrieve only from episodic memory
    pub async fn retrieve_episodic(&self, episodic: &[SessionSummary], query: &str) -> Vec<ScoredResult> {
        self.score_episodic_layer(episodic, query).await
    }

    /// Retrieve only from semantic memory
    pub async fn retrieve_semantic(&self, semantic: &[EntityRecord], query: &str) -> Vec<ScoredResult> {
        self.score_semantic_layer(semantic, query).await
    }

    /// Score working memory layer using keyword matching
    pub async fn score_working_layer(&self, working: &[MemoryDocument], query: &str) -> Vec<ScoredResult> {
        let query_lower = query.to_lowercase();
        let query_terms_owned: Vec<String> = query_lower.split_whitespace().map(|s| s.to_string()).collect();

        let mut results: Vec<ScoredResult> = if working.len() > 100 {
            let working = working.to_vec();
            let active_zones = self.config.active_zones.clone();
            tokio::task::spawn_blocking(move || {
                let query_terms: Vec<&str> = query_terms_owned.iter().map(|s| s.as_str()).collect();
                working
                    .par_iter()
                    .filter_map(|doc| score_single_working(doc, &query_lower, &query_terms, active_zones.as_ref()))
                    .collect()
            })
            .await
            .unwrap_or_default()
        } else {
            let query_terms: Vec<&str> = query_terms_owned.iter().map(|s| s.as_str()).collect();
            working
                .iter()
                .filter_map(|doc| score_single_working(doc, &query_lower, &query_terms, self.config.active_zones.as_ref()))
                .collect()
        };

        // Sort by score descending
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results
    }

    /// Score episodic memory layer using summary and event matching
    pub async fn score_episodic_layer(&self, episodic: &[SessionSummary], query: &str) -> Vec<ScoredResult> {
        let query_lower = query.to_lowercase();
        let query_terms_owned: Vec<String> = query_lower.split_whitespace().map(|s| s.to_string()).collect();

        let mut results: Vec<ScoredResult> = if episodic.len() > 100 {
            let episodic = episodic.to_vec();
            tokio::task::spawn_blocking(move || {
                let query_terms: Vec<&str> = query_terms_owned.iter().map(|s| s.as_str()).collect();
                episodic
                    .par_iter()
                    .filter_map(|session| score_single_episodic(session, &query_lower, &query_terms))
                    .collect()
            })
            .await
            .unwrap_or_default()
        } else {
            let query_terms: Vec<&str> = query_terms_owned.iter().map(|s| s.as_str()).collect();
            episodic
                .iter()
                .filter_map(|session| score_single_episodic(session, &query_lower, &query_terms))
                .collect()
        };

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results
    }

    /// Score semantic memory layer using entity matching
    pub async fn score_semantic_layer(&self, semantic: &[EntityRecord], query: &str) -> Vec<ScoredResult> {
        let query_lower = query.to_lowercase();

        let mut results: Vec<ScoredResult> = if semantic.len() > 100 {
            let semantic = semantic.to_vec();
            tokio::task::spawn_blocking(move || {
                semantic
                    .par_iter()
                    .filter_map(|entity| score_single_semantic(entity, &query_lower))
                    .collect()
            })
            .await
            .unwrap_or_default()
        } else {
            semantic
                .iter()
                .filter_map(|entity| score_single_semantic(entity, &query_lower))
                .collect()
        };

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results
    }

    /// Apply layer weight to all scores in a result set
    fn apply_weights(&self, results: Vec<ScoredResult>, weight: f32) -> Vec<ScoredResult> {
        results
            .into_iter()
            .map(|mut r| {
                r.score *= weight;
                r
            })
            .collect()
    }

    /// Get configuration reference
    pub fn config(&self) -> &GatingConfig {
        &self.config
    }

    /// Update layer weights
    pub fn set_weights(&mut self, weights: LayerWeights) {
        self.config.layer_weights = weights;
    }

    /// Update relevance threshold
    pub fn set_threshold(&mut self, threshold: f32) {
        self.config.relevance_threshold = threshold.clamp(0.0, 1.0);
    }
}

/// Session summary for episodic memory layer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub summary: String,
    pub key_events: Vec<Event>,
    #[serde(default)]
    pub sentiment_timeline: Vec<f32>,
}

/// Key event within a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub description: String,
    pub event_type: String,
}

/// Layer statistics for monitoring
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LayerStats {
    pub working_count: usize,
    pub episodic_count: usize,
    pub semantic_count: usize,
    pub last_retrieval_layer_weights: LayerWeights,
    pub total_queries: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer_weights_default() {
        let weights = LayerWeights::default();
        assert!((weights.working - 0.3).abs() < 0.001);
        assert!((weights.episodic - 0.3).abs() < 0.001);
        assert!((weights.semantic - 0.4).abs() < 0.001);
        assert!(weights.is_valid());
    }

    #[test]
    fn test_layer_weights_custom() {
        let weights = LayerWeights::new(0.2, 0.3, 0.5);
        assert!((weights.working - 0.2).abs() < 0.001);
        assert!((weights.semantic - 0.5).abs() < 0.001);
        assert!(weights.is_valid());
    }

    #[test]
    fn test_weight_for_layer() {
        let weights = LayerWeights::default();
        assert!((weights.weight_for("working") - 0.3).abs() < 0.001);
        assert!((weights.weight_for("episodic") - 0.3).abs() < 0.001);
        assert!((weights.weight_for("semantic") - 0.4).abs() < 0.001);
        assert!((weights.weight_for("unknown")).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_working_layer_scoring() {
        let gating = AdaptiveGating::with_defaults();
        let docs = vec![
            MemoryDocument {
                id: Some("doc1".to_string()),
                path: "test/path1".to_string(),
                content: "BELA works at SWAL".to_string(),
                metadata: serde_json::json!({}),
                content_vector: None,
                embedding: vec![],
                ..Default::default()
            },
            MemoryDocument {
                id: Some("doc2".to_string()),
                path: "test/path2".to_string(),
                content: "Something else entirely".to_string(),
                metadata: serde_json::json!({}),
                content_vector: None,
                embedding: vec![],
                ..Default::default()
            },
        ];

        let results = gating.score_working_layer(&docs, "BELA").await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "doc1");
        assert!(results[0].score > 0.0);
    }

    #[tokio::test]
    async fn test_semantic_layer_scoring() {
        let gating = AdaptiveGating::with_defaults();
        let entities = vec![
            EntityRecord {
                id: "entity1".to_string(),
                name: "BELA".to_string(),
                normalized_name: "bela".to_string(),
                entity_type: crate::memory::entity_graph::EntityType::Person,
                aliases: vec![],
                description: Some("Developer at SWAL".to_string()),
                occurrence_count: 5,
                memory_count: 3,
                first_seen: chrono::Utc::now(),
                last_seen: chrono::Utc::now(),
                merged_from: vec![],
                trust_score: 0.5,
                trust_rank: 1,
            },
            EntityRecord {
                id: "entity2".to_string(),
                name: "SWAL".to_string(),
                normalized_name: "swal".to_string(),
                entity_type: crate::memory::entity_graph::EntityType::Organization,
                aliases: vec![],
                description: Some("Southwest AI Labs".to_string()),
                occurrence_count: 10,
                memory_count: 5,
                first_seen: chrono::Utc::now(),
                last_seen: chrono::Utc::now(),
                merged_from: vec![],
                trust_score: 0.5,
                trust_rank: 1,
            },
        ];

        let results = gating.score_semantic_layer(&entities, "BELA").await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "entity1");
        // Exact matches are scaled by the fixed semantic confidence multiplier.
        assert_eq!(results[0].score, 0.5);
    }

    #[tokio::test]
    async fn test_multi_layer_retrieval() {
        let mut gating = AdaptiveGating::with_defaults();
        gating.set_threshold(0.0);
        let docs = vec![MemoryDocument {
            id: Some("doc1".to_string()),
            path: "test".to_string(),
            content: "BELA works at SWAL".to_string(),
            metadata: serde_json::json!({}),
            content_vector: None,
            embedding: vec![],
            ..Default::default()
        }];
        let sessions: Vec<SessionSummary> = vec![];
        let entities: Vec<EntityRecord> = vec![];

        let results = gating.retrieve(&docs, &sessions, &entities, "BELA").await;
        assert!(!results.is_empty());
        assert_eq!(results[0].source, "hybrid");
    }

    #[tokio::test]
    async fn test_score_documents_parallel_equals_sequential() {
        let gating = AdaptiveGating::with_defaults();
        let mut docs = Vec::new();
        for i in 0..150 {
            docs.push(MemoryDocument {
                id: Some(format!("doc{}", i)),
                path: format!("test/path{}", i),
                content: format!("BELA works at SWAL in office {}", i),
                metadata: serde_json::json!({}),
                ..Default::default()
            });
        }

        // Test working layer
        let sequential_results: Vec<ScoredResult> = docs
            .iter()
            .filter_map(|doc| {
                score_single_working(
                    doc,
                    "bela",
                    &["bela"],
                    None,
                )
            })
            .collect();

        let parallel_results = gating.score_working_layer(&docs, "bela").await;

        assert_eq!(sequential_results.len(), parallel_results.len());
        for (s, p) in sequential_results.iter().zip(parallel_results.iter()) {
            assert_eq!(s.id, p.id);
            assert!((s.score - p.score).abs() < 0.0001);
        }
    }
}
