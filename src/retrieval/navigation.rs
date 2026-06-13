//! Navigation Policy for intelligent memory traversal
//!
//! Implements scoring for graph transitions based on multiple signals:
//! cosine similarity, edge confidence, node importance, context relevance,
//! cross-layer/dir bonuses, and peripheral-hub boosts.

use crate::domain::memory::belief::BeliefEdge;
use super::policy::{NavigationPolicy, NavigationScore};

impl NavigationPolicy {
    /// Scores a transition (edge) from a current node towards a target given a query.
    pub fn score_transition(
        &self,
        query: &str,
        edge: &BeliefEdge,
        now: chrono::DateTime<chrono::Utc>,
        source_degree: usize,
        target_degree: usize,
    ) -> f32 {
        let score_components = self.calculate_score_components(query, edge, now, source_degree, target_degree);

        (score_components.semantic_similarity * self.traversal_weights.semantic_similarity) +
        (score_components.confidence * self.traversal_weights.confidence) +
        (score_components.edge_weight * self.traversal_weights.edge_weight) +
        (score_components.recency * self.traversal_weights.recency) +
        (score_components.cross_layer * self.traversal_weights.cross_layer) +
        (score_components.cross_dir * self.traversal_weights.cross_dir) +
        (score_components.peripheral_hub * self.traversal_weights.peripheral_hub)
    }

    /// Decomposes the transition into its constituent score signals.
    pub fn calculate_score_components(
        &self,
        query: &str,
        edge: &BeliefEdge,
        now: chrono::DateTime<chrono::Utc>,
        source_degree: usize,
        target_degree: usize,
    ) -> NavigationScore {
        // [G3] File-type aware edge filtering: edges INFERRED that cross language families receive score 0
        if edge.is_inferred {
            if let (Some(src_lang), Some(tgt_lang)) = (&edge.source_language, &edge.target_language) {
                if src_lang != tgt_lang {
                    return NavigationScore {
                        semantic_similarity: 0.0,
                        confidence: 0.0,
                        edge_weight: 0.0,
                        recency: 0.0,
                        cross_layer: 0.0,
                        cross_dir: 0.0,
                        peripheral_hub: 0.0,
                    };
                }
            }
        }

        let query_lower = query.to_lowercase();
        let _source_lower = edge.source.to_lowercase();
        let target_lower = edge.target.to_lowercase();
        let relation_lower = edge.relation_type.to_lowercase();

        // 1. Semantic similarity
        let mut similarity = 0.0_f32;
        if target_lower.contains(&query_lower) || query_lower.contains(&target_lower) {
            similarity = 1.0;
        } else {
            let query_terms: Vec<&str> = query_lower.split_whitespace().collect();
            let mut matches = 0;
            for term in &query_terms {
                if target_lower.contains(term) || relation_lower.contains(term) {
                    matches += 1;
                }
            }
            if !query_terms.is_empty() {
                similarity = matches as f32 / query_terms.len() as f32;
            }
        }

        // 2. Confidence score
        let confidence = edge.confidence_score;

        // 3. Edge weight
        let weight = edge.weight;

        // 4. Recency (Sigmoid-based decay)
        let age_hours = (now - edge.updated_at).num_hours() as f32;
        // sigmoid(x) = 1 / (1 + e^x)
        // We want 1.0 at age=0 and decay slowly.
        // x = (age - center) / scale
        // For 1 week (168h) center, scale=40
        let recency = 1.0 / (1.0 + ( (age_hours - 72.0) / 48.0 ).exp());

        // 5. Cross-layer bonus (working <-> episodic <-> semantic)
        // Heuristic: check provenance_id or naming patterns
        let mut cross_layer = 0.0;
        let is_semantic = edge.source.starts_with("concept:") || edge.target.starts_with("concept:") || edge.provenance_id == "semantic";
        let is_episodic = edge.provenance_id.starts_with("session") || edge.source.contains("session") || edge.target.contains("session");

        if is_semantic && is_episodic {
            cross_layer = 1.0;
        } else if (is_semantic || is_episodic) && !edge.provenance_id.contains("session") && !edge.provenance_id.contains("semantic") {
            // Likely working -> semantic or working -> episodic
            cross_layer = 0.8;
        }

        // 6. Cross-directory bonus
        let mut cross_dir = 0.0;
        if let (Some(s_dir), Some(t_dir)) = (get_parent_dir(&edge.source), get_parent_dir(&edge.target)) {
            if s_dir == t_dir && !s_dir.is_empty() {
                cross_dir = 1.0;
            } else if s_dir.starts_with(&t_dir) || t_dir.starts_with(&s_dir) {
                cross_dir = 0.5;
            }
        }

        // 7. Peripheral -> Hub bonus
        let mut peripheral_hub = 0.0;
        if source_degree <= 3 && target_degree >= 10 {
            peripheral_hub = 1.2; // Extra boost for major hubs
        } else if source_degree <= 2 && target_degree >= 5 {
            peripheral_hub = 1.0;
        }

        NavigationScore {
            semantic_similarity: similarity,
            confidence,
            edge_weight: weight,
            recency,
            cross_layer,
            cross_dir,
            peripheral_hub,
        }
    }
}

fn get_parent_dir(path: &str) -> Option<String> {
    if path.contains('/') || path.contains('\\') {
        let normalized = path.replace('\\', "/");
        let parts: Vec<&str> = normalized.split('/').collect();
        if parts.len() > 1 {
            return Some(parts[..parts.len()-1].join("/"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::memory::belief::BeliefEdge;
    use super::super::gating::LayerWeights;
    use super::super::policy::TraversalWeights;

    #[test]
    fn test_score_transition_basic() {
        let policy = NavigationPolicy::new(
            LayerWeights::default(),
            TraversalWeights::default(),
            0.01
        );
        let edge = BeliefEdge::new(
            "Xavier".to_string(),
            "Rust".to_string(),
            "written_in".to_string(),
            0.9,
            "provenance".to_string(),
        );
        let score = policy.score_transition("Rust", &edge, chrono::Utc::now(), 1, 1);
        assert!(score > 0.0);
    }

    #[test]
    fn test_sigmoid_recency() {
        let policy = NavigationPolicy::new(
            LayerWeights::default(),
            TraversalWeights::default(),
            0.01
        );
        let mut edge = BeliefEdge::new(
            "A".to_string(),
            "B".to_string(),
            "rel".to_string(),
            0.5,
            "prov".to_string(),
        );

        let now = chrono::Utc::now();
        let components_now = policy.calculate_score_components("query", &edge, now, 1, 1);

        edge.updated_at = now - chrono::Duration::hours(72);
        let components_72h = policy.calculate_score_components("query", &edge, now, 1, 1);

        edge.updated_at = now - chrono::Duration::hours(240);
        let components_old = policy.calculate_score_components("query", &edge, now, 1, 1);

        assert!(components_now.recency > components_72h.recency);
        assert!(components_72h.recency > components_old.recency);
        // At 72h (center), it should be approx 0.5
        assert!((components_72h.recency - 0.5).abs() < 0.1);
    }

    #[test]
    fn test_cross_dir_bonus() {
        let policy = NavigationPolicy::new(
            LayerWeights::default(),
            TraversalWeights::default(),
            0.01
        );
        let edge_same = BeliefEdge::new(
            "src/main.rs".to_string(),
            "src/lib.rs".to_string(),
            "imports".to_string(),
            0.9,
            "prov".to_string(),
        );
        let edge_diff = BeliefEdge::new(
            "src/main.rs".to_string(),
            "tests/test.rs".to_string(),
            "tests".to_string(),
            0.9,
            "prov".to_string(),
        );

        let now = chrono::Utc::now();
        let comp_same = policy.calculate_score_components("query", &edge_same, now, 1, 1);
        let comp_diff = policy.calculate_score_components("query", &edge_diff, now, 1, 1);

        assert_eq!(comp_same.cross_dir, 1.0);
        assert_eq!(comp_diff.cross_dir, 0.0);
    }

    #[test]
    fn test_score_transition_cross_language_inferred() {
        let policy = NavigationPolicy::with_defaults();
        let mut edge = BeliefEdge::new(
            "Xavier".to_string(),
            "Rust".to_string(),
            "written_in".to_string(),
            0.9,
            "provenance".to_string(),
        );
        edge.is_inferred = true;
        edge.source_language = Some("python".to_string());
        edge.target_language = Some("rust".to_string());

        let score = policy.score_transition("Rust", &edge, chrono::Utc::now());
        assert_eq!(score, 0.0, "Inferred cross-language edge should have score 0.0");

        // Same language family - should have normal score
        edge.source_language = Some("rust".to_string());
        let score = policy.score_transition("Rust", &edge, chrono::Utc::now());
        assert!(score > 0.0);
    }
}
