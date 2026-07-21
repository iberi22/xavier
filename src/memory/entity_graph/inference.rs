//! Inference engine for the entity graph.
//!
//! Implements rule-based inference to derive new relationships
//! from the existing graph structure, such as transitivity and
//! inheritance.

use super::storage::{GraphData, RelationUpsert};
use super::types::EntityRelationRecord;
use chrono::Utc;

pub struct InferenceEngine;

impl InferenceEngine {
    /// Run.
    pub fn run(data: &mut GraphData) -> Vec<EntityRelationRecord> {
        let mut inferred = Vec::new();
        let now = Utc::now();

        // 1. Transitivity for "is_a" and "part_of"
        let transitivity_types = ["is_a", "part_of"];
        for rel_type in transitivity_types {
            let relations: Vec<_> = data
                .relations
                .values()
                .filter(|r| r.relation_type == rel_type)
                .cloned()
                .collect();

            for r1 in &relations {
                for r2 in &relations {
                    if r1.target == r2.source && r1.source != r2.target {
                        // Potential inference: r1.source -> rel_type -> r2.target
                        let exists = data.relations.values().any(|r| {
                            r.source == r1.source
                                && r.target == r2.target
                                && r.relation_type == rel_type
                        });

                        if !exists {
                            let weight = r1.confidence_score * r2.confidence_score * 0.8;
                            let new_rel = data.upsert_relation(RelationUpsert {
                                source: &r1.source,
                                target: &r2.target,
                                relation_type: rel_type,
                                weight,
                                co_occurrence_score: 0.0,
                                memory_id: None,
                                now,
                            });
                            inferred.push(new_rel);
                        }
                    }
                }
            }
        }

        // 2. Inheritance: (A works_at B) AND (B located_in C) => (A located_in C)
        let works_at_relations: Vec<_> = data
            .relations
            .values()
            .filter(|r| r.relation_type == "works_at")
            .cloned()
            .collect();
        let located_in_relations: Vec<_> = data
            .relations
            .values()
            .filter(|r| r.relation_type == "located_in")
            .cloned()
            .collect();

        for r1 in &works_at_relations {
            for r2 in &located_in_relations {
                if r1.target == r2.source {
                    let exists = data.relations.values().any(|r| {
                        r.source == r1.source
                            && r.target == r2.target
                            && r.relation_type == "located_in"
                    });

                    if !exists {
                        let weight = r1.confidence_score * r2.confidence_score * 0.9;
                        let new_rel = data.upsert_relation(RelationUpsert {
                            source: &r1.source,
                            target: &r2.target,
                            relation_type: "located_in",
                            weight,
                            co_occurrence_score: 0.0,
                            memory_id: None,
                            now,
                        });
                        inferred.push(new_rel);
                    }
                }
            }
        }

        inferred
    }
}
