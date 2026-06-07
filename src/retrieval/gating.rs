//! Adaptive Retrieval Gating - Multi-layer memory retrieval with weighted fusion
//!
//! Implements adaptive gating that scores and fuses results from Working, Episodic,
//! and Semantic memory layers using RRF (Reciprocal Rank Fusion).

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use super::scoring::*;
use crate::context::ContextLevel;
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
    /// Adjust weights dynamically based on query characteristics
    pub fn adaptive(
        query: &str,
        context_level: ContextLevel,
        _active_zones: &[ContextZone],
    ) -> Self {
        let factual_score = query_factuality_score(query);
        let procedural_score = query_procedural_score(query);

        if factual_score > 0.7 {
            // Factual queries -> more weight to semantic/long-term
            Self::new(0.2, 0.2, 0.6)
        } else if procedural_score > 0.7 {
            // Procedural queries -> more weight to episodic/sessions
            Self::new(0.2, 0.6, 0.2)
        } else if context_level == ContextLevel::Minimal {
            // Immediate/minimal context -> more weight to working/recent
            Self::new(0.6, 0.3, 0.1)
        } else {
            Self::default()
        }
    }

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
    /// Multiplier for targeted zones (default 1.5)
    pub zone_boost_multiplier: f32,
    /// Multiplier for non-targeted zones (default 0.5)
    pub zone_penalty_multiplier: f32,
    /// Weight for recency boosting (0.0 to 1.0)
    pub recency_weight: f32,
    /// Half-life in hours for recency decay
    pub half_life_hours: f32,
    /// Whether to enable belief graph grounding validation
    pub grounding_enabled: bool,
    /// Minimum confidence for semantic grounding
    pub grounding_min_confidence: f32,
}

impl Default for GatingConfig {
    fn default() -> Self {
        Self {
            layer_weights: LayerWeights::default(),
            relevance_threshold: config::DEFAULT_RELEVANCE_THRESHOLD,
            rrf_k: config::DEFAULT_RRF_K,
            max_results: config::DEFAULT_MAX_RESULTS,
            active_zones: None,
            zone_boost_multiplier: config::configured_zone_boost(),
            zone_penalty_multiplier: config::configured_zone_penalty(),
            recency_weight: config::DEFAULT_RECENCY_WEIGHT,
            half_life_hours: config::DEFAULT_HALF_LIFE_HOURS,
            grounding_enabled: true,
            grounding_min_confidence: 0.5,
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

/// Result from a multi-layer search (for context pack export)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayeredSearchResult {
    pub topic: String,
    pub timestamp: String,
    pub level_0_working: Vec<ScoredResult>,
    pub level_1_entity_graph: Vec<ScoredResult>,
    pub level_2_semantic: Vec<ScoredResult>,
    pub level_3_episodic: Vec<ScoredResult>,
}

/// Adaptive gating for multi-layer memory retrieval
#[derive(Debug, Clone)]
pub struct AdaptiveGating {
    config: GatingConfig,
}

// ---------------------------------------------------------------------------
// Helper functions — used in both parallel and sequential scoring paths
// ---------------------------------------------------------------------------

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
        belief_graph: Option<crate::memory::belief_graph::SharedBeliefGraph>,
    ) -> Vec<ScoredResult> {
        let now = chrono::Utc::now();
        // 1. Score each layer independently (may use parallel execution internally)
        let working_results = self.score_working_layer_at(working, query, now).await;
        let episodic_results = self.score_episodic_layer_at(episodic, query, now).await;
        let semantic_results = self.score_semantic_layer_at(semantic, query, now).await;

        // 2. Apply layer weights to scores
        let weighted_working =
            self.apply_weights(working_results, self.config.layer_weights.working);
        let weighted_episodic =
            self.apply_weights(episodic_results, self.config.layer_weights.episodic);
        let weighted_semantic =
            self.apply_weights(semantic_results, self.config.layer_weights.semantic);

        // 3. Fuse with RRF
        let mut fused = reciprocal_rank_fusion(
            vec![weighted_working, weighted_episodic, weighted_semantic],
            self.config.rrf_k,
        );

        // 4. Optional Reranking for precision boost
        if let Some(hook) = crate::search::rerank::RerankHook::from_env() {
            let rerank_limit = crate::retrieval::config::DEFAULT_RERANK_LIMIT;
            if fused.len() > rerank_limit {
                fused.truncate(rerank_limit);
            }

            let _ = crate::search::hooks::SearchHook::post_query(&hook, query, &mut fused).await;
        }

        // 5. Filter by threshold
        let mut results: Vec<ScoredResult> = fused
            .into_iter()
            .filter(|r| r.score >= self.config.relevance_threshold)
            .collect();

        // 5. Apply grounding validation if enabled
        if self.config.grounding_enabled {
            if let Some(graph_lock) = belief_graph {
                let graph = graph_lock.read().await;
                // Convert ScoredResult back to MemoryDocument for validate_grounding
                // Note: We only have content/id, so we build partial documents
                let docs_to_validate: Vec<MemoryDocument> = results
                    .iter()
                    .map(|r| MemoryDocument {
                        id: Some(r.id.clone()),
                        content: r.content.clone(),
                        path: r.path.clone(),
                        ..Default::default()
                    })
                    .collect();

                let grounding = graph
                    .validate_grounding(&docs_to_validate, self.config.grounding_min_confidence)
                    .await;

                results.retain(|r| {
                    grounding
                        .iter()
                        .find(|(id, _, _)| id == &r.id)
                        .map(|(_, grounded, _)| *grounded)
                        .unwrap_or(false)
                });
            }
        }

        // 6. Limit results
        results.into_iter().take(self.config.max_results).collect()
    }

    /// Retrieve only from working memory
    pub async fn retrieve_working(
        &self,
        working: &[MemoryDocument],
        query: &str,
    ) -> Vec<ScoredResult> {
        self.score_working_layer_at(working, query, chrono::Utc::now())
            .await
    }

    /// Retrieve only from episodic memory
    pub async fn retrieve_episodic(
        &self,
        episodic: &[SessionSummary],
        query: &str,
    ) -> Vec<ScoredResult> {
        self.score_episodic_layer_at(episodic, query, chrono::Utc::now())
            .await
    }

    /// Retrieve only from semantic memory
    pub async fn retrieve_semantic(
        &self,
        semantic: &[EntityRecord],
        query: &str,
    ) -> Vec<ScoredResult> {
        self.score_semantic_layer_at(semantic, query, chrono::Utc::now())
            .await
    }

    /// Score working memory layer using keyword matching (with parallelism for large sets).
    pub async fn score_working_layer_at(
        &self,
        working: &[MemoryDocument],
        query: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Vec<ScoredResult> {
        let query_lower = query.to_lowercase();
        let query_terms_owned: Vec<String> = query_lower
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        let mut results: Vec<ScoredResult> = if working.len() > 100 {
            let working = working.to_vec();
            let active_zones = self.config.active_zones.clone();
            let zone_boost = self.config.zone_boost_multiplier;
            let zone_penalty = self.config.zone_penalty_multiplier;
            let recency_weight = self.config.recency_weight;
            let half_life = self.config.half_life_hours;

            tokio::task::spawn_blocking(move || {
                let query_terms: Vec<&str> = query_terms_owned.iter().map(|s| s.as_str()).collect();
                working
                    .par_iter()
                    .filter_map(|doc| {
                        score_single_working(
                            doc,
                            &WorkingScoringParams {
                                query_lower: &query_lower,
                                query_terms: &query_terms,
                                active_zones: active_zones.as_ref(),
                                zone_boost_multiplier: zone_boost,
                                zone_penalty_multiplier: zone_penalty,
                                now,
                                recency_weight,
                                half_life_hours: half_life,
                            },
                        )
                    })
                    .collect()
            })
            .await
            .unwrap_or_default()
        } else {
            let query_terms: Vec<&str> = query_terms_owned.iter().map(|s| s.as_str()).collect();
            working
                .iter()
                .filter_map(|doc| {
                    score_single_working(
                        doc,
                        &WorkingScoringParams {
                            query_lower: &query_lower,
                            query_terms: &query_terms,
                            active_zones: self.config.active_zones.as_ref(),
                            zone_boost_multiplier: self.config.zone_boost_multiplier,
                            zone_penalty_multiplier: self.config.zone_penalty_multiplier,
                            now,
                            recency_weight: self.config.recency_weight,
                            half_life_hours: self.config.half_life_hours,
                        },
                    )
                })
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

    /// Score episodic memory layer using summary and event matching (with parallelism for large sets).
    pub async fn score_episodic_layer_at(
        &self,
        episodic: &[SessionSummary],
        query: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Vec<ScoredResult> {
        let query_lower = query.to_lowercase();
        let query_terms_owned: Vec<String> = query_lower
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        let mut results: Vec<ScoredResult> = if episodic.len() > 100 {
            let episodic = episodic.to_vec();
            let recency_weight = self.config.recency_weight;
            let half_life = self.config.half_life_hours;

            tokio::task::spawn_blocking(move || {
                let query_terms: Vec<&str> = query_terms_owned.iter().map(|s| s.as_str()).collect();
                episodic
                    .par_iter()
                    .filter_map(|session| {
                        score_single_episodic(
                            session,
                            &query_lower,
                            &query_terms,
                            now,
                            recency_weight,
                            half_life,
                        )
                    })
                    .collect()
            })
            .await
            .unwrap_or_default()
        } else {
            let query_terms: Vec<&str> = query_terms_owned.iter().map(|s| s.as_str()).collect();
            episodic
                .iter()
                .filter_map(|session| {
                    score_single_episodic(
                        session,
                        &query_lower,
                        &query_terms,
                        now,
                        self.config.recency_weight,
                        self.config.half_life_hours,
                    )
                })
                .collect()
        };

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results
    }

    /// Score semantic memory layer using entity matching (with parallelism for large sets).
    pub async fn score_semantic_layer_at(
        &self,
        semantic: &[EntityRecord],
        query: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Vec<ScoredResult> {
        let query_lower = query.to_lowercase();

        let mut results: Vec<ScoredResult> = if semantic.len() > 100 {
            let semantic = semantic.to_vec();
            let recency_weight = self.config.recency_weight;
            let half_life = self.config.half_life_hours;

            tokio::task::spawn_blocking(move || {
                semantic
                    .par_iter()
                    .filter_map(|entity| {
                        score_single_semantic(entity, &query_lower, now, recency_weight, half_life)
                    })
                    .collect()
            })
            .await
            .unwrap_or_default()
        } else {
            semantic
                .iter()
                .filter_map(|entity| {
                    score_single_semantic(
                        entity,
                        &query_lower,
                        now,
                        self.config.recency_weight,
                        self.config.half_life_hours,
                    )
                })
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

    /// Perform multi-layer retrieval and return a LayeredSearchResult (for context pack export)
    pub async fn retrieve_layered(
        &self,
        all_docs: &[MemoryDocument],
        episodic: &[SessionSummary],
        semantic: &[EntityRecord],
        query: &str,
    ) -> LayeredSearchResult {
        let now = chrono::Utc::now();
        // Level 0: Working Memory (Filtered for non-belief documents)
        let working_docs: Vec<MemoryDocument> = all_docs
            .iter()
            .filter(|d| d.level != crate::memory::schema::MemoryLevel::Belief)
            .cloned()
            .collect();
        let level_0_results = self.score_working_layer_at(&working_docs, query, now).await;

        // Level 1: Entity Graph
        let level_1_results = self.score_semantic_layer_at(semantic, query, now).await;

        // Level 2: Semantic (Rules, Definitions) -> Documents with MemoryLevel::Belief
        let level_2_results = self.score_belief_layer(all_docs, query);

        // Level 3: Episodic (History, snippets)
        let level_3_results = self.score_episodic_layer_at(episodic, query, now).await;

        LayeredSearchResult {
            topic: query.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            level_0_working: level_0_results,
            level_1_entity_graph: level_1_results,
            level_2_semantic: level_2_results,
            level_3_episodic: level_3_results,
        }
    }

    /// Score belief layer (Level 2) using keyword matching on Belief-level documents
    fn score_belief_layer(&self, documents: &[MemoryDocument], query: &str) -> Vec<ScoredResult> {
        let beliefs: Vec<MemoryDocument> = documents
            .iter()
            .filter(|d| d.level == crate::memory::schema::MemoryLevel::Belief)
            .cloned()
            .collect();
        self.score_document_layer(&beliefs, query, "semantic_belief")
    }

    fn score_document_layer(
        &self,
        documents: &[MemoryDocument],
        query: &str,
        source: &str,
    ) -> Vec<ScoredResult> {
        let query_lower = query.to_lowercase();
        let query_terms_owned: Vec<String> = query_lower
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        let mut results: Vec<ScoredResult> = documents
            .iter()
            .filter_map(|doc| {
                let content_lower = doc.content.to_lowercase();
                let mut score = 0.0_f32;

                if content_lower.contains(&query_lower) {
                    score += config::EXACT_PHRASE_MATCH_BONUS;
                }

                for term in &query_terms_owned {
                    if content_lower.contains(term) {
                        score += config::TERM_MATCH_BONUS;
                        let count = content_lower.matches(term).count() as f32;
                        score += (count * config::TERM_OCCURRENCE_BONUS)
                            .min(config::MAX_TERM_OCCURRENCE_BONUS);
                    }
                }

                if score > 0.0 {
                    Some(ScoredResult {
                        id: doc.id.clone().unwrap_or_default(),
                        content: doc.content.clone(),
                        score: score.min(1.0),
                        source: source.to_string(),
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
            })
            .collect();

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(self.config.max_results);
        results
    }
}

/// Score how "factual" a query is (e.g., "what is", "how does it work")
fn query_factuality_score(query: &str) -> f32 {
    let query = query.to_lowercase();
    let keywords = [
        "what is",
        "who is",
        "define",
        "explain",
        "meaning",
        "how does",
        "what are",
        "list of",
        "fact",
        "describe",
        "qué es",
        "quién es",
        "cómo funciona",
        "qué son",
        "definir",
    ];

    let mut score = 0.0_f32;
    for &kw in &keywords {
        if query.contains(kw) {
            score += 0.4;
        }
    }
    score.min(1.0)
}

/// Score how "procedural" a query is (e.g., "how did we do", "steps for")
fn query_procedural_score(query: &str) -> f32 {
    let query = query.to_lowercase();
    let keywords = [
        "how to",
        "steps",
        "procedure",
        "process",
        "guide",
        "how did we",
        "instructions",
        "workflow",
        "method",
        "cómo hicimos",
        "pasos",
        "procedimiento",
        "instrucciones",
    ];

    let mut score = 0.0_f32;
    for &kw in &keywords {
        if query.contains(kw) {
            score += 0.4;
        }
    }
    score.min(1.0)
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

        let results = gating
            .score_working_layer_at(&docs, "BELA", chrono::Utc::now())
            .await;
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

        let results = gating
            .score_semantic_layer_at(&entities, "BELA", chrono::Utc::now())
            .await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "entity1");
        // Exact matches (0.5) are boosted by recency (1.3) = 0.65
        assert_eq!(results[0].score, 0.65);
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

        let results = gating
            .retrieve(&docs, &sessions, &entities, "BELA", None)
            .await;
        assert!(!results.is_empty());
        assert_eq!(results[0].source, "hybrid");
    }

    #[tokio::test]
    async fn test_zone_multipliers() {
        let config = GatingConfig {
            zone_boost_multiplier: 2.0,
            zone_penalty_multiplier: 0.1,
            active_zones: Some(vec![ContextZone::Global]),
            ..Default::default()
        };

        let gating = AdaptiveGating::new(config);

        let docs = vec![
            MemoryDocument {
                id: Some("doc1".to_string()),
                path: "test1".to_string(),
                content: "search term".to_string(),
                metadata: serde_json::json!({"zone": "global"}),
                ..Default::default()
            },
            MemoryDocument {
                id: Some("doc2".to_string()),
                path: "test2".to_string(),
                content: "search term".to_string(),
                metadata: serde_json::json!({"zone": "atomic"}),
                ..Default::default()
            },
        ];

        let results = gating
            .score_working_layer_at(&docs, "search term", chrono::Utc::now())
            .await;

        assert_eq!(results.len(), 2);

        let res1 = results.iter().find(|r| r.id == "doc1").unwrap();
        let res2 = results.iter().find(|r| r.id == "doc2").unwrap();

        // Base score for "search term" (two terms)
        // EXACT_PHRASE_MATCH_BONUS = 0.5
        // TERM_MATCH_BONUS = 0.1 (x2)
        // TERM_OCCURRENCE_BONUS = 0.05 (x2)
        // Total base score = 0.5 + 0.2 + 0.1 = 0.8
        // doc1 (boost 2.0): 0.8 * 2.0 = 1.6
        // doc2 (penalty 0.1): 0.8 * 0.1 = 0.08
        assert!((res1.score - 1.6).abs() < 0.001);
        assert!((res2.score - 0.08).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_recency_bias() {
        let gating = AdaptiveGating::with_defaults();
        let now = chrono::Utc::now();
        let yesterday = now - chrono::Duration::days(1);
        let last_month = now - chrono::Duration::days(30);

        let docs = vec![
            MemoryDocument {
                id: Some("recent".to_string()),
                path: "test/recent".to_string(),
                content: "BELA works at SWAL".to_string(),
                metadata: serde_json::json!({
                    "updated_at": yesterday.to_rfc3339()
                }),
                ..Default::default()
            },
            MemoryDocument {
                id: Some("old".to_string()),
                path: "test/old".to_string(),
                content: "BELA works at SWAL".to_string(),
                metadata: serde_json::json!({
                    "updated_at": last_month.to_rfc3339()
                }),
                ..Default::default()
            },
        ];

        let results = gating.score_working_layer_at(&docs, "BELA", now).await;
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "recent");
        assert!(results[0].score > results[1].score);
    }

    #[test]
    fn test_query_factuality_score() {
        assert!(query_factuality_score("What is the capital of France?") >= 0.4);
        assert!(query_factuality_score("How does photosynthesis work?") >= 0.4);
        assert_eq!(query_factuality_score("Hello world"), 0.0);
    }

    #[test]
    fn test_query_procedural_score() {
        assert!(query_procedural_score("How to bake a cake") >= 0.4);
        assert!(query_procedural_score("Steps for deploying a server") >= 0.4);
        assert_eq!(query_procedural_score("What is a server"), 0.0);
    }

    #[test]
    fn test_layer_weights_adaptive() {
        // Factual query
        let weights =
            LayerWeights::adaptive("What is the meaning of life?", ContextLevel::Medium, &[]);
        assert!(weights.semantic > weights.working);
        assert!(weights.semantic > weights.episodic);

        // Procedural query
        let weights = LayerWeights::adaptive(
            "How did we implement the auth system? Give me the steps.",
            ContextLevel::Medium,
            &[],
        );
        assert!(weights.episodic > weights.working);
        assert!(weights.episodic > weights.semantic);

        // Minimal context query
        let weights = LayerWeights::adaptive("status", ContextLevel::Minimal, &[]);
        assert!(weights.working > weights.episodic);
        assert!(weights.working > weights.semantic);

        // Default case
        let weights = LayerWeights::adaptive("Hello", ContextLevel::Medium, &[]);
        let default = LayerWeights::default();
        assert!((weights.working - default.working).abs() < 0.001);
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

        // Sequential scoring via helper
        let sequential_results: Vec<ScoredResult> = docs
            .iter()
            .filter_map(|doc| {
                score_single_working(
                    doc,
                    &WorkingScoringParams {
                        query_lower: "bela",
                        query_terms: &["bela"],
                        active_zones: None,
                        zone_boost_multiplier: 1.5,
                        zone_penalty_multiplier: 0.5,
                        now: chrono::Utc::now(),
                        recency_weight: 0.3,
                        half_life_hours: 168.0,
                    },
                )
            })
            .collect();

        let parallel_results = gating
            .score_working_layer_at(&docs, "bela", chrono::Utc::now())
            .await;

        assert_eq!(sequential_results.len(), parallel_results.len());
        for (s, p) in sequential_results.iter().zip(parallel_results.iter()) {
            assert_eq!(s.id, p.id);
            assert!((s.score - p.score).abs() < 0.0001);
        }
    }
}
