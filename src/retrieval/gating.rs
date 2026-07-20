//! Adaptive Retrieval Gating - Multi-layer memory retrieval with weighted fusion
//!
//! Implements adaptive gating that scores and fuses results from Working, Episodic,
//! and Semantic memory layers using RRF (Reciprocal Rank Fusion).

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::policy::NavigationPolicy;
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
    /// Navigation policy for intelligent graph traversal
    pub navigation_policy: Option<NavigationPolicy>,
    /// Whether to enable predictive cache warming
    pub cache_warming_enabled: bool,
    /// Threshold for cache warming (0.0 to 1.0)
    pub cache_warming_threshold: f32,
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
            navigation_policy: Some(NavigationPolicy::with_defaults()),
            cache_warming_enabled: false,
            cache_warming_threshold: config::DEFAULT_CACHE_WARMING_THRESHOLD,
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

/// Adaptive zone booster that adjusts zone multipliers dynamically
/// based on per-user/session feedback.
///
/// Tracks which zones produce better results for each user and
/// adjusts boost/penalty multipliers accordingly.
#[derive(Debug, Clone)]
pub struct AdaptiveZoneBooster {
    /// Per-user/zone performance scores
    user_zone_scores: Arc<Mutex<HashMap<String, HashMap<String, ZonePerformance>>>>,
    /// Base boost multiplier (starting point before adaptation)
    base_boost: f32,
    /// Base penalty multiplier
    base_penalty: f32,
    /// Learning rate for score updates
    learning_rate: f32,
}

/// Performance tracking for a single zone per user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZonePerformance {
    /// Number of queries where this zone was relevant
    pub hits: u64,
    /// Total queries where this zone was considered
    pub total: u64,
    /// Average score of results from this zone
    pub avg_score: f32,
}

impl Default for AdaptiveZoneBooster {
    fn default() -> Self {
        Self {
            user_zone_scores: Arc::new(Mutex::new(HashMap::new())),
            base_boost: config::DEFAULT_ZONE_BOOST_MULTIPLIER,
            base_penalty: config::DEFAULT_ZONE_PENALTY_MULTIPLIER,
            learning_rate: 0.1,
        }
    }
}

impl AdaptiveZoneBooster {
    /// Creates a new adaptive zone booster with default parameters.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records feedback for a user's zone: whether results were relevant and the avg score.
    pub async fn record_feedback(&self, user_id: &str, zone: &str, was_relevant: bool, score: f32) {
        let mut user_zones = self.user_zone_scores.lock().await;
        let entry = user_zones
            .entry(user_id.to_string())
            .or_default()
            .entry(zone.to_string())
            .or_insert(ZonePerformance {
                hits: 0,
                total: 0,
                avg_score: 0.0,
            });

        entry.total += 1;
        if was_relevant {
            entry.hits += 1;
        }
        // Running average
        let n = entry.total as f32;
        entry.avg_score = entry.avg_score * ((n - 1.0) / n) + score * (1.0 / n);
    }

    /// Gets the adaptive boost multiplier for a specific user and zone.
    /// Zones with high hit rates get boosted more; low-rate zones get penalized.
    pub async fn get_boost(&self, user_id: &str, zone: &str) -> f32 {
        let user_zones = self.user_zone_scores.lock().await;
        if let Some(zones) = user_zones.get(user_id) {
            if let Some(perf) = zones.get(zone) {
                if perf.total > 5 {
                    // Enough data to adapt
                    let hit_rate = perf.hits as f32 / perf.total as f32;
                    // Scale: hit_rate 1.0 → max boost, 0.5 → base, 0.0 → min
                    let factor = 1.0 + (hit_rate - 0.5) * self.learning_rate * 10.0;
                    return (self.base_boost * factor).clamp(0.1, 5.0);
                }
            }
        }
        self.base_boost
    }

    /// Gets the adaptive penalty multiplier for a specific user and zone.
    pub async fn get_penalty(&self, user_id: &str, zone: &str) -> f32 {
        let user_zones = self.user_zone_scores.lock().await;
        if let Some(zones) = user_zones.get(user_id) {
            if let Some(perf) = zones.get(zone) {
                if perf.total > 5 {
                    let hit_rate = perf.hits as f32 / perf.total as f32;
                    // Inverse: low hit rate → higher penalty
                    let factor = 1.0 - (hit_rate - 0.5) * self.learning_rate * 5.0;
                    return (self.base_penalty * factor).clamp(0.1, 2.0);
                }
            }
        }
        self.base_penalty
    }

    /// Returns the hit rate for a user/zone combination (0.0 to 1.0).
    pub async fn hit_rate(&self, user_id: &str, zone: &str) -> f32 {
        let user_zones = self.user_zone_scores.lock().await;
        if let Some(zones) = user_zones.get(user_id) {
            if let Some(perf) = zones.get(zone) {
                if perf.total > 0 {
                    return perf.hits as f32 / perf.total as f32;
                }
            }
        }
        0.0
    }

    /// Returns the mean hit rate across ALL tracked user/zone combinations (0.0–1.0).
    ///
    /// Used by the auto-improvement loop as a cache-effectiveness proxy: when the
    /// booster has accumulated feedback, this reflects how often the adaptive
    /// weighting served relevant results. Returns 0.0 when no data is recorded.
    pub async fn average_hit_rate(&self) -> f32 {
        let user_zones = self.user_zone_scores.lock().await;
        let mut total_hits = 0u64;
        let mut total_queries = 0u64;
        for zones in user_zones.values() {
            for perf in zones.values() {
                total_hits += perf.hits;
                total_queries += perf.total;
            }
        }
        if total_queries == 0 {
            0.0
        } else {
            total_hits as f32 / total_queries as f32
        }
    }

    /// Resets data for all users.
    pub async fn reset_all(&self) {
        self.user_zone_scores.lock().await.clear();
    }

    /// Resets data for a specific user.
    pub async fn reset_user(&self, user_id: &str) {
        let mut user_zones = self.user_zone_scores.lock().await;
        user_zones.remove(user_id);
    }
}

use tokio::sync::RwLock;

/// Adaptive gating for multi-layer memory retrieval
#[derive(Debug, Clone)]
pub struct AdaptiveGating {
    config: GatingConfig,
    policy: Option<Arc<RwLock<super::policy::NavigationPolicy>>>,
    memory: Option<Arc<crate::memory::qmd_memory::QmdMemory>>,
    zone_booster: Option<Arc<AdaptiveZoneBooster>>,
}

// ---------------------------------------------------------------------------
// Helper functions — used in both parallel and sequential scoring paths
// ---------------------------------------------------------------------------

impl AdaptiveGating {
    pub fn new(config: GatingConfig) -> Self {
        Self {
            config,
            policy: None,
            memory: None,
            zone_booster: None,
        }
    }

    pub fn with_policy(
        config: GatingConfig,
        policy: Arc<RwLock<super::policy::NavigationPolicy>>,
    ) -> Self {
        Self {
            config,
            policy: Some(policy),
            memory: None,
            zone_booster: None,
        }
    }

    pub fn with_memory(mut self, memory: Arc<crate::memory::qmd_memory::QmdMemory>) -> Self {
        self.memory = Some(memory);
        self
    }

    pub fn with_booster(mut self, booster: Arc<AdaptiveZoneBooster>) -> Self {
        self.zone_booster = Some(booster);
        self
    }

    pub fn with_defaults() -> Self {
        let settings = crate::settings::XavierSettings::current();
        let policy = super::policy::NavigationPolicy::new(
            LayerWeights::new(
                settings.retrieval.learned_policy.working_weight,
                settings.retrieval.learned_policy.episodic_weight,
                settings.retrieval.learned_policy.semantic_weight,
            ),
            super::policy::TraversalWeights {
                semantic_similarity: settings.retrieval.learned_policy.semantic_similarity_weight,
                confidence: settings.retrieval.learned_policy.confidence_weight,
                edge_weight: settings.retrieval.learned_policy.edge_weight,
                recency: settings.retrieval.learned_policy.recency_weight,
                cross_layer: settings.retrieval.learned_policy.cross_layer_weight,
                cross_dir: settings.retrieval.learned_policy.cross_dir_weight,
                peripheral_hub: settings.retrieval.learned_policy.peripheral_hub_weight,
            },
            settings.retrieval.learned_policy.learning_rate,
        );

        let mut config = GatingConfig {
            cache_warming_enabled: settings.retrieval.cache_warming_enabled,
            ..Default::default()
        };
        if let Some(threshold) = settings.retrieval.cache_warming_threshold {
            config.cache_warming_threshold = threshold;
        }

        Self {
            config,
            policy: Some(Arc::new(RwLock::new(policy))),
            memory: None,
            zone_booster: None,
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
        self.retrieve_with_telemetry(working, episodic, semantic, query, belief_graph, None, None)
            .await
    }

    /// Retrieve from all memory layers with telemetry recording
    #[allow(clippy::too_many_arguments)]
    pub async fn retrieve_with_telemetry(
        &self,
        working: &[MemoryDocument],
        episodic: &[SessionSummary],
        semantic: &[EntityRecord],
        query: &str,
        belief_graph: Option<crate::memory::belief_graph::SharedBeliefGraph>,
        telemetry: Option<Arc<crate::memory::telemetry::NavTelemetry>>,
        user_id: Option<String>,
    ) -> Vec<ScoredResult> {
        let now = chrono::Utc::now();
        let start_time = std::time::Instant::now();

        // 1. Score each layer independently (may use parallel execution internally)
        let working_results = self
            .score_working_layer_at(working, query, now, user_id.as_deref())
            .await;
        let episodic_results = self.score_episodic_layer_at(episodic, query, now).await;
        let mut semantic_results = self.score_semantic_layer_at(semantic, query, now).await;

        // 1.1 Guided graph expansion (if graph and policy available)
        let expansion_policy = if let Some(p) = &self.config.navigation_policy {
            Some(p.clone())
        } else if let Some(policy_lock) = &self.policy {
            Some(policy_lock.read().await.clone())
        } else {
            None
        };

        if let (Some(graph_lock), Some(policy)) = (belief_graph.as_ref(), expansion_policy) {
            let graph = graph_lock.read().await;
            let pathfinder =
                crate::memory::graph_traversal::Pathfinder::with_policy(&graph, policy);

            let mut expansions = Vec::new();
            // Expand from top semantic hits
            for hit in semantic_results.iter().take(3) {
                // hit.content for semantic layer is the entity name
                let expanded = pathfinder.guided_search(&hit.content, query, 2);
                for scored_edge in expanded {
                    // Record navigation telemetry if available
                    if let Some(ref t) = telemetry {
                        t.record_nav_score(scored_edge.policy_score).await;
                    }

                    let edge = scored_edge.edge;
                    expansions.push(ScoredResult {
                        id: format!("belief/{}", edge.id),
                        content: format!("{} {} {}", edge.source, edge.relation_type, edge.target),
                        score: scored_edge.policy_score,
                        source: "semantic_expansion".to_string(),
                        path: format!("beliefs/{}", edge.id),
                        updated_at: Some(edge.updated_at.timestamp_millis()),
                        zone: None,
                    });
                }
            }

            // Deduplicate expansions (same belief might be reached via different paths)
            let mut seen_expansions = HashSet::new();
            expansions.retain(|r| seen_expansions.insert(r.id.clone()));

            semantic_results.extend(expansions);
        }

        // 2. Determine weights: use learned policy if available, otherwise use config defaults
        let weights = if let Some(policy_lock) = &self.policy {
            let policy = policy_lock.read().await;
            policy.layer_weights
        } else {
            self.config.layer_weights
        };

        // 2. Apply layer weights to scores
        let weighted_working = self.apply_weights(working_results, weights.working);
        let weighted_episodic = self.apply_weights(episodic_results, weights.episodic);
        let weighted_semantic = self.apply_weights(semantic_results, weights.semantic);

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
            if let Some(ref graph_lock) = belief_graph {
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

        // 6. Predictive Cache Warming
        if self.config.cache_warming_enabled {
            self.perform_cache_warming(&results, belief_graph.clone());
        }

        // 7. Limit results
        let final_results: Vec<ScoredResult> =
            results.into_iter().take(self.config.max_results).collect();

        // 8. Record telemetry
        let latency = start_time.elapsed().as_millis() as u64;
        if let Some(t) = telemetry {
            t.record_latency(latency);
        }
        tracing::debug!("Retrieval latency: {}ms", latency);

        final_results
    }

    /// Predict likely future queries based on current results and navigation patterns
    pub async fn predict_next_queries(
        &self,
        results: &[ScoredResult],
        belief_graph: Option<crate::memory::belief_graph::SharedBeliefGraph>,
    ) -> Vec<String> {
        self.predict_next_queries_with_telemetry(results, belief_graph, None)
            .await
    }

    /// Enhanced next query prediction with sibling detection and telemetry hotspots
    pub async fn predict_next_queries_with_telemetry(
        &self,
        results: &[ScoredResult],
        belief_graph: Option<crate::memory::belief_graph::SharedBeliefGraph>,
        telemetry: Option<Arc<crate::memory::telemetry::NavTelemetry>>,
    ) -> Vec<String> {
        let mut predictions = HashSet::new();
        let hotspots = if let Some(ref t) = telemetry {
            let hs = t.get_hotspots(10).await;
            hs.into_iter().map(|(id, _)| id).collect::<HashSet<_>>()
        } else {
            HashSet::new()
        };

        // 1. Graph-based prediction: Explore neighbors of top hits in the belief graph
        if let (Some(graph_lock), Some(policy)) = (belief_graph, &self.config.navigation_policy) {
            // We use a block here to drop the read lock quickly
            let mut top_concepts = Vec::new();
            {
                let graph = graph_lock.read().await;
                for hit in results.iter().take(3) {
                    // Try to find if the content represents a concept in the graph
                    if let Some(node) = graph.get_node(&hit.content) {
                        top_concepts.push(node.concept);
                    }
                }
            }

            for concept in top_concepts {
                let graph = graph_lock.read().await;
                let pathfinder =
                    crate::memory::graph_traversal::Pathfinder::with_policy(&graph, policy.clone());

                // Perform a 1-hop guided search to find relevant neighbors
                let neighbors = pathfinder.guided_search(&concept, "", 1);
                for scored_edge in neighbors {
                    let mut threshold = self.config.cache_warming_threshold;

                    // Boost if the target is a hotspot
                    if hotspots.contains(&scored_edge.edge.target) {
                        threshold *= 0.7; // Lower threshold = easier to include
                    }

                    if scored_edge.policy_score >= threshold {
                        predictions.insert(scored_edge.edge.target);
                    }
                }
            }
        }

        // 2. Hierarchy-based prediction: Suggest sibling documents or sub-directories
        let mut paths = Vec::new();
        for hit in results.iter().take(5) {
            if !hit.path.is_empty() && !hit.path.starts_with("beliefs/") {
                paths.push(&hit.path);
            }
        }

        for path in paths {
            if let Some(parent) = std::path::Path::new(path).parent() {
                if let Some(parent_str) = parent.to_str() {
                    if !parent_str.is_empty() && parent_str != "." {
                        // Predict the parent directory as a potential future search scope
                        predictions.insert(parent_str.to_string());

                        // [B4] Predict siblings: suggest files in the same directory
                        if let Some(ref mem) = self.memory {
                            if let Ok(entries) = mem.ls(parent_str).await {
                                for entry in entries.iter().take(5) {
                                    if entry.is_doc && &entry.path != path {
                                        predictions.insert(entry.path.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        predictions.into_iter().collect()
    }

    /// Perform asynchronous cache warming for predicted queries
    pub fn perform_cache_warming(
        &self,
        results: &[ScoredResult],
        belief_graph: Option<crate::memory::belief_graph::SharedBeliefGraph>,
    ) {
        let memory = match &self.memory {
            Some(m) => Arc::clone(m),
            None => return,
        };

        let gating_clone = self.clone();
        let results_clone = results.to_vec();
        let belief_graph_clone = belief_graph;
        let limit = self.config.max_results;

        tokio::spawn(async move {
            let predictions = gating_clone
                .predict_next_queries(&results_clone, belief_graph_clone)
                .await;

            for query in predictions {
                let memory_inner = Arc::clone(&memory);
                tokio::spawn(async move {
                    // Background search to populate QmdMemory cache
                    let _ = memory_inner.search_with_cache(&query, limit).await;
                    tracing::debug!("Cache warmed for predicted query: '{}'", query);
                });
            }
        });
    }

    /// Retrieve only from working memory
    pub async fn retrieve_working(
        &self,
        working: &[MemoryDocument],
        query: &str,
    ) -> Vec<ScoredResult> {
        self.score_working_layer_at(working, query, chrono::Utc::now(), None)
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
        user_id: Option<&str>,
    ) -> Vec<ScoredResult> {
        let query_lower = query.to_lowercase();
        let query_terms_owned: Vec<String> = query_lower
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        let mut zone_boost = self.config.zone_boost_multiplier;
        let mut zone_penalty = self.config.zone_penalty_multiplier;

        if let (Some(booster), Some(uid)) = (&self.zone_booster, user_id) {
            if let Some(active) = &self.config.active_zones {
                if let Some(zone) = active.first() {
                    zone_boost = booster.get_boost(uid, zone.as_str()).await;
                    zone_penalty = booster.get_penalty(uid, zone.as_str()).await;
                }
            }
        }

        let mut results: Vec<ScoredResult> = if working.len() > 100 {
            let working = working.to_vec();
            let active_zones = self.config.active_zones.clone();
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
                            zone_boost_multiplier: zone_boost,
                            zone_penalty_multiplier: zone_penalty,
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

    /// Get the effective weights used for retrieval
    pub async fn effective_weights(&self) -> LayerWeights {
        if let Some(policy_lock) = &self.policy {
            let policy = policy_lock.read().await;
            policy.layer_weights
        } else {
            self.config.layer_weights
        }
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
        working_docs: &[MemoryDocument],
        all_docs: &[MemoryDocument],
        episodic: &[SessionSummary],
        semantic: &[EntityRecord],
        query: &str,
    ) -> LayeredSearchResult {
        let now = chrono::Utc::now();
        // Level 0: Working Memory (Bounded hot set)
        let level_0_results = self
            .score_working_layer_at(working_docs, query, now, None)
            .await;

        // Level 1: Entity Graph
        let level_1_results = self.score_semantic_layer_at(semantic, query, now).await;

        // Level 2: Semantic (Rules, Definitions) -> Documents with MemoryLevel::Belief
        // Note: Semantic Level 2 still uses the full corpus (all_docs) to find Beliefs.
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
                        zone: None,
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
            .score_working_layer_at(&docs, "BELA", chrono::Utc::now(), None)
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
            .score_working_layer_at(&docs, "search term", chrono::Utc::now(), None)
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

        let results = gating
            .score_working_layer_at(&docs, "BELA", now, None)
            .await;
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
            .score_working_layer_at(&docs, "bela", chrono::Utc::now(), None)
            .await;

        assert_eq!(sequential_results.len(), parallel_results.len());
        for (s, p) in sequential_results.iter().zip(parallel_results.iter()) {
            assert_eq!(s.id, p.id);
            assert!((s.score - p.score).abs() < 0.0001);
        }
    }

    #[tokio::test]
    async fn test_predict_next_queries_hierarchy() {
        let gating = AdaptiveGating::with_defaults();
        let results = vec![
            ScoredResult {
                id: "1".to_string(),
                path: "docs/rust/basics.md".to_string(),
                content: "content".to_string(),
                score: 1.0,
                source: "working".to_string(),
                zone: None,
                ..Default::default()
            },
            ScoredResult {
                id: "2".to_string(),
                path: "docs/rust/advanced.md".to_string(),
                content: "content".to_string(),
                score: 0.9,
                source: "working".to_string(),
                zone: None,
                ..Default::default()
            },
        ];

        let predictions = gating.predict_next_queries(&results, None).await;
        assert!(predictions.contains(&"docs/rust".to_string()));
    }

    #[tokio::test]
    async fn test_predict_next_queries_with_siblings() {
        use crate::memory::qmd_memory::QmdMemory;
        let docs_arc = Arc::new(tokio::sync::RwLock::new(Vec::new()));
        let memory = Arc::new(QmdMemory::new(docs_arc));

        memory
            .add_document(
                "docs/rust/main.rs".to_string(),
                "main".to_string(),
                serde_json::json!({}),
            )
            .await
            .unwrap();
        memory
            .add_document(
                "docs/rust/lib.rs".to_string(),
                "lib".to_string(),
                serde_json::json!({}),
            )
            .await
            .unwrap();

        let mut gating = AdaptiveGating::with_defaults();
        gating = gating.with_memory(memory);

        let results = vec![ScoredResult {
            id: "1".to_string(),
            path: "docs/rust/main.rs".to_string(),
            content: "content".to_string(),
            score: 1.0,
            source: "working".to_string(),
            zone: None,
            ..Default::default()
        }];

        let predictions = gating.predict_next_queries(&results, None).await;
        // Should contain parent
        assert!(predictions.contains(&"docs/rust".to_string()));
        // Should contain sibling
        assert!(predictions.contains(&"docs/rust/lib.rs".to_string()));
    }

    #[tokio::test]
    async fn test_predict_next_queries_graph() {
        let config = GatingConfig {
            cache_warming_threshold: 0.5,
            ..Default::default()
        };
        let gating = AdaptiveGating::new(config);

        let graph = Arc::new(tokio::sync::RwLock::new(
            crate::memory::belief_graph::BeliefGraph::new(),
        ));
        {
            let g = graph.write().await;
            g.add_node("Rust".to_string(), 1.0, None);
            g.add_node("Cargo".to_string(), 1.0, None);
            g.add_relation(
                "Rust".to_string(),
                "Cargo".to_string(),
                "uses".to_string(),
                None,
                None,
            )
            .await
            .unwrap();
        }

        let results = vec![ScoredResult {
            id: "rust-id".to_string(),
            content: "Rust".to_string(),
            score: 1.0,
            source: "semantic".to_string(),
            zone: None,
            ..Default::default()
        }];

        let predictions = gating.predict_next_queries(&results, Some(graph)).await;
        assert!(predictions.contains(&"cargo".to_string()));
    }

    // -----------------------------------------------------------------------
    // AdaptiveZoneBooster tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_adaptive_booster_defaults() {
        let booster = AdaptiveZoneBooster::new();
        let boost = booster.get_boost("user1", "decision").await;
        let penalty = booster.get_penalty("user1", "decision").await;
        assert!((boost - 1.5).abs() < 0.01);
        assert!((penalty - 0.5).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_adaptive_booster_feedback_adapts() {
        let booster = AdaptiveZoneBooster::new();
        // Give 10 positive feedbacks for user1/decision
        for _ in 0..10 {
            booster
                .record_feedback("user1", "decision", true, 0.9)
                .await;
        }
        // Give 5/10 negative feedbacks for user1/code
        for i in 0..10 {
            let relevant = i < 3;
            booster
                .record_feedback("user1", "code", relevant, 0.5)
                .await;
        }

        let decision_boost = booster.get_boost("user1", "decision").await;
        let code_boost = booster.get_boost("user1", "code").await;

        // decision has 100% hit rate → boost should be higher than base
        assert!(
            decision_boost > 1.6,
            "decision boost should be high, got {}",
            decision_boost
        );
        // code has 30% hit rate → boost should be lower than decision
        assert!(
            code_boost < decision_boost,
            "code boost ({}) should be < decision boost ({})",
            code_boost,
            decision_boost
        );
    }

    #[tokio::test]
    async fn test_adaptive_booster_hit_rate() {
        let booster = AdaptiveZoneBooster::new();
        assert!((booster.hit_rate("user", "zone").await - 0.0).abs() < 0.001);

        booster.record_feedback("user", "zone", true, 1.0).await;
        booster.record_feedback("user", "zone", false, 0.0).await;
        booster.record_feedback("user", "zone", true, 0.8).await;

        let rate = booster.hit_rate("user", "zone").await;
        assert!((rate - 2.0 / 3.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_adaptive_booster_reset_user() {
        let booster = AdaptiveZoneBooster::new();
        booster.record_feedback("user1", "zone", true, 1.0).await;
        booster.record_feedback("user2", "zone", true, 1.0).await;

        booster.reset_user("user1").await;

        assert!((booster.hit_rate("user1", "zone").await - 0.0).abs() < 0.001);
        assert!((booster.hit_rate("user2", "zone").await - 1.0).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_adaptive_booster_reset_all() {
        let booster = AdaptiveZoneBooster::new();
        booster.record_feedback("user1", "zone", true, 1.0).await;
        booster.record_feedback("user2", "zone", false, 0.0).await;

        booster.reset_all().await;

        assert!((booster.hit_rate("user1", "zone").await - 0.0).abs() < 0.001);
        assert!((booster.hit_rate("user2", "zone").await - 0.0).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_adaptive_booster_insufficient_data() {
        let booster = AdaptiveZoneBooster::new();
        // Less than 5 feedbacks — should return base values
        for _ in 0..3 {
            booster.record_feedback("user", "zone", true, 1.0).await;
        }
        let boost = booster.get_boost("user", "zone").await;
        let penalty = booster.get_penalty("user", "zone").await;
        // With <5 data points, should use base (not adapted)
        assert!((boost - 1.5).abs() < 0.01);
        assert!((penalty - 0.5).abs() < 0.01);
    }
}
