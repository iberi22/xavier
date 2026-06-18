//! Belief Graph - conceptual graph used by the Xavier reasoning layers.

use aho_corasick::AhoCorasick;
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, RwLock};
use tokio::sync::RwLock as AsyncRwLock;
use tracing::info;

use crate::agents::belief_evaluator::BeliefEvaluator;
use crate::domain::memory::belief::{BeliefEdge, BeliefNode};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Confidence {
    High,
    Medium,
    Low,
}

impl Confidence {
    pub fn score(self) -> f32 {
        match self {
            Self::High => 0.9,
            Self::Medium => 0.6,
            Self::Low => 0.3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Belief {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub confidence: Confidence,
}

impl Belief {
    pub fn new(subject: String, predicate: String, object: String, confidence: Confidence) -> Self {
        Self {
            subject,
            predicate,
            object,
            confidence,
        }
    }
}

/// Thread-safe belief graph that exposes both sync and async-friendly helpers.
#[derive(Debug)]
pub struct BeliefGraph {
    nodes: RwLock<HashMap<String, BeliefNode>>,
    edges: RwLock<Vec<BeliefEdge>>,
    adjacency: RwLock<HashMap<String, HashSet<String>>>,
    evaluator: BeliefEvaluator,
}

impl BeliefGraph {
    pub fn new() -> Self {
        Self {
            nodes: RwLock::new(HashMap::new()),
            edges: RwLock::new(Vec::new()),
            adjacency: RwLock::new(HashMap::new()),
            evaluator: BeliefEvaluator::new(),
        }
    }

    pub fn add_node(&self, concept: String, confidence: f32, language_family: Option<String>) {
        use std::str::FromStr;
        use crate::memory::qmd::NormalizedId;
        let normalized = NormalizedId::from_str(&concept).unwrap_or_else(|_| NormalizedId::from_str_unchecked(&concept));
        let concept_norm = normalized.to_string();

        if self.get_node(&concept_norm).is_some() {
            return;
        }

        let id = ulid::Ulid::new().to_string();
        let node = BeliefNode {
            id: id.clone(),
            concept: concept_norm.clone(),
            confidence,
            language_family,
            created_at: Utc::now(),
        };

        self.nodes
            .write()
            .expect("belief_graph: nodes write lock poisoned")
            .insert(id, node);
        self.adjacency
            .write()
            .expect("belief_graph: adjacency write lock poisoned")
            .entry(concept_norm.clone())
            .or_default();

        info!("Added node: {}", concept_norm);
    }

    pub async fn add_edge(&self, from: String, to: String, relation: String) {
        let _ = self.add_relation(from, to, relation, None, None).await;
    }

    pub async fn add_relation(
        &self,
        source: String,
        target: String,
        relation_type: String,
        provenance_id: Option<String>,
        source_type: Option<&str>,
    ) -> Result<()> {
        use std::str::FromStr;
        use crate::memory::qmd::NormalizedId;
        let source_norm = NormalizedId::from_str(&source).unwrap_or_else(|_| NormalizedId::from_str_unchecked(&source)).to_string();
        let target_norm = NormalizedId::from_str(&target).unwrap_or_else(|_| NormalizedId::from_str_unchecked(&target)).to_string();

        let provenance_id = provenance_id.unwrap_or_else(|| "unknown".to_string());
        let confidence_score = self
            .evaluator
            .evaluate_confidence(source_type.unwrap_or("unknown"), &relation_type)
            .await;

        let lang_family = crate::memory::languages::get_language_family(&provenance_id);

        if self.get_node(&source_norm).is_none() {
            self.add_node(source_norm.clone(), 0.5, lang_family.clone());
        }
        if self.get_node(&target_norm).is_none() {
            self.add_node(target_norm.clone(), 0.5, lang_family.clone());
        }

        let mut new_edge = BeliefEdge::new(
            source_norm.clone(),
            target_norm.clone(),
            relation_type,
            confidence_score,
            provenance_id,
        );

        if source_type == Some("inference") {
            new_edge.is_inferred = true;
        }

        if let Some(src_node) = self.get_node(&source_norm) {
            new_edge.source_language = src_node.language_family;
        }
        if let Some(tgt_node) = self.get_node(&target_norm) {
            new_edge.target_language = tgt_node.language_family;
        }

        let existing_edges = self.get_edges_async().await;
        if let Some(contradicts_id) = self
            .evaluator
            .find_contradiction(&new_edge, &existing_edges)
        {
            new_edge.contradicts_edge_id = Some(contradicts_id);
            info!(
                "Contradiction detected for {} -> {} ({}). Adding competing belief.",
                source_norm, target_norm, new_edge.relation_type
            );
        }

        self.edges
            .write()
            .expect("belief_graph: edges write lock poisoned")
            .push(new_edge.clone());

        let mut adjacency = self
            .adjacency
            .write()
            .expect("belief_graph: adjacency write lock poisoned");
        adjacency
            .entry(source_norm.clone())
            .or_default()
            .insert(target_norm.clone());
        adjacency.entry(target_norm.clone()).or_default();

        info!(
            "Added relation: {} -> {} ({}) [confidence: {}]",
            source_norm, target_norm, new_edge.relation_type, confidence_score
        );

        Ok(())
    }

    pub fn get_related(&self, concept: &str) -> Vec<String> {
        use std::str::FromStr;
        use crate::memory::qmd::NormalizedId;
        let normalized = NormalizedId::from_str(concept).unwrap_or_else(|_| NormalizedId::from_str_unchecked(concept));
        let concept_norm = normalized.as_str();

        self.adjacency
            .read()
            .expect("belief_graph: adjacency read lock poisoned")
            .get(concept_norm)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn get_node(&self, concept: &str) -> Option<BeliefNode> {
        use std::str::FromStr;
        use crate::memory::qmd::NormalizedId;
        let normalized = NormalizedId::from_str(concept).unwrap_or_else(|_| NormalizedId::from_str_unchecked(concept));
        let concept_norm = normalized.as_str();

        self.nodes
            .read()
            .expect("belief_graph: nodes read lock poisoned")
            .values()
            .find(|node| node.concept == concept_norm)
            .cloned()
    }

    pub fn list_nodes(&self) -> Vec<BeliefNode> {
        self.nodes
            .read()
            .expect("belief_graph: nodes read lock poisoned")
            .values()
            .cloned()
            .collect()
    }

    pub fn get_edges(&self) -> Vec<BeliefEdge> {
        self.edges
            .read()
            .expect("belief_graph: edges write lock poisoned")
            .clone()
    }

    pub async fn get_edges_async(&self) -> Vec<BeliefEdge> {
        self.get_edges()
    }

    pub fn get_relations(&self) -> Vec<BeliefEdge> {
        self.get_edges()
    }

    pub fn replace_relations(&self, edges: Vec<BeliefEdge>) {
        use std::str::FromStr;
        use crate::memory::qmd::NormalizedId;

        let mut nodes = HashMap::new();
        let mut adjacency = HashMap::<String, HashSet<String>>::new();
        let mut normalized_edges = Vec::new();

        for mut edge in edges {
            let source_norm = NormalizedId::from_str(&edge.source).unwrap_or_else(|_| NormalizedId::from_str_unchecked(&edge.source)).to_string();
            let target_norm = NormalizedId::from_str(&edge.target).unwrap_or_else(|_| NormalizedId::from_str_unchecked(&edge.target)).to_string();

            edge.source = source_norm.clone();
            edge.target = target_norm.clone();

            nodes.entry(source_norm.clone()).or_insert(BeliefNode {
                id: source_norm.clone(),
                concept: source_norm.clone(),
                confidence: edge.confidence_score,
                language_family: edge.source_language.clone(),
                created_at: edge.created_at,
            });
            nodes.entry(target_norm.clone()).or_insert(BeliefNode {
                id: target_norm.clone(),
                concept: target_norm.clone(),
                confidence: edge.confidence_score,
                language_family: edge.target_language.clone(),
                created_at: edge.created_at,
            });

            adjacency
                .entry(source_norm)
                .or_default()
                .insert(target_norm);
            adjacency.entry(edge.target.clone()).or_default();

            normalized_edges.push(edge);
        }

        *self
            .nodes
            .write()
            .expect("belief_graph: nodes write lock poisoned") = nodes;
        *self
            .adjacency
            .write()
            .expect("belief_graph: adjacency write lock poisoned") = adjacency;
        *self
            .edges
            .write()
            .expect("belief_graph: edges write lock poisoned") = normalized_edges;
    }

    pub async fn add_belief(&self, belief: Belief, source_memory_id: Option<String>) -> Result<()> {
        let confidence_score = belief.confidence.score();
        let lang_family = source_memory_id
            .as_ref()
            .and_then(|id| crate::memory::languages::get_language_family(id));

        if self.get_node(&belief.subject).is_none() {
            self.add_node(belief.subject.clone(), confidence_score, lang_family.clone());
        }

        if self.get_node(&belief.object).is_none() {
            self.add_node(belief.object.clone(), confidence_score, lang_family.clone());
        }

        self.add_relation(
            belief.subject,
            belief.object,
            belief.predicate,
            source_memory_id,
            None,
        )
        .await
    }

    /// Returns the highest-confidence paths or multiple beliefs if ambiguity exists.
    pub async fn search(&self, query: &str) -> Vec<BeliefEdge> {
        let query_lower = query.to_lowercase();
        let words: Vec<_> = query_lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 2)
            .collect();

        if words.is_empty() {
            return Vec::new();
        }

        let mut results = self
            .get_edges()
            .into_iter()
            .filter(|edge| {
                let s = edge.source.to_lowercase();
                let t = edge.target.to_lowercase();
                let r = edge.relation_type.to_lowercase();

                words
                    .iter()
                    .any(|w| s.contains(w) || t.contains(w) || r.contains(w))
            })
            .collect::<Vec<_>>();

        results.sort_by(|a, b| {
            b.confidence_score
                .partial_cmp(&a.confidence_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results
    }

    pub async fn bfs(&self, start: &str) -> Vec<String> {
        use std::str::FromStr;
        use crate::memory::qmd::NormalizedId;
        let normalized = NormalizedId::from_str(start).unwrap_or_else(|_| NormalizedId::from_str_unchecked(start));
        let start_norm = normalized.to_string();

        let adjacency = self
            .adjacency
            .read()
            .expect("belief_graph: adjacency read lock poisoned")
            .clone();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::from([start_norm]);
        let mut ordered = Vec::new();

        while let Some(current) = queue.pop_front() {
            if !visited.insert(current.clone()) {
                continue;
            }

            if current != start {
                ordered.push(current.clone());
            }

            if let Some(neighbors) = adjacency.get(&current) {
                for neighbor in neighbors {
                    if !visited.contains(neighbor) {
                        queue.push_back(neighbor.clone());
                    }
                }
            }
        }

        ordered
    }

    /// Finds the highest-confidence path between two concepts.
    pub async fn find_highest_confidence_path(&self, start: &str, end: &str) -> Vec<BeliefEdge> {
        use std::str::FromStr;
        use crate::memory::qmd::NormalizedId;
        let start_norm = NormalizedId::from_str(start).unwrap_or_else(|_| NormalizedId::from_str_unchecked(start)).to_string();
        let end_norm = NormalizedId::from_str(end).unwrap_or_else(|_| NormalizedId::from_str_unchecked(end)).to_string();

        let edges = self.get_edges();
        let mut distances = HashMap::new();
        let mut previous = HashMap::new();
        let mut queue = HashSet::new();

        distances.insert(start_norm.clone(), 0.0f32);
        queue.insert(start_norm.clone());

        // Simple Dijkstra-like approach using confidence as weight (higher is better, so we use 1.0 - confidence as cost)
        while !queue.is_empty() {
            let current = queue
                .iter()
                .min_by(|a, b| {
                    let da = distances.get(*a).unwrap_or(&f32::INFINITY);
                    let db = distances.get(*b).unwrap_or(&f32::INFINITY);
                    da.partial_cmp(db).unwrap_or(std::cmp::Ordering::Equal)
                })
                .cloned()
                .expect("belief_graph: find_highest_confidence_path had empty queue");

            queue.remove(&current);

            if current == end_norm {
                break;
            }

            for edge in edges.iter().filter(|e| e.source == current) {
                let alt = distances.get(&current).unwrap_or(&f32::INFINITY)
                    + (1.0 - edge.confidence_score);
                if alt < *distances.get(&edge.target).unwrap_or(&f32::INFINITY) {
                    distances.insert(edge.target.clone(), alt);
                    previous.insert(edge.target.clone(), edge.clone());
                    queue.insert(edge.target.clone());
                }
            }
        }

        let mut path = Vec::new();
        let mut curr = end.to_string();
        while let Some(edge) = previous.get(&curr) {
            path.push(edge.clone());
            curr = edge.source.clone();
        }
        path.reverse();
        path
    }

    pub async fn has_supporting_beliefs(&self, memory_id: &str) -> bool {
        self.get_edges()
            .iter()
            .any(|e| e.provenance_id == memory_id)
    }

    /// Validates if a list of documents are grounded in the belief graph.
    /// Returns a list of (memory_id, is_grounded, explanation)
    pub async fn validate_grounding(
        &self,
        documents: &[crate::memory::qmd_memory::MemoryDocument],
        min_confidence: f32,
    ) -> Vec<(String, bool, String)> {
        let mut results = Vec::new();
        let edges = self.get_edges();
        let nodes = self.list_nodes();

        // Performance Optimization: Filter eligible nodes and build Aho-Corasick automaton
        let eligible_nodes: Vec<_> = nodes
            .into_iter()
            .filter(|n| n.confidence >= min_confidence)
            .collect();

        let ac = if !eligible_nodes.is_empty() {
            let patterns: Vec<String> = eligible_nodes
                .iter()
                .map(|n| n.concept.to_lowercase())
                .collect();
            AhoCorasick::new(patterns).ok()
        } else {
            None
        };

        for doc in documents {
            let memory_id = doc.id.clone().unwrap_or_else(|| doc.path.clone());

            // Check if this specific memory ID is a provenance for any belief
            let has_belief = edges.iter().any(|e| e.provenance_id == memory_id);

            if has_belief {
                results.push((
                    memory_id,
                    true,
                    "Directly grounded in belief graph".to_string(),
                ));
                continue;
            }

            // Semantic grounding: check if key terms in content match established nodes above threshold
            let mut matched_concepts = HashSet::new();
            if let Some(ref ac_idx) = ac {
                let content_lower = doc.content.to_lowercase();
                for mat in ac_idx.find_iter(&content_lower) {
                    matched_concepts
                        .insert(eligible_nodes[mat.pattern().as_usize()].concept.clone());
                }
            }

            if !matched_concepts.is_empty() {
                let mut node_names: Vec<_> = matched_concepts.into_iter().collect();
                node_names.sort();
                results.push((
                    memory_id,
                    true,
                    format!("Semantically grounded through concepts: {:?}", node_names),
                ));
            } else {
                results.push((
                    memory_id,
                    false,
                    "No supporting beliefs or nodes found in graph".to_string(),
                ));
            }
        }

        results
    }
}

impl Default for BeliefGraph {
    fn default() -> Self {
        Self::new()
    }
}

pub type SharedBeliefGraph = Arc<AsyncRwLock<BeliefGraph>>;

#[cfg(test)]
mod grounding_tests {
    use super::*;
    use crate::memory::qmd_memory::MemoryDocument;

    #[tokio::test]
    async fn test_validate_grounding_direct() {
        let graph = BeliefGraph::new();
        let memory_id = "mem-1".to_string();

        // Add a relation with provenance_id
        graph
            .add_relation(
                "Xavier".to_string(),
                "Memory".to_string(),
                "is_a".to_string(),
                Some(memory_id.clone()),
                None,
            )
            .await
            .unwrap();

        let docs = vec![MemoryDocument {
            id: Some(memory_id.clone()),
            content: "Something about Xavier".to_string(),
            ..Default::default()
        }];

        let results = graph.validate_grounding(&docs, 0.5).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, memory_id);
        assert!(results[0].1);
        assert!(results[0].2.contains("Directly grounded"));
    }

    #[tokio::test]
    async fn test_validate_grounding_semantic() {
        let graph = BeliefGraph::new();
        graph.add_node("Xavier".to_string(), 0.9, None);
        graph.add_node("Rust".to_string(), 0.4, None);

        let docs = vec![MemoryDocument {
            id: Some("doc-1".to_string()),
            content: "Xavier is written in Rust".to_string(),
            ..Default::default()
        }];

        // With min_confidence 0.5, only "Xavier" should match
        let results = graph.validate_grounding(&docs, 0.5).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].1);
        assert!(results[0].2.contains("xavier"));
        assert!(!results[0].2.contains("rust"));

        // With min_confidence 0.3, both should match
        let results = graph.validate_grounding(&docs, 0.3).await;
        assert!(results[0].2.contains("xavier"));
        assert!(results[0].2.contains("rust"));
    }

    #[tokio::test]
    async fn test_validate_grounding_no_match() {
        let graph = BeliefGraph::new();
        graph.add_node("Xavier".to_string(), 0.9, None);

        let docs = vec![MemoryDocument {
            id: Some("doc-1".to_string()),
            content: "Something unrelated".to_string(),
            ..Default::default()
        }];

        let results = graph.validate_grounding(&docs, 0.5).await;
        assert_eq!(results.len(), 1);
        assert!(!results[0].1);
    }

    #[tokio::test]
    async fn test_node_id_normalization() {
        let graph = BeliefGraph::new();
        graph.add_node("My Concept".to_string(), 0.9, None);
        graph.add_node("my_concept".to_string(), 0.8, None);

        let nodes = graph.list_nodes();
        // Should only have 1 node due to normalization
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].concept, "my_concept");
    }
}
