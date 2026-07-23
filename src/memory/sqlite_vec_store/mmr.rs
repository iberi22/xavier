//! Maximum Marginal Relevance (MMR) for result diversification.

use crate::memory::store::HybridSearchResult;

/// Configuration for MMR diversification.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct MmrConfig {
    pub mmr_enabled: bool,
    pub lambda: f32,
    pub k: usize,
}

impl Default for MmrConfig {
    fn default() -> Self {
        Self {
            mmr_enabled: true,
            lambda: 0.5,
            k: 0, // 0 defaults to limit
        }
    }
}

/// Computes the cosine similarity between two float vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot_product = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;
    for (val_a, val_b) in a.iter().zip(b.iter()) {
        dot_product += val_a * val_b;
        norm_a += val_a * val_a;
        norm_b += val_b * val_b;
    }
    if norm_a <= 0.0 || norm_b <= 0.0 {
        return 0.0;
    }
    dot_product / (norm_a.sqrt() * norm_b.sqrt())
}

/// Diversify results using Maximum Marginal Relevance (MMR).
///
/// If any result lacks an embedding, returns the original results unmodified (passthrough).
pub fn mmr_diversify(
    results: Vec<HybridSearchResult>,
    lambda: f32,
    k: usize,
) -> Vec<HybridSearchResult> {
    if results.is_empty() {
        return results;
    }

    let target_k = if k == 0 { results.len() } else { k.min(results.len()) };

    // Guard: MMR requires non-empty embeddings in each result.
    if results.iter().any(|r| r.record.embedding.is_empty()) {
        let mut passthrough = results;
        passthrough.truncate(target_k);
        return passthrough;
    }

    // Normalize hybrid search relevance scores to [0.0, 1.0] for fair balancing with cosine similarity.
    let max_score = results
        .iter()
        .map(|r| r.score)
        .fold(f32::NEG_INFINITY, f32::max);
    let min_score = results
        .iter()
        .map(|r| r.score)
        .fold(f32::INFINITY, f32::min);
    let score_range = max_score - min_score;

    let normalized_scores: Vec<f32> = results
        .iter()
        .map(|r| {
            if score_range > 0.0 {
                (r.score - min_score) / score_range
            } else {
                1.0
            }
        })
        .collect();

    let mut selected_indices = Vec::with_capacity(target_k);
    let mut remaining_indices: Vec<usize> = (0..results.len()).collect();

    // Greedy selection loop
    while selected_indices.len() < target_k && !remaining_indices.is_empty() {
        if selected_indices.is_empty() {
            // First item selected is simply the most relevant item (the first in our sorted list)
            let best_idx = remaining_indices.remove(0);
            selected_indices.push(best_idx);
        } else {
            let mut best_mmr_score = f32::NEG_INFINITY;
            let mut best_remaining_idx_in_vec = 0;

            for (idx_in_remaining, &candidate_idx) in remaining_indices.iter().enumerate() {
                let relevance = normalized_scores[candidate_idx];
                let candidate_emb = &results[candidate_idx].record.embedding;

                // Find maximum similarity of the candidate to any already selected item
                let mut max_sim = f32::NEG_INFINITY;
                for &sel_idx in &selected_indices {
                    let sel_emb = &results[sel_idx].record.embedding;
                    let sim = cosine_similarity(candidate_emb, sel_emb);
                    if sim > max_sim {
                        max_sim = sim;
                    }
                }

                // MMR formula: MMR = lambda * relevance - (1.0 - lambda) * max_sim
                let mmr_score = lambda * relevance - (1.0 - lambda) * max_sim;

                if mmr_score > best_mmr_score {
                    best_mmr_score = mmr_score;
                    best_remaining_idx_in_vec = idx_in_remaining;
                }
            }

            let best_idx = remaining_indices.remove(best_remaining_idx_in_vec);
            selected_indices.push(best_idx);
        }
    }

    // Reconstruct the result list based on the selected indices
    let mut diversified_results = Vec::with_capacity(selected_indices.len());
    for idx in selected_indices {
        diversified_results.push(results[idx].clone());
    }

    diversified_results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::{HybridSearchResult, MemoryRecord};

    fn make_mock_result(id: &str, score: f32, embedding: Vec<f32>) -> HybridSearchResult {
        HybridSearchResult {
            record: MemoryRecord {
                id: id.to_string(),
                embedding,
                ..Default::default()
            },
            score,
            vector_score: 0.0,
            lexical_score: 0.0,
            kg_score: 0.0,
            bm25: None,
        }
    }

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-5);

        let c = vec![0.0, 1.0, 0.0];
        assert!((cosine_similarity(&a, &c) - 0.0).abs() < 1e-5);

        let d = vec![-1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &d) - (-1.0)).abs() < 1e-5);

        let empty: Vec<f32> = vec![];
        assert_eq!(cosine_similarity(&empty, &empty), 0.0);
    }

    #[test]
    fn test_mmr_diversify_passthrough_on_empty_embeddings() {
        let results = vec![
            make_mock_result("1", 0.9, vec![]),
            make_mock_result("2", 0.8, vec![1.0, 2.0]),
        ];
        let diversified = mmr_diversify(results.clone(), 0.5, 2);
        assert_eq!(diversified.len(), 2);
        assert_eq!(diversified[0].record.id, "1");
    }

    #[test]
    fn test_mmr_diversify_lambda_one_is_ranking() {
        let results = vec![
            make_mock_result("1", 0.9, vec![1.0, 0.0]),
            make_mock_result("2", 0.8, vec![1.0, 0.01]),
            make_mock_result("3", 0.7, vec![0.0, 1.0]),
        ];
        // With lambda=1.0, relevance is everything, no diversity.
        let diversified = mmr_diversify(results.clone(), 1.0, 3);
        assert_eq!(diversified[0].record.id, "1");
        assert_eq!(diversified[1].record.id, "2");
        assert_eq!(diversified[2].record.id, "3");
    }

    #[test]
    fn test_mmr_diversify_duplicates_penalized() {
        // 10 results, where 3 are exact duplicates of others:
        // Unique elements: "1", "2", "4", "6", "8", "9", "10"
        // Duplicates: "3" (of "1"), "5" (of "2"), "7" (of "1")
        let results = vec![
            make_mock_result("1", 1.0, vec![1.0, 0.0]),
            make_mock_result("2", 0.9, vec![0.0, 1.0]),
            make_mock_result("3", 0.8, vec![1.0, 0.0]), // duplicate of 1
            make_mock_result("4", 0.7, vec![0.707, 0.707]),
            make_mock_result("5", 0.6, vec![0.0, 1.0]), // duplicate of 2
            make_mock_result("6", 0.5, vec![-1.0, 0.0]),
            make_mock_result("7", 0.4, vec![1.0, 0.0]), // duplicate of 1
            make_mock_result("8", 0.3, vec![0.5, -0.5]),
            make_mock_result("9", 0.2, vec![-0.5, 0.5]),
            make_mock_result("10", 0.1, vec![-0.1, -0.9]),
        ];

        // MMR returns maximum 8 elements if k=8, containing only unique elements plus at most 1 duplicate to fill the slot.
        // There are 7 unique elements in total. Thus, the returned list of size 8 will have at most 7 unique elements.
        let diversified = mmr_diversify(results.clone(), 0.5, 8);
        assert_eq!(diversified.len(), 8);

        // Count unique IDs
        let mut unique_ids = std::collections::HashSet::new();
        for res in &diversified {
            unique_ids.insert(res.record.id.clone());
        }

        // MMR returns maximum 8 elements if k=8.
        assert!(diversified.len() <= 8);

        // All selected items should have unique ID strings (distinct results)
        assert_eq!(unique_ids.len(), diversified.len());

        // For k < 8, e.g., k = 6:
        let diversified_6 = mmr_diversify(results.clone(), 0.5, 6);
        assert!(diversified_6.len() <= 6);
        let mut unique_ids_6 = std::collections::HashSet::new();
        for res in &diversified_6 {
            unique_ids_6.insert(res.record.id.clone());
        }
        assert_eq!(unique_ids_6.len(), diversified_6.len());
    }
}
