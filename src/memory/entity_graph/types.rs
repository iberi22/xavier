//! Entity graph type definitions.
//!
//! Defines the core data structures for the entity graph, including
//! entities, relationships, property types, and serialization formats
//! used throughout the knowledge graph layer.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub use xavier_core_logic::{EntityRecord, EntityType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedEntity {
    pub name: String,
    pub entity_type: EntityType,
    pub span: (usize, usize),
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
