//! Entity graph type definitions.
//!
//! Defines the core data structures for the entity graph, including
//! entities, relationships, property types, and serialization formats
//! used throughout the knowledge graph layer.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
    /// As str.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedEntity {
    pub name: String,
    pub entity_type: EntityType,
    pub span: (usize, usize),
}

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
    /// Trust score [0.0, 1.0] based on confirmation count (default 0.5)
    #[serde(default)]
    pub trust_score: f32,
    /// Trust rank for ordering (higher = more trusted)
    #[serde(default)]
    pub trust_rank: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRelationRecord {
    pub id: String,
    pub source: String,
    pub target: String,
    pub relation_type: String,
    pub weight: f32,
    pub co_occurrence_score: f32,
    pub support_count: usize,
    #[serde(default)]
    pub provenance: Vec<String>,
    pub confidence_score: f32,
    pub contradicts_edge_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl EntityRelationRecord {
    /// Source name.
    pub fn source_name(&self, data: &super::storage::GraphData) -> String {
        data.entities
            .get(&self.source)
            .map(|e| e.normalized_name.clone())
            .unwrap_or_else(|| self.source.clone())
    }

    /// Target name.
    pub fn target_name(&self, data: &super::storage::GraphData) -> String {
        data.entities
            .get(&self.target)
            .map(|e| e.normalized_name.clone())
            .unwrap_or_else(|| self.target.clone())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum GraphDirection {
    #[default]
    Outgoing,
    Incoming,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversalStep {
    pub from: String,
    pub to: String,
    pub relation_type: String,
    pub depth: usize,
    pub weight: f32,
    #[serde(default)]
    pub path: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityNeighbors {
    pub entity: EntityRecord,
    pub incoming: Vec<EntityRelationRecord>,
    pub outgoing: Vec<EntityRelationRecord>,
    pub traversal: Vec<TraversalStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphRelationsView {
    pub entity_id: Option<String>,
    pub direction: GraphDirection,
    pub max_depth: usize,
    pub total_relations: usize,
    pub relations: Vec<EntityRelationRecord>,
    #[serde(default)]
    pub traversal: Vec<TraversalStep>,
}

#[derive(Debug, Clone)]
pub struct RawRelation {
    pub source: String,
    pub target: String,
    pub relation_type: String,
    pub score: f32,
}
