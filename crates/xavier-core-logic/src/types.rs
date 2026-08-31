//! Pure type definitions for core logic (BM25, RRF, scoring, snippet extraction)
//!
//! Free of I/O, database dependencies, or async runtimes. WASM compatible.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Clearance levels for security gating.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub enum ClearanceLevel {
    #[serde(
        rename = "UNCLASSIFIED",
        alias = "unclassified",
        alias = "Unclassified"
    )]
    #[default]
    Unclassified = 0,
    #[serde(rename = "INTERNAL", alias = "internal", alias = "Internal")]
    Internal = 1,
    #[serde(rename = "RESTRICTED", alias = "restricted", alias = "Restricted")]
    Restricted = 2,
    #[serde(
        rename = "CONFIDENTIAL",
        alias = "confidential",
        alias = "Confidential"
    )]
    Confidential = 3,
    #[serde(rename = "SECRET", alias = "secret", alias = "Secret")]
    Secret = 4,
    #[serde(
        rename = "TOPSECRET",
        alias = "top_secret",
        alias = "TopSecret",
        alias = "topsecret"
    )]
    TopSecret = 5,
}

impl From<u8> for ClearanceLevel {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Unclassified,
            1 => Self::Internal,
            2 => Self::Restricted,
            3 => Self::Confidential,
            4 => Self::Secret,
            5 => Self::TopSecret,
            _ => Self::Unclassified,
        }
    }
}

impl From<ClearanceLevel> for u8 {
    fn from(level: ClearanceLevel) -> Self {
        level as u8
    }
}

impl From<&str> for ClearanceLevel {
    fn from(value: &str) -> Self {
        match value.to_ascii_uppercase().as_str() {
            "UNCLASSIFIED" => Self::Unclassified,
            "INTERNAL" => Self::Internal,
            "RESTRICTED" => Self::Restricted,
            "CONFIDENTIAL" => Self::Confidential,
            "SECRET" => Self::Secret,
            "TOPSECRET" | "TOP_SECRET" => Self::TopSecret,
            _ => Self::Unclassified,
        }
    }
}

impl ClearanceLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unclassified => "unclassified",
            Self::Internal => "internal",
            Self::Restricted => "restricted",
            Self::Confidential => "confidential",
            Self::Secret => "secret",
            Self::TopSecret => "top_secret",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "unclassified" => Self::Unclassified,
            "internal" => Self::Internal,
            "restricted" => Self::Restricted,
            "confidential" => Self::Confidential,
            "secret" => Self::Secret,
            _ => Self::TopSecret,
        }
    }
}

/// Context zones for retrieval weighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContextZone {
    #[default]
    Atomic,
    Cluster,
    Global,
    Relational,
}

impl ContextZone {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Atomic => "atomic",
            Self::Cluster => "cluster",
            Self::Global => "global",
            Self::Relational => "relational",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "cluster" => Self::Cluster,
            "global" => Self::Global,
            "relational" => Self::Relational,
            _ => Self::Atomic,
        }
    }
}

/// Memory processing levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLevel {
    #[default]
    Raw,
    Processed,
    Extracted,
    Belief,
}

impl MemoryLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Processed => "processed",
            Self::Extracted => "extracted",
            Self::Belief => "belief",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "processed" => Self::Processed,
            "extracted" => Self::Extracted,
            "belief" => Self::Belief,
            _ => Self::Raw,
        }
    }
}

/// Relation kind descriptor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelationKind {
    pub name: String,
    pub inverse: Option<String>,
}

impl RelationKind {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            inverse: None,
        }
    }
}

/// Document representation for memory retrieval and BM25 scoring.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryDocument {
    pub id: Option<String>,
    pub path: String,
    pub content: String,
    pub metadata: serde_json::Value,
    #[serde(default)]
    pub content_vector: Option<Vec<f32>>,
    pub embedding: Vec<f32>,
    #[serde(default)]
    pub cluster_id: Option<String>,
    pub parent_id: Option<String>,
    #[serde(default)]
    pub level: MemoryLevel,
    #[serde(default)]
    pub relation: Option<RelationKind>,
    #[serde(default)]
    pub clearance: ClearanceLevel,
    #[serde(default)]
    pub minhash: Option<Vec<u64>>,
    #[serde(default)]
    pub score: f32,
    #[serde(default)]
    pub source_node_id: Option<String>,
    #[serde(default)]
    pub source_db_id: Option<String>,
}

impl Default for MemoryDocument {
    fn default() -> Self {
        Self {
            id: None,
            path: String::new(),
            content: String::new(),
            metadata: serde_json::json!({}),
            content_vector: None,
            embedding: Vec::new(),
            cluster_id: None,
            parent_id: None,
            level: MemoryLevel::Raw,
            relation: None,
            clearance: ClearanceLevel::Unclassified,
            minhash: None,
            score: 0.0,
            source_node_id: None,
            source_db_id: None,
        }
    }
}

impl MemoryDocument {
    pub fn estimated_bytes(&self) -> u64 {
        self.id
            .as_ref()
            .map(|value| value.len())
            .unwrap_or_default() as u64
            + self.path.len() as u64
            + self.content.len() as u64
            + self.metadata.to_string().len() as u64
            + self
                .content_vector
                .as_ref()
                .map(|value| value.len() * std::mem::size_of::<f32>())
                .unwrap_or_default() as u64
            + (self.embedding.len() * std::mem::size_of::<f32>()) as u64
            + self.cluster_id.as_ref().map(|s| s.len()).unwrap_or(0) as u64
            + self.parent_id.as_ref().map(|s| s.len()).unwrap_or(0) as u64
            + 1
            + self.relation.as_ref().map(|r| r.name.len()).unwrap_or(0) as u64
            + self
                .minhash
                .as_ref()
                .map(|m| m.len() * std::mem::size_of::<u64>())
                .unwrap_or(0) as u64
            + 4
            + self.source_node_id.as_ref().map(|s| s.len()).unwrap_or(0) as u64
            + self.source_db_id.as_ref().map(|s| s.len()).unwrap_or(0) as u64
    }
}

/// Key event within an episodic session summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub timestamp: DateTime<Utc>,
    pub description: String,
    pub event_type: String,
}

/// Session summary structure for episodic retrieval scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub start_time: DateTime<Utc>,
    pub summary: String,
    pub key_events: Vec<Event>,
    #[serde(default)]
    pub sentiment_timeline: Vec<f32>,
}

/// Entity types in knowledge graph.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    Person,
    Organization,
    Location,
    Product,
    Concept,
    Unknown,
}

impl EntityType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::Organization => "organization",
            Self::Location => "location",
            Self::Product => "product",
            Self::Concept => "concept",
            Self::Unknown => "unknown",
        }
    }
}

/// Entity record in knowledge graph for semantic scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRecord {
    pub id: String,
    pub name: String,
    pub normalized_name: String,
    pub entity_type: EntityType,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub description: Option<String>,
    pub occurrence_count: usize,
    pub memory_count: usize,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    #[serde(default)]
    pub merged_from: Vec<String>,
    #[serde(default)]
    pub trust_score: f32,
    #[serde(default)]
    pub trust_rank: usize,
}

/// Scored search result returned by retrieval and RRF algorithms.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct ScoredResult {
    pub id: String,
    pub content: String,
    pub score: f32,
    pub source: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub updated_at: Option<i64>,
    #[serde(default)]
    pub zone: Option<String>,
}
