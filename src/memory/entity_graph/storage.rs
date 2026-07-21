// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Entity graph storage and persistence.
//!
//! Implements storage operations for the entity graph, including
//! CRUD operations for entities and relationships, with SQLite
//! persistence and in-memory caching.

use super::extraction::*;
use super::types::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub(super) struct RelationUpsert<'a> {
    pub source: &'a str,
    pub target: &'a str,
    pub relation_type: &'a str,
    pub weight: f32,
    pub co_occurrence_score: f32,
    pub memory_id: Option<&'a str>,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphData {
    pub entities: HashMap<String, EntityRecord>,
    pub entity_lookup: HashMap<String, String>,
    pub relations: HashMap<String, EntityRelationRecord>,
    pub relation_lookup: HashMap<String, String>,
    pub outgoing: HashMap<String, HashSet<String>>,
    pub incoming: HashMap<String, HashSet<String>>,
    pub memory_entities: HashMap<String, HashSet<String>>,
}

impl GraphData {
    pub(super) fn resolve_entity_id(&self, entity_id_or_name: &str) -> Option<String> {
        if self.entities.contains_key(entity_id_or_name) {
            return Some(entity_id_or_name.to_string());
        }
        let key = normalize_name(entity_id_or_name);
        self.entity_lookup.get(&key).cloned()
    }

    pub(super) fn upsert_entity(
        &mut self,
        entity: ExtractedEntity,
        memory_id: &str,
        metadata: Option<&serde_json::Value>,
        now: DateTime<Utc>,
    ) -> String {
        let normalized_name = normalize_name(&entity.name);
        let lookup_key = entity_lookup_key(&normalized_name, entity.entity_type);
        let entity_id = self
            .entity_lookup
            .get(&lookup_key)
            .cloned()
            .unwrap_or_else(|| ulid::Ulid::new().to_string());
        let record = self
            .entities
            .entry(entity_id.clone())
            .or_insert_with(|| EntityRecord {
                id: entity_id.clone(),
                name: entity.name.clone(),
                normalized_name: normalized_name.clone(),
                entity_type: entity.entity_type,
                aliases: Vec::new(),
                description: None,
                occurrence_count: 0,
                memory_count: 0,
                first_seen: now,
                last_seen: now,
                merged_from: Vec::new(),
                trust_score: 0.5,
                trust_rank: 0,
            });

        if record.name != entity.name {
            record.aliases.push(entity.name.clone());
            record.aliases.sort();
            record.aliases.dedup();
        }
        record.normalized_name = normalized_name.clone();
        record.entity_type = entity.entity_type;
        record.occurrence_count += 1;
        record.memory_count += 1;
        record.last_seen = now;
        if let Some(metadata) = metadata {
            if record.description.is_none() {
                record.description = metadata
                    .get("description")
                    .and_then(|value| value.as_str())
                    .map(|value| value.to_string());
            }
        }

        self.entity_lookup.insert(lookup_key, entity_id.clone());
        self.entity_lookup
            .entry(normalized_name.clone())
            .or_insert_with(|| entity_id.clone());
        self.memory_entities
            .entry(memory_id.to_string())
            .or_default()
            .insert(entity_id.clone());
        entity_id
    }

    pub(super) fn upsert_relation(&mut self, relation: RelationUpsert<'_>) -> EntityRelationRecord {
        let lookup_key =
            relation_lookup_key(relation.source, relation.target, relation.relation_type);
        let relation_id = self
            .relation_lookup
            .get(&lookup_key)
            .cloned()
            .unwrap_or_else(|| ulid::Ulid::new().to_string());

        let entry = self
            .relations
            .entry(relation_id.clone())
            .or_insert_with(|| EntityRelationRecord {
                id: relation_id.clone(),
                source: relation.source.to_string(),
                target: relation.target.to_string(),
                relation_type: relation.relation_type.to_string(),
                weight: 0.0,
                co_occurrence_score: relation.co_occurrence_score,
                support_count: 0,
                provenance: Vec::new(),
                confidence_score: 1.0,
                contradicts_edge_id: None,
                created_at: relation.now,
                updated_at: relation.now,
            });

        entry.weight = (entry.weight + relation.weight).min(10.0);
        entry.confidence_score = (entry.confidence_score + relation.weight).min(1.0);
        entry.co_occurrence_score = relation.co_occurrence_score.max(entry.co_occurrence_score);
        entry.support_count += 1;
        if let Some(memory_id) = relation.memory_id {
            if !entry.provenance.iter().any(|item| item == memory_id) {
                entry.provenance.push(memory_id.to_string());
            }
        }
        entry.updated_at = relation.now;

        self.relation_lookup.insert(lookup_key, relation_id.clone());
        self.outgoing
            .entry(relation.source.to_string())
            .or_default()
            .insert(relation.target.to_string());
        self.incoming
            .entry(relation.target.to_string())
            .or_default()
            .insert(relation.source.to_string());
        self.outgoing
            .entry(relation.target.to_string())
            .or_default();
        self.incoming
            .entry(relation.source.to_string())
            .or_default();

        entry.clone()
    }

    pub(super) fn rebuild_indexes(&mut self) {
        self.entity_lookup.clear();
        self.outgoing.clear();
        self.incoming.clear();
        self.relation_lookup.clear();
        for entity in self.entities.values() {
            self.entity_lookup.insert(
                entity_lookup_key(&entity.normalized_name, entity.entity_type),
                entity.id.clone(),
            );
            self.entity_lookup
                .entry(entity.normalized_name.clone())
                .or_insert_with(|| entity.id.clone());
            for alias in &entity.aliases {
                self.entity_lookup
                    .entry(normalize_name(alias))
                    .or_insert_with(|| entity.id.clone());
            }
        }
        for relation in self.relations.values() {
            self.relation_lookup.insert(
                relation_lookup_key(&relation.source, &relation.target, &relation.relation_type),
                relation.id.clone(),
            );
            self.outgoing
                .entry(relation.source.clone())
                .or_default()
                .insert(relation.target.clone());
            self.incoming
                .entry(relation.target.clone())
                .or_default()
                .insert(relation.source.clone());
        }
    }

    pub(super) fn relation_neighbors(&self, entity_id: &str) -> HashSet<String> {
        let mut neighbors = HashSet::new();
        if let Some(outgoing) = self.outgoing.get(entity_id) {
            neighbors.extend(outgoing.iter().cloned());
        }
        if let Some(incoming) = self.incoming.get(entity_id) {
            neighbors.extend(incoming.iter().cloned());
        }
        neighbors
    }

    pub(super) fn relink_relation_neighbor(&mut self, from: &str, to: &str, neighbor: &str) {
        for relation in self.relations.values_mut() {
            if relation.source == from && relation.target == neighbor {
                relation.source = to.to_string();
            }
            if relation.target == from && relation.source == neighbor {
                relation.target = to.to_string();
            }
        }
    }

    pub(super) fn remove_relations_for_entity(&mut self, entity_id: &str) {
        self.relations
            .retain(|_, relation| relation.source != entity_id && relation.target != entity_id);
    }

    pub(super) fn apply_decay(&mut self, factor: f32, now: DateTime<Utc>) {
        let factor = factor.clamp(0.0, 1.0);
        for relation in self.relations.values_mut() {
            let hours_since = (now - relation.updated_at).num_hours() as f32;
            if hours_since > 0.0 {
                let decay = (1.0 - factor).powf(hours_since);
                relation.weight *= decay;
                relation.confidence_score *= decay;
                relation.updated_at = now;
            }
        }
        for entity in self.entities.values_mut() {
            let hours_since = (now - entity.last_seen).num_hours() as f32;
            if hours_since > 0.0 {
                let decay = (1.0 - factor).powf(hours_since);
                entity.trust_score *= decay;
                entity.last_seen = now;
            }
        }
    }
}
