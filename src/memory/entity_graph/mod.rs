//! Entity graph for memory relationships
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.

pub mod extraction;
pub mod inference;
pub mod storage;
pub mod types;

pub use types::*;

use anyhow::{anyhow, Result};
use chrono::Utc;
use std::collections::{HashSet, VecDeque};
use tokio::sync::RwLock;

use storage::GraphData;
use storage::RelationUpsert;

#[derive(Debug, Default)]
pub struct EntityGraph {
    inner: RwLock<GraphData>,
}

pub type SharedEntityGraph = std::sync::Arc<EntityGraph>;

impl EntityGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn upsert_memory(
        &self,
        memory_id: &str,
        content: &str,
        metadata: Option<&serde_json::Value>,
    ) -> Result<GraphRelationsView> {
        let extracted = extraction::extract_entities(content);
        self.index_memory(memory_id, content, metadata, extracted)
            .await
    }

    pub async fn remove_memory(&self, memory_id: &str) -> Result<()> {
        let mut data = self.inner.write().await;
        if let Some(entities) = data.memory_entities.remove(memory_id) {
            for entity_id in entities {
                if let Some(entity) = data.entities.get_mut(&entity_id) {
                    entity.memory_count = entity.memory_count.saturating_sub(1);
                    entity.last_seen = Utc::now();
                }
            }
        }

        data.relations.retain(|_, relation| {
            !relation
                .provenance
                .iter()
                .any(|provenance| provenance == memory_id)
        });
        data.rebuild_indexes();
        Ok(())
    }

    pub async fn entity(&self, entity_id_or_name: &str) -> Option<EntityRecord> {
        let data = self.inner.read().await;
        data.resolve_entity_id(entity_id_or_name)
            .and_then(|id| data.entities.get(&id).cloned())
    }

    pub async fn all_entities(&self) -> Vec<EntityRecord> {
        self.inner.read().await.entities.values().cloned().collect()
    }

    pub async fn all_relations(&self) -> Vec<EntityRelationRecord> {
        self.inner
            .read()
            .await
            .relations
            .values()
            .cloned()
            .collect()
    }

    pub async fn relations_for_entity(
        &self,
        entity_id_or_name: &str,
        max_depth: usize,
        relation_types: Option<&[String]>,
        direction: GraphDirection,
    ) -> Result<GraphRelationsView> {
        let data = self.inner.read().await;
        let entity_id = data
            .resolve_entity_id(entity_id_or_name)
            .ok_or_else(|| anyhow!("entity not found: {entity_id_or_name}"))?;
        let traversal =
            Self::traverse_locked(&data, &entity_id, max_depth, relation_types, direction);
        let relations = Self::relations_from_locked(&data, &entity_id, relation_types, direction);

        Ok(GraphRelationsView {
            entity_id: Some(entity_id),
            direction,
            max_depth,
            total_relations: relations.len(),
            relations,
            traversal,
        })
    }

    pub async fn entity_neighbors(
        &self,
        entity_id_or_name: &str,
        max_depth: usize,
        relation_types: Option<&[String]>,
        direction: GraphDirection,
    ) -> Result<EntityNeighbors> {
        let data = self.inner.read().await;
        let entity_id = data
            .resolve_entity_id(entity_id_or_name)
            .ok_or_else(|| anyhow!("entity not found: {entity_id_or_name}"))?;
        let entity = data
            .entities
            .get(&entity_id)
            .cloned()
            .ok_or_else(|| anyhow!("entity not found: {entity_id}"))?;
        let traversal =
            Self::traverse_locked(&data, &entity_id, max_depth, relation_types, direction);
        let incoming = Self::relations_from_ids_locked(&data, &entity_id, relation_types, true);
        let outgoing = Self::relations_from_ids_locked(&data, &entity_id, relation_types, false);

        Ok(EntityNeighbors {
            entity,
            incoming,
            outgoing,
            traversal,
        })
    }

    pub async fn export_json(&self) -> Result<String> {
        let data = self.inner.read().await;
        serde_json::to_string(&*data).map_err(|e| anyhow!("failed to export json: {e}"))
    }

    pub async fn import_json(&self, json: &str) -> Result<()> {
        let mut data = self.inner.write().await;
        let imported: GraphData =
            serde_json::from_str(json).map_err(|e| anyhow!("failed to import json: {e}"))?;
        *data = imported;
        data.rebuild_indexes();
        Ok(())
    }

    pub async fn export_bincode(&self) -> Result<Vec<u8>> {
        let data = self.inner.read().await;
        bincode::serialize(&*data).map_err(|e| anyhow!("failed to export bincode: {e}"))
    }

    pub async fn import_bincode(&self, bytes: &[u8]) -> Result<()> {
        let mut data = self.inner.write().await;
        let imported: GraphData =
            bincode::deserialize(bytes).map_err(|e| anyhow!("failed to import bincode: {e}"))?;
        *data = imported;
        data.rebuild_indexes();
        Ok(())
    }

    pub async fn apply_decay(&self, factor: f32) -> Result<()> {
        let mut data = self.inner.write().await;
        data.apply_decay(factor, Utc::now());
        Ok(())
    }

    pub async fn run_inference(&self) -> Result<Vec<EntityRelationRecord>> {
        let mut data = self.inner.write().await;
        let inferred = inference::InferenceEngine::run(&mut data);
        if !inferred.is_empty() {
            data.rebuild_indexes();
        }
        Ok(inferred)
    }

    pub async fn merge_entities(
        &self,
        primary_id: &str,
        secondary_id: &str,
    ) -> Result<EntityRecord> {
        let mut data = self.inner.write().await;
        let primary_id = data
            .resolve_entity_id(primary_id)
            .ok_or_else(|| anyhow!("primary entity not found: {primary_id}"))?;
        let secondary_id = data
            .resolve_entity_id(secondary_id)
            .ok_or_else(|| anyhow!("secondary entity not found: {secondary_id}"))?;
        if primary_id == secondary_id {
            return data
                .entities
                .get(&primary_id)
                .cloned()
                .ok_or_else(|| anyhow!("entity not found: {primary_id}"));
        }

        let Some(mut secondary) = data.entities.remove(&secondary_id) else {
            return Err(anyhow!("secondary entity not found: {secondary_id}"));
        };
        let merged = {
            let Some(primary) = data.entities.get_mut(&primary_id) else {
                return Err(anyhow!("primary entity not found: {primary_id}"));
            };

            primary.aliases.push(secondary.name.clone());
            primary.aliases.append(&mut secondary.aliases);
            primary.aliases.sort();
            primary.aliases.dedup();
            primary.occurrence_count += secondary.occurrence_count;
            primary.memory_count += secondary.memory_count;
            primary.merged_from.push(secondary.id.clone());
            primary.merged_from.append(&mut secondary.merged_from);
            primary.merged_from.sort();
            primary.merged_from.dedup();
            primary.last_seen = primary.last_seen.max(secondary.last_seen);
            if primary.description.is_none() {
                primary.description = secondary.description.take();
            }

            primary.clone()
        };

        let neighbor_ids: Vec<String> = data
            .relation_neighbors(&secondary_id)
            .iter()
            .cloned()
            .collect();
        for entity_id in neighbor_ids {
            if entity_id == primary_id {
                continue;
            }
            data.relink_relation_neighbor(&secondary_id, &primary_id, &entity_id);
        }
        data.remove_relations_for_entity(&secondary_id);
        data.rebuild_indexes();
        Ok(merged)
    }

    /// Shims for backward compatibility
    pub fn extract_entities(text: &str) -> Vec<ExtractedEntity> {
        extraction::extract_entities(text)
    }

    pub fn extract_relation_candidates(text: &str) -> Vec<RawRelation> {
        extraction::extract_relation_candidates(text)
    }

    async fn index_memory(
        &self,
        memory_id: &str,
        content: &str,
        metadata: Option<&serde_json::Value>,
        extracted: Vec<ExtractedEntity>,
    ) -> Result<GraphRelationsView> {
        let now = Utc::now();
        let mut data = self.inner.write().await;
        let memory_key = memory_id.to_string();
        let mut entity_ids = Vec::new();
        let mut seen_entities = HashSet::new();

        if let Some(existing_entities) = data.memory_entities.remove(&memory_key) {
            for entity_id in existing_entities {
                if let Some(entity) = data.entities.get_mut(&entity_id) {
                    entity.memory_count = entity.memory_count.saturating_sub(1);
                }
            }
        }
        data.relations.retain(|_, relation| {
            !relation
                .provenance
                .iter()
                .any(|provenance| provenance == memory_id)
        });

        for entity in extracted {
            let entity_id = data.upsert_entity(entity, &memory_key, metadata, now);
            if seen_entities.insert(entity_id.clone()) {
                entity_ids.push(entity_id);
            }
        }

        let mut created_relations = Vec::new();
        let co_occurrence_score = extraction::co_occurrence_score(entity_ids.len());
        for i in 0..entity_ids.len() {
            for j in (i + 1)..entity_ids.len() {
                let source = entity_ids[i].clone();
                let target = entity_ids[j].clone();
                let relation = data.upsert_relation(RelationUpsert {
                    source: &source,
                    target: &target,
                    relation_type: "co_occurs_with",
                    weight: co_occurrence_score,
                    co_occurrence_score,
                    memory_id: Some(memory_id),
                    now,
                });
                created_relations.push(relation.clone());
                created_relations.push(data.upsert_relation(RelationUpsert {
                    source: &target,
                    target: &source,
                    relation_type: "co_occurs_with",
                    weight: co_occurrence_score,
                    co_occurrence_score,
                    memory_id: Some(memory_id),
                    now,
                }));
            }
        }

        for raw_relation in extraction::extract_relation_candidates(content) {
            let Some(source_id) = data.resolve_entity_id(&raw_relation.source) else {
                continue;
            };
            let Some(target_id) = data.resolve_entity_id(&raw_relation.target) else {
                continue;
            };
            let relation = data.upsert_relation(RelationUpsert {
                source: &source_id,
                target: &target_id,
                relation_type: &raw_relation.relation_type,
                weight: raw_relation.score,
                co_occurrence_score: 0.0,
                memory_id: Some(memory_id),
                now,
            });
            created_relations.push(relation);
        }

        if let Some(metadata) = metadata {
            if let Some(description) = metadata.get("description").and_then(|value| value.as_str())
            {
                if let Some(first_id) = entity_ids.first() {
                    if let Some(entity) = data.entities.get_mut(first_id) {
                        if entity.description.is_none() {
                            entity.description = Some(description.to_string());
                        }
                    }
                }
            }
        }

        data.memory_entities
            .insert(memory_key.clone(), entity_ids.iter().cloned().collect());
        data.rebuild_indexes();

        let relations: Vec<_> = created_relations
            .into_iter()
            .filter(|relation| relation.provenance.iter().any(|item| item == memory_id))
            .collect();
        Ok(GraphRelationsView {
            entity_id: data
                .memory_entities
                .get(&memory_key)
                .and_then(|set| set.iter().next().cloned()),
            direction: GraphDirection::Both,
            max_depth: 1,
            total_relations: relations.len(),
            relations,
            traversal: Vec::new(),
        })
    }

    fn traverse_locked(
        data: &GraphData,
        start_entity: &str,
        max_depth: usize,
        relation_types: Option<&[String]>,
        direction: GraphDirection,
    ) -> Vec<TraversalStep> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::from([(
            start_entity.to_string(),
            0usize,
            vec![start_entity.to_string()],
        )]);
        let mut steps = Vec::new();

        while let Some((entity_id, depth, path)) = queue.pop_front() {
            if depth >= max_depth || !visited.insert((entity_id.clone(), depth)) {
                continue;
            }

            for relation in Self::relations_from_locked(data, &entity_id, relation_types, direction)
            {
                let next = if relation.source == entity_id {
                    relation.target.clone()
                } else {
                    relation.source.clone()
                };
                let mut next_path = path.clone();
                next_path.push(next.clone());
                steps.push(TraversalStep {
                    from: relation.source.clone(),
                    to: relation.target.clone(),
                    relation_type: relation.relation_type.clone(),
                    depth,
                    weight: relation.weight,
                    path: next_path.clone(),
                });
                queue.push_back((next, depth + 1, next_path));
            }
        }

        steps
    }

    fn relations_from_locked(
        data: &GraphData,
        entity_id: &str,
        relation_types: Option<&[String]>,
        direction: GraphDirection,
    ) -> Vec<EntityRelationRecord> {
        match direction {
            GraphDirection::Outgoing => {
                Self::relations_from_ids_locked(data, entity_id, relation_types, false)
            }
            GraphDirection::Incoming => {
                Self::relations_from_ids_locked(data, entity_id, relation_types, true)
            }
            GraphDirection::Both => {
                let mut relations =
                    Self::relations_from_ids_locked(data, entity_id, relation_types, false);
                relations.extend(Self::relations_from_ids_locked(
                    data,
                    entity_id,
                    relation_types,
                    true,
                ));
                relations
            }
        }
    }

    fn relations_from_ids_locked(
        data: &GraphData,
        entity_id: &str,
        relation_types: Option<&[String]>,
        incoming: bool,
    ) -> Vec<EntityRelationRecord> {
        let keys = if incoming {
            data.incoming.get(entity_id)
        } else {
            data.outgoing.get(entity_id)
        };
        let Some(keys) = keys else {
            return Vec::new();
        };

        keys.iter()
            .filter_map(|other_id| {
                data.relations.values().find(|relation| {
                    if incoming {
                        relation.source == *other_id && relation.target == entity_id
                    } else {
                        relation.source == entity_id && relation.target == *other_id
                    }
                })
            })
            .filter(|relation| {
                relation_types
                    .map(|allowed| allowed.iter().any(|item| item == &relation.relation_type))
                    .unwrap_or(true)
            })
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn indexes_entities_relations_and_traversal() {
        let graph = EntityGraph::new();
        let view = graph
            .upsert_memory(
                "memory-1",
                "BELA works at SWAL and knows Leonardo in Bogota.",
                None,
            )
            .await
            .expect("test assertion");

        assert!(view.total_relations > 0);
        let bela = graph.entity("BELA").await.expect("entity should exist");
        let neighbors = graph
            .entity_neighbors(&bela.id, 2, None, GraphDirection::Both)
            .await
            .expect("test assertion");
        assert_eq!(neighbors.entity.id, bela.id);
        assert!(!neighbors.traversal.is_empty());
    }

    #[tokio::test]
    async fn merges_entities_and_preserves_aliases() {
        let graph = EntityGraph::new();
        graph
            .upsert_memory("memory-1", "Alice works at Acme", None)
            .await
            .expect("test assertion");
        graph
            .upsert_memory("memory-2", "Alicia knows Bob", None)
            .await
            .expect("test assertion");

        let entities = graph.all_entities().await;
        assert!(!entities.is_empty());
        let primary = graph
            .merge_entities("Alice", "Alicia")
            .await
            .expect("test assertion");
        assert!(primary
            .aliases
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case("Alicia")));
    }

    #[tokio::test]
    async fn serialization_roundtrip() {
        let graph = EntityGraph::new();
        graph
            .upsert_memory("memory-1", "Alice works at Acme", None)
            .await
            .unwrap();

        // JSON
        let json = graph.export_json().await.unwrap();
        let graph2 = EntityGraph::new();
        graph2.import_json(&json).await.unwrap();
        assert_eq!(graph2.all_entities().await.len(), 2);

        // Bincode
        let bytes = graph.export_bincode().await.unwrap();
        let graph3 = EntityGraph::new();
        graph3.import_bincode(&bytes).await.unwrap();
        assert_eq!(graph3.all_entities().await.len(), 2);
    }

    #[tokio::test]
    async fn test_decay() {
        let graph = EntityGraph::new();
        graph
            .upsert_memory("memory-1", "Alice works at Acme", None)
            .await
            .unwrap();

        let initial_relations = graph.all_relations().await;
        assert!(!initial_relations.is_empty());
        let initial_weight = initial_relations[0].weight;

        // Manually adjust updated_at back in time
        {
            let mut data = graph.inner.write().await;
            for r in data.relations.values_mut() {
                r.updated_at = Utc::now() - chrono::Duration::hours(24);
            }
            for e in data.entities.values_mut() {
                e.last_seen = Utc::now() - chrono::Duration::hours(24);
            }
        }

        // Apply 10% hourly decay
        graph.apply_decay(0.1).await.unwrap();

        let decayed_relations = graph.all_relations().await;
        assert!(decayed_relations[0].weight < initial_weight);
    }

    #[tokio::test]
    async fn test_inference() {
        let graph = EntityGraph::new();

        // (Alice works_at Acme) AND (Acme located_in London) => (Alice located_in London)
        graph
            .upsert_memory("m1", "Alice works at Acme", None)
            .await
            .unwrap();
        graph
            .upsert_memory("m2", "Acme located in London", None)
            .await
            .unwrap();

        let inferred = graph.run_inference().await.unwrap();
        assert!(!inferred.is_empty());

        let data = graph.inner.read().await;
        let alice_london = inferred
            .iter()
            .find(|r| r.source_name(&data) == "alice" && r.target_name(&data) == "london");
        assert!(alice_london.is_some());
    }
}
