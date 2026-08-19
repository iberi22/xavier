//! Hierarchical semantic memory compressor for aged sessions
//!
//! Consolidates 100+ dialogue turns into structured factual concepts and
//! summary memory cards, reducing SQLite memory footprint while preserving
//! critical entity identifiers.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// A single dialogue turn within a conversation session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogueTurn {
    /// Unique turn identifier
    pub id: String,
    /// Identifier of the session this turn belongs to
    pub session_id: String,
    /// Role of the speaker ("user", "assistant", "system", etc.)
    pub role: String,
    /// Text content of the turn
    pub content: String,
    /// Optional vector embedding representation of the turn content
    pub embedding: Option<Vec<f32>>,
    /// Timestamp when the turn occurred
    pub timestamp: DateTime<Utc>,
    /// Explicitly recognized or annotated entities in this turn
    pub entities: Vec<String>,
    /// Index position of turn in the session
    pub turn_index: usize,
}

impl DialogueTurn {
    /// Create a new dialogue turn
    pub fn new(
        id: impl Into<String>,
        session_id: impl Into<String>,
        role: impl Into<String>,
        content: impl Into<String>,
        turn_index: usize,
    ) -> Self {
        Self {
            id: id.into(),
            session_id: session_id.into(),
            role: role.into(),
            content: content.into(),
            embedding: None,
            timestamp: Utc::now(),
            entities: Vec::new(),
            turn_index,
        }
    }

    /// Builder method to set embedding
    pub fn with_embedding(mut self, embedding: Vec<f32>) -> Self {
        self.embedding = Some(embedding);
        self
    }

    /// Builder method to set entities
    pub fn with_entities(mut self, entities: Vec<String>) -> Self {
        self.entities = entities;
        self
    }

    /// Builder method to set timestamp
    pub fn with_timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
        self.timestamp = timestamp;
        self
    }
}

/// A structured semantic summary card representing compressed factual concepts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticSummaryCard {
    /// Unique summary card identifier
    pub card_id: String,
    /// Session identifier
    pub session_id: String,
    /// Compression hierarchy level (1 = Turn cluster concept, 2 = Session factual summary)
    pub level: usize,
    /// Descriptive title for this concept card
    pub title: String,
    /// Synthesized summary text preserving factual details
    pub summary: String,
    /// Source dialogue turn IDs merged into this card
    pub source_turn_ids: Vec<String>,
    /// Key entity identifiers preserved from source turns
    pub key_entities: Vec<String>,
    /// Compression ratio achieved (0.0 to 1.0 scale, e.g., 0.75 = 75% size reduction)
    pub compression_ratio: f32,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Raw input character count
    pub raw_char_count: usize,
    /// Compressed output character count
    pub compressed_char_count: usize,
    /// Additional metadata key-value pairs
    pub metadata: HashMap<String, String>,
}

/// Result object returned by compression operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionResult {
    /// Session identifier processed
    pub session_id: String,
    /// Synthesized semantic summary cards across hierarchy levels
    pub cards: Vec<SemanticSummaryCard>,
    /// Original dialogue turn count
    pub original_turn_count: usize,
    /// Original text character count
    pub original_char_count: usize,
    /// Compressed text character count across final summary nodes
    pub compressed_char_count: usize,
    /// Overall compression ratio achieved
    pub overall_compression_ratio: f32,
    /// Complete list of entity identifiers preserved during compression
    pub preserved_entities: Vec<String>,
    /// Estimated storage bytes saved
    pub storage_bytes_saved: usize,
}

/// Configuration options for the semantic memory compressor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticCompressorConfig {
    /// Cosine similarity threshold for clustering turns (default: 0.85)
    pub similarity_threshold: f32,
    /// Maximum turns permitted per cluster (default: 10)
    pub max_cluster_size: usize,
    /// Minimum turns required to form a cluster (default: 1)
    pub min_cluster_size: usize,
    /// Target compression ratio threshold (default: 0.70)
    pub target_compression_ratio: f32,
    /// Age threshold in hours after which sessions are eligible for compression (default: 24)
    pub aged_session_hours: u64,
    /// Max hierarchy levels for multi-tier summarization (default: 2)
    pub max_hierarchy_levels: usize,
}

impl Default for SemanticCompressorConfig {
    fn default() -> Self {
        Self {
            similarity_threshold: 0.85,
            max_cluster_size: 10,
            min_cluster_size: 1,
            target_compression_ratio: 0.70,
            aged_session_hours: 24,
            max_hierarchy_levels: 2,
        }
    }
}

/// Hierarchical semantic memory compressor for dialogue sessions
#[derive(Debug, Clone)]
pub struct SemanticCompressor {
    config: SemanticCompressorConfig,
}

impl SemanticCompressor {
    /// Create a new `SemanticCompressor` with default configuration
    pub fn new() -> Self {
        Self::with_config(SemanticCompressorConfig::default())
    }

    /// Create a new `SemanticCompressor` with custom configuration
    pub fn with_config(config: SemanticCompressorConfig) -> Self {
        Self { config }
    }

    /// Get current configuration reference
    pub fn config(&self) -> &SemanticCompressorConfig {
        &self.config
    }

    /// Compute cosine similarity between two vector embeddings
    pub fn calculate_cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
        if left.is_empty() || right.is_empty() || left.len() != right.len() {
            return 0.0;
        }

        let dot_product: f32 = left.iter().zip(right.iter()).map(|(a, b)| a * b).sum();
        let left_mag: f32 = left.iter().map(|val| val * val).sum::<f32>().sqrt();
        let right_mag: f32 = right.iter().map(|val| val * val).sum::<f32>().sqrt();

        if left_mag == 0.0 || right_mag == 0.0 {
            0.0
        } else {
            dot_product / (left_mag * right_mag)
        }
    }

    /// Compute cosine distance between two vector embeddings (1.0 - cosine_similarity)
    pub fn calculate_cosine_distance(left: &[f32], right: &[f32]) -> f32 {
        1.0 - Self::calculate_cosine_similarity(left, right)
    }

    /// Compute textual Jaccard keyword similarity as fallback when embeddings are absent
    pub fn calculate_text_similarity(a: &str, b: &str) -> f32 {
        let words_a: HashSet<String> = a
            .split_whitespace()
            .map(|w| w.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()).to_string())
            .filter(|w| w.len() >= 3)
            .collect();
        let words_b: HashSet<String> = b
            .split_whitespace()
            .map(|w| w.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()).to_string())
            .filter(|w| w.len() >= 3)
            .collect();

        if words_a.is_empty() || words_b.is_empty() {
            return 0.0;
        }

        let intersection = words_a.intersection(&words_b).count();
        let union = words_a.union(&words_b).count();

        if union == 0 {
            0.0
        } else {
            intersection as f32 / union as f32
        }
    }

    /// Measure pairwise turn similarity considering both vector embeddings and keyword fallback
    pub fn turn_similarity(&self, turn_a: &DialogueTurn, turn_b: &DialogueTurn) -> f32 {
        match (&turn_a.embedding, &turn_b.embedding) {
            (Some(emb_a), Some(emb_b)) if !emb_a.is_empty() && !emb_b.is_empty() => {
                Self::calculate_cosine_similarity(emb_a, emb_b)
            }
            _ => Self::calculate_text_similarity(&turn_a.content, &turn_b.content),
        }
    }

    /// Extract key entity identifiers (alphanumeric IDs, dates, technical terms, code elements)
    pub fn extract_key_entities(text: &str) -> Vec<String> {
        let mut entities = HashSet::new();

        // Regex patterns for entity identification
        for word in text.split_whitespace() {
            let clean = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-');

            // 1. Specific key patterns like IDs: USR-123, PROJ-001, NODE-01, KEY-99, etc.
            if clean.contains('-') || clean.contains('_') {
                let parts: Vec<&str> = clean.split(&['-', '_'][..]).collect();
                if parts.len() >= 2 && parts.iter().any(|p| p.chars().any(|c| c.is_ascii_digit())) {
                    entities.insert(clean.to_string());
                }
            }

            // 2. Dates or version numbers like 2025-03-01, v1.2.3
            if clean.starts_with('v') && clean.len() > 1 && clean[1..].chars().next().is_some_and(|c| c.is_ascii_digit()) {
                entities.insert(clean.to_string());
            }

            // 3. Mixed uppercase/digit codes like AB123, DB42, UUIDs
            if clean.len() >= 3 && clean.chars().any(|c| c.is_ascii_uppercase()) && clean.chars().any(|c| c.is_ascii_digit()) {
                entities.insert(clean.to_string());
            }

            // 4. Proper capitalization (CamelCase or PascalCase)
            if clean.len() >= 4
                && clean.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                && !["What", "When", "Where", "This", "That", "There", "Here", "Your", "With", "From"].contains(&clean)
            {
                entities.insert(clean.to_string());
            }
        }

        let mut sorted: Vec<String> = entities.into_iter().collect();
        sorted.sort();
        sorted
    }

    /// Cluster turns based on cosine similarity threshold (>0.85)
    pub fn cluster_turns(&self, turns: &[DialogueTurn]) -> Vec<Vec<DialogueTurn>> {
        if turns.is_empty() {
            return Vec::new();
        }

        let mut clusters: Vec<Vec<DialogueTurn>> = Vec::new();

        for turn in turns {
            let mut added = false;

            // Try adding to existing cluster if similarity to cluster seed/centroid >= threshold
            if let Some(last_cluster) = clusters.last_mut() {
                if last_cluster.len() < self.config.max_cluster_size {
                    // Compare against seed (first element) or immediate neighbor
                    let sim = self.turn_similarity(&last_cluster[0], turn);
                    let neighbor_sim = self.turn_similarity(last_cluster.last().unwrap(), turn);

                    if sim >= self.config.similarity_threshold || neighbor_sim >= self.config.similarity_threshold {
                        last_cluster.push(turn.clone());
                        added = true;
                    }
                }
            }

            if !added {
                clusters.push(vec![turn.clone()]);
            }
        }

        clusters
    }

    /// Synthesize a Level-1 semantic summary card from a cluster of dialogue turns
    pub fn synthesize_card_level1(&self, session_id: &str, cluster: &[DialogueTurn], card_index: usize) -> SemanticSummaryCard {
        let raw_char_count: usize = cluster.iter().map(|t| t.content.len()).sum();
        let turn_ids: Vec<String> = cluster.iter().map(|t| t.id.clone()).collect();

        // Extract and aggregate all entity identifiers
        let mut key_entities_set = HashSet::new();
        for turn in cluster {
            for entity in &turn.entities {
                key_entities_set.insert(entity.clone());
            }
            for extracted in Self::extract_key_entities(&turn.content) {
                key_entities_set.insert(extracted);
            }
        }

        let mut key_entities: Vec<String> = key_entities_set.into_iter().collect();
        key_entities.sort();

        // Synthesize summary text
        let mut summary_lines = Vec::new();
        summary_lines.push(format!("### Concept Summary #{card_index} ({session_id})"));

        if !key_entities.is_empty() {
            summary_lines.push(format!("**Entities**: {}", key_entities.join(", ")));
        }

        let roles: HashSet<&str> = cluster.iter().map(|t| t.role.as_str()).collect();
        summary_lines.push(format!("**Participants**: {}", roles.into_iter().collect::<Vec<_>>().join(", ")));

        summary_lines.push("**Key Statements:**".to_string());
        for turn in cluster {
            let snippet = if turn.content.len() > 100 {
                format!("{}...", &turn.content[..100].trim())
            } else {
                turn.content.trim().to_string()
            };
            summary_lines.push(format!("- {}: {}", turn.role, snippet));
        }

        let summary_text = summary_lines.join("\n");
        let compressed_char_count = summary_text.len();

        let compression_ratio = if raw_char_count > 0 {
            1.0 - (compressed_char_count as f32 / raw_char_count as f32).min(1.0)
        } else {
            0.0
        };

        let mut metadata = HashMap::new();
        metadata.insert("cluster_size".to_string(), cluster.len().to_string());
        metadata.insert("level".to_string(), "1".to_string());

        SemanticSummaryCard {
            card_id: format!("{session_id}-card-l1-{card_index}"),
            session_id: session_id.to_string(),
            level: 1,
            title: format!("Cluster #{card_index} Factual Concept"),
            summary: summary_text,
            source_turn_ids: turn_ids,
            key_entities,
            compression_ratio,
            created_at: Utc::now(),
            raw_char_count,
            compressed_char_count,
            metadata,
        }
    }

    /// Synthesize a Level-2 hierarchical session summary card from Level-1 cards
    pub fn synthesize_card_level2(&self, session_id: &str, level1_cards: &[SemanticSummaryCard]) -> SemanticSummaryCard {
        let raw_char_count: usize = level1_cards.iter().map(|c| c.raw_char_count).sum();
        let mut source_turn_ids = Vec::new();

        let mut key_entities_set = HashSet::new();
        for card in level1_cards {
            source_turn_ids.extend(card.source_turn_ids.clone());
            for entity in &card.key_entities {
                key_entities_set.insert(entity.clone());
            }
        }

        let mut key_entities: Vec<String> = key_entities_set.into_iter().collect();
        key_entities.sort();

        let mut summary_lines = Vec::new();
        summary_lines.push(format!("# Executive Factual Overview for Session {session_id}"));
        summary_lines.push(format!("- **Total Clusters Summarized**: {}", level1_cards.len()));

        if !key_entities.is_empty() {
            summary_lines.push(format!("- **Preserved Key Entities**: {}", key_entities.join(", ")));
        }

        summary_lines.push("\n## Core Factual Concepts:".to_string());
        for (idx, card) in level1_cards.iter().enumerate() {
            let first_line = card.summary.lines().nth(3).unwrap_or("Concept cluster");
            summary_lines.push(format!("{}. {}: {}", idx + 1, card.title, first_line));
        }

        let summary_text = summary_lines.join("\n");
        let compressed_char_count = summary_text.len();

        let compression_ratio = if raw_char_count > 0 {
            1.0 - (compressed_char_count as f32 / raw_char_count as f32).min(1.0)
        } else {
            0.0
        };

        let mut metadata = HashMap::new();
        metadata.insert("l1_cards_count".to_string(), level1_cards.len().to_string());
        metadata.insert("level".to_string(), "2".to_string());

        SemanticSummaryCard {
            card_id: format!("{session_id}-card-l2-overview"),
            session_id: session_id.to_string(),
            level: 2,
            title: format!("Aged Session Overview - {session_id}"),
            summary: summary_text,
            source_turn_ids,
            key_entities,
            compression_ratio,
            created_at: Utc::now(),
            raw_char_count,
            compressed_char_count,
            metadata,
        }
    }

    /// Check whether a session is considered "aged" based on elapsed hours since last turn
    pub fn is_session_aged(&self, turns: &[DialogueTurn]) -> bool {
        if turns.is_empty() {
            return false;
        }

        let latest_timestamp = turns.iter().map(|t| t.timestamp).max().unwrap_or_else(Utc::now);
        let elapsed_hours = (Utc::now() - latest_timestamp).num_hours();

        elapsed_hours >= self.config.aged_session_hours as i64
    }

    /// Compress a set of dialogue turns for a session into hierarchical semantic summary cards
    pub fn compress_session(&self, session_id: &str, turns: &[DialogueTurn]) -> CompressionResult {
        if turns.is_empty() {
            return CompressionResult {
                session_id: session_id.to_string(),
                cards: Vec::new(),
                original_turn_count: 0,
                original_char_count: 0,
                compressed_char_count: 0,
                overall_compression_ratio: 0.0,
                preserved_entities: Vec::new(),
                storage_bytes_saved: 0,
            };
        }

        let original_char_count: usize = turns.iter().map(|t| t.content.len()).sum();

        // 1. Cluster dialogue turns by similarity threshold (>0.85)
        let clusters = self.cluster_turns(turns);

        // 2. Synthesize Level-1 summary cards
        let mut cards = Vec::new();
        for (idx, cluster) in clusters.iter().enumerate() {
            let l1_card = self.synthesize_card_level1(session_id, cluster, idx + 1);
            cards.push(l1_card);
        }

        // 3. Synthesize Level-2 overview card if hierarchical mode enabled
        if self.config.max_hierarchy_levels >= 2 && !cards.is_empty() {
            let level1_cards: Vec<SemanticSummaryCard> = cards.clone();
            let l2_card = self.synthesize_card_level2(session_id, &level1_cards);
            cards.push(l2_card);
        }

        // Collect all preserved entities across all generated cards
        let mut preserved_entities_set = HashSet::new();
        for card in &cards {
            for entity in &card.key_entities {
                preserved_entities_set.insert(entity.clone());
            }
        }

        let mut preserved_entities: Vec<String> = preserved_entities_set.into_iter().collect();
        preserved_entities.sort();

        // Calculate final compressed character count based on the highest level summary
        let final_overview = cards.iter().find(|c| c.level == 2).unwrap_or_else(|| &cards[0]);
        let compressed_char_count = final_overview.compressed_char_count;

        let overall_compression_ratio = if original_char_count > 0 {
            1.0 - (compressed_char_count as f32 / original_char_count as f32).min(1.0)
        } else {
            0.0
        };

        let storage_bytes_saved = original_char_count.saturating_sub(compressed_char_count);

        CompressionResult {
            session_id: session_id.to_string(),
            cards,
            original_turn_count: turns.len(),
            original_char_count,
            compressed_char_count,
            overall_compression_ratio,
            preserved_entities,
            storage_bytes_saved,
        }
    }
}

impl Default for SemanticCompressor {
    fn default() -> Self {
        Self::new()
    }
}
