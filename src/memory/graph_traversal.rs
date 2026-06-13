//! Memory graph traversal algorithms
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::domain::memory::belief::BeliefEdge;
use crate::memory::belief_graph::BeliefGraph;
use crate::retrieval::navigation::NavigationPolicy;
use serde::{Deserialize, Serialize};
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};

/// Represents a node affected by a change, including the path taken to reach it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AffectedNode {
    pub node: String,
    pub relation: String,
    pub depth: usize,
}

/// A utility for traversing the belief graph using various algorithms.
pub struct Pathfinder<'a> {
    /// The original graph. Kept for constructor consistency and potential future extensions
    /// that might need to re-query the full graph beyond the initial adjacency map.
    #[allow(dead_code)]
    graph: &'a BeliefGraph,
    policy: Option<NavigationPolicy>,
    adjacency_map: HashMap<String, Vec<BeliefEdge>>,
}

/// A wrapper for a belief edge with its associated policy-based score.
#[derive(Clone)]
pub struct ScoredEdge {
    pub edge: BeliefEdge,
    pub policy_score: f32,
}

impl<'a> Pathfinder<'a> {
    /// Creates a new Pathfinder instance.
    pub fn new(graph: &'a BeliefGraph) -> Self {
        let relations = graph.get_relations();
        let mut adjacency_map: HashMap<String, Vec<BeliefEdge>> = HashMap::new();
        for relation in relations {
            adjacency_map
                .entry(relation.source.clone())
                .or_default()
                .push(relation);
        }

        Self {
            graph,
            policy: None,
            adjacency_map,
        }
    }

    /// Creates a new Pathfinder instance with a specific navigation policy.
    pub fn with_policy(graph: &'a BeliefGraph, policy: NavigationPolicy) -> Self {
        let mut pathfinder = Self::new(graph);
        pathfinder.policy = Some(policy);
        pathfinder
    }

    /// Finds the shortest path between two concepts using BFS.
    /// Returns a list of relations forming the path.
    pub fn shortest_path(&self, start: &str, end: &str) -> Vec<BeliefEdge> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        queue.push_back((start.to_string(), Vec::new()));
        visited.insert(start.to_string());

        while let Some((current, path)) = queue.pop_front() {
            if current == end {
                return path;
            }

            if let Some(relations) = self.adjacency_map.get(&current) {
                for relation in relations {
                    if !visited.contains(&relation.target) {
                        visited.insert(relation.target.clone());
                        let mut new_path = path.clone();
                        new_path.push(relation.clone());
                        queue.push_back((relation.target.clone(), new_path));
                    }
                }
            }
        }

        Vec::new()
    }

    /// Performs a k-hop expansion from a start concept.
    /// Returns all relations within k hops.
    pub fn k_hop_expansion(&self, start: &str, k: usize) -> Vec<BeliefEdge> {
        let mut result = Vec::new();
        let mut visited_nodes = HashSet::new();
        let mut visited_relations = HashSet::new();
        let mut current_layer = HashSet::new();

        current_layer.insert(start.to_string());
        visited_nodes.insert(start.to_string());

        for _ in 0..k {
            let mut next_layer = HashSet::new();
            for current in current_layer {
                if let Some(relations) = self.adjacency_map.get(&current) {
                    for relation in relations {
                        if visited_relations.insert(relation.id.clone()) {
                            result.push(relation.clone());
                        }
                        if visited_nodes.insert(relation.target.clone()) {
                            next_layer.insert(relation.target.clone());
                        }
                    }
                }
            }
            if next_layer.is_empty() {
                break;
            }
            current_layer = next_layer;
        }

        result
    }

    /// Finds all possible paths from start to end up to max_depth.
    pub fn all_paths(&self, start: &str, end: &str, max_depth: usize) -> Vec<Vec<BeliefEdge>> {
        let mut results = Vec::new();
        self.find_all_paths_recursive(start, end, max_depth, Vec::new(), &mut results);
        results
    }

    fn find_all_paths_recursive(
        &self,
        current: &str,
        end: &str,
        depth_left: usize,
        current_path: Vec<BeliefEdge>,
        results: &mut Vec<Vec<BeliefEdge>>,
    ) {
        if current == end {
            if !current_path.is_empty() {
                results.push(current_path);
            }
            return;
        }

        if depth_left == 0 {
            return;
        }

        if let Some(relations) = self.adjacency_map.get(current) {
            for relation in relations {
                if !current_path.iter().any(|r| r.target == relation.target) {
                    let mut next_path = current_path.clone();
                    next_path.push(relation.clone());
                    self.find_all_paths_recursive(
                        &relation.target,
                        end,
                        depth_left - 1,
                        next_path,
                        results,
                    );
                }
            }
        }
    }

    /// Performs a guided search from a start concept using the navigation policy.
    /// Uses a priority queue to explore paths with higher cumulative scores first.
    pub fn guided_search(&self, start: &str, query: &str, max_depth: usize) -> Vec<ScoredEdge> {
        let mut result = Vec::new();
        let mut visited_nodes = HashSet::new();
        let mut visited_relations = HashSet::new();
        let mut priority_queue = BinaryHeap::new();

        let now = chrono::Utc::now();
        let policy = self
            .policy
            .as_ref()
            .cloned()
            .unwrap_or_else(NavigationPolicy::with_defaults);

        #[derive(PartialEq)]
        struct NodeState {
            score: f32,
            concept: String,
            depth: usize,
        }
        impl Eq for NodeState {}
        impl PartialOrd for NodeState {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                self.score.partial_cmp(&other.score)
            }
        }
        impl Ord for NodeState {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                self.score
                    .partial_cmp(&other.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }
        }

        priority_queue.push(NodeState {
            score: 1.0,
            concept: start.to_string(),
            depth: 0,
        });
        visited_nodes.insert(start.to_string());

        while let Some(NodeState {
            score: current_score,
            concept: current,
            depth,
        }) = priority_queue.pop()
        {
            if depth >= max_depth {
                continue;
            }

            if let Some(relations) = self.adjacency_map.get(&current) {
                for relation in relations {
                    // Mark relation as visited at push time, not pop time, to avoid
                    // expanding the same relation via a different parent path with a lower score.
                    if !visited_relations.contains(&relation.id) {
                        visited_relations.insert(relation.id.clone());
                        let transition_score = policy.score_transition(query, relation, now);
                        let combined_score = current_score * transition_score;

                        // Threshold to prune low-relevance paths
                        if combined_score > 0.1 && !visited_nodes.contains(&relation.target) {
                            visited_nodes.insert(relation.target.clone());
                            result.push(ScoredEdge {
                                edge: relation.clone(),
                                policy_score: combined_score,
                            });
                            priority_queue.push(NodeState {
                                score: combined_score,
                                concept: relation.target.clone(),
                                depth: depth + 1,
                            });
                        }
                    }
                }
            }
        }

        // Sort final results by policy score descending
        result.sort_by(|a, b| {
            b.policy_score
                .partial_cmp(&a.policy_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        result
    }

    /// Performs a depth-limited BFS to find nodes affected by a change at `start_node`.
    pub fn affected_bfs(&self, start_node: &str, max_depth: usize) -> Vec<AffectedNode> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        queue.push_back((start_node.to_string(), "root".to_string(), 0));
        visited.insert(start_node.to_string());

        while let Some((current, relation_type, depth)) = queue.pop_front() {
            if depth > 0 {
                result.push(AffectedNode {
                    node: current.clone(),
                    relation: relation_type,
                    depth,
                });
            }

            if depth < max_depth {
                if let Some(relations) = self.adjacency_map.get(&current) {
                    for relation in relations {
                        if !visited.contains(&relation.target) {
                            visited.insert(relation.target.clone());
                            queue.push_back((
                                relation.target.clone(),
                                relation.relation_type.clone(),
                                depth + 1,
                            ));
                        }
                    }
                }
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pathfinding() {
        let graph = BeliefGraph::new();
        graph.add_node("A".to_string(), 1.0, None);
        graph.add_node("B".to_string(), 1.0, None);
        graph.add_node("C".to_string(), 1.0, None);
        graph.add_node("D".to_string(), 1.0, None);

        graph
            .add_relation(
                "A".to_string(),
                "B".to_string(),
                "related_to".to_string(),
                Some("mem1".to_string()),
                None,
            )
            .await
            .unwrap();
        graph
            .add_relation(
                "B".to_string(),
                "C".to_string(),
                "related_to".to_string(),
                Some("mem2".to_string()),
                None,
            )
            .await
            .unwrap();
        graph
            .add_relation(
                "A".to_string(),
                "D".to_string(),
                "related_to".to_string(),
                Some("mem3".to_string()),
                None,
            )
            .await
            .unwrap();
        graph
            .add_relation(
                "D".to_string(),
                "C".to_string(),
                "related_to".to_string(),
                Some("mem4".to_string()),
                None,
            )
            .await
            .unwrap();

        let pathfinder = Pathfinder::new(&graph);

        let shortest = pathfinder.shortest_path("A", "C");
        assert_eq!(shortest.len(), 2);
        assert_eq!(shortest[0].source, "A");
        assert_eq!(shortest[0].target, "B");
        assert_eq!(shortest[1].source, "B");
        assert_eq!(shortest[1].target, "C");

        let expansion = pathfinder.k_hop_expansion("A", 1);
        assert_eq!(expansion.len(), 2);

        let all = pathfinder.all_paths("A", "C", 3);
        assert_eq!(all.len(), 2);

        let affected = pathfinder.affected_bfs("A", 2);
        assert_eq!(affected.len(), 3); // B, D, C (via B or D)
        assert!(affected.iter().any(|a| a.node == "B" && a.depth == 1));
        assert!(affected.iter().any(|a| a.node == "D" && a.depth == 1));
        assert!(affected.iter().any(|a| a.node == "C" && a.depth == 2));
    }

    #[tokio::test]
    async fn test_guided_search() {
        let graph = BeliefGraph::new();
        graph.add_node("A".to_string(), 1.0, None);
        graph.add_node("B".to_string(), 1.0, None);
        graph.add_node("D".to_string(), 1.0, None);

        graph
            .add_relation(
                "A".to_string(),
                "B".to_string(),
                "rust_expert".to_string(),
                Some("mem1".to_string()),
                None,
            )
            .await
            .unwrap();
        graph
            .add_relation(
                "A".to_string(),
                "D".to_string(),
                "cooks_pasta".to_string(),
                Some("mem2".to_string()),
                None,
            )
            .await
            .unwrap();

        let pathfinder = Pathfinder::new(&graph);
        let results = pathfinder.guided_search("A", "Rust", 1);

        assert!(!results.is_empty());
        assert!(results.iter().any(|r| r.edge.target == "B"));
        assert!(results[0].edge.target == "B");
        assert!(results[0].policy_score > 0.5);
    }
}
