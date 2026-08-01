//! Reciprocal Rank Fusion for search combination
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct ScoredResult {
    pub id: String,
    pub content: String,
    pub score: f32,
    pub source: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub updated_at: Option<i64>, // Unix timestamp ms for deduplication
    #[serde(default)]
    pub zone: Option<String>,
}

#[derive(Clone, Debug)]
struct FusedScore {
    id: String,
    content: String,
    best_original_score: f32,
    source: String,
    path: String,
    updated_at: Option<i64>,
    zone: Option<String>,
    total_rrf: f32,
    total_weight: f32,
}

impl FusedScore {
    fn new(result: &ScoredResult, contribution: f32, weight: f32) -> Self {
        let best_original_score = result.score;
        Self {
            id: result.id.clone(),
            content: result.content.clone(),
            best_original_score,
            source: result.source.clone(),
            path: result.path.clone(),
            updated_at: result.updated_at,
            zone: result.zone.clone(),
            total_rrf: contribution,
            total_weight: weight,
        }
    }

    fn add_score(&mut self, result: &ScoredResult, contribution: f32, weight: f32) {
        if result.score > self.best_original_score {
            self.best_original_score = result.score;
            self.content = result.content.clone();
            self.source = result.source.clone();
        }
        self.total_rrf += contribution;
        self.total_weight += weight;
    }

    fn into_result(self) -> ScoredResult {
        ScoredResult {
            id: self.id,
            content: self.content,
            score: self.total_rrf,
            source: "hybrid".to_string(),
            path: self.path,
            updated_at: self.updated_at,
            zone: self.zone,
        }
    }
}

/// Reciprocal Rank Fusion.
///
/// Result positions are treated as 1-based ranks.
/// After fusion, deduplicates by canonical path — when the same path appears
/// multiple times, keeps only the entry with the most recent `updated_at`.
pub fn reciprocal_rank_fusion(result_sets: Vec<Vec<ScoredResult>>, k: u32) -> Vec<ScoredResult> {
    reciprocal_rank_fusion_weighted(result_sets.into_iter().map(|set| (set, 1.0)).collect(), k)
}

/// Reciprocal Rank Fusion with weights for each result set.
pub fn reciprocal_rank_fusion_weighted(
    result_sets: Vec<(Vec<ScoredResult>, f32)>,
    k: u32,
) -> Vec<ScoredResult> {
    let mut scores: HashMap<String, FusedScore> = HashMap::new();

    for (result_set, weight) in result_sets {
        for (index, result) in result_set.into_iter().enumerate() {
            let rank = (index as u32) + 1;
            let contribution = weight / ((k + rank) as f32);

            scores
                .entry(result.path.clone())
                .and_modify(|entry| {
                    entry.add_score(&result, contribution, weight);
                    // Keep the entry with the most recent updated_at
                    if result.updated_at > entry.updated_at {
                        entry.id = result.id.clone();
                        entry.content = result.content.clone();
                        entry.source = result.source.clone();
                        entry.updated_at = result.updated_at;
                        entry.zone = result.zone.clone();
                    }
                })
                .or_insert_with(|| FusedScore::new(&result, contribution, weight));
        }
    }

    let mut ranked: Vec<_> = scores.into_values().collect();
    ranked.sort_by(|left, right| {
        right
            .total_rrf
            .partial_cmp(&left.total_rrf)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                right
                    .total_weight
                    .partial_cmp(&left.total_weight)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| left.id.cmp(&right.id))
    });

    ranked.into_iter().map(FusedScore::into_result).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rrf_fusion_two_result_sets() {
        let results = vec![
            vec![
                ScoredResult {
                    id: "a".into(),
                    content: "alpha".into(),
                    score: 1.0,
                    source: "keyword".into(),
                    path: "projects/a".into(),
                    updated_at: Some(1000),
                    zone: None,
                },
                ScoredResult {
                    id: "b".into(),
                    content: "bravo".into(),
                    score: 0.9,
                    source: "keyword".into(),
                    path: "projects/b".into(),
                    updated_at: Some(2000),
                    zone: None,
                },
                ScoredResult {
                    id: "c".into(),
                    content: "charlie".into(),
                    score: 0.8,
                    source: "keyword".into(),
                    path: "projects/c".into(),
                    updated_at: Some(3000),
                    zone: None,
                },
            ],
            vec![
                ScoredResult {
                    id: "b2".into(),
                    content: "bravo-rev2".into(),
                    score: 1.0,
                    source: "vector".into(),
                    path: "projects/b".into(), // same path as b above — should dedupe, keeping this (more recent)
                    updated_at: Some(2500),
                    zone: None,
                },
                ScoredResult {
                    id: "d".into(),
                    content: "delta".into(),
                    score: 0.9,
                    source: "vector".into(),
                    path: "projects/d".into(),
                    updated_at: Some(4000),
                    zone: None,
                },
                ScoredResult {
                    id: "a2".into(),
                    content: "alpha-rev2".into(),
                    score: 0.8,
                    source: "vector".into(),
                    path: "projects/a".into(), // same path as a above — should dedupe, keeping this (more recent)
                    updated_at: Some(1500),
                    zone: None,
                },
            ],
        ];

        let fused = reciprocal_rank_fusion(results, 60);
        let ids: Vec<_> = fused.iter().map(|result| result.id.clone()).collect();

        assert_eq!(ids[0], "b2"); // "b" path deduped, rev2 (2500) > original (2000)
        assert_eq!(ids[1], "a2"); // "a" path deduped, rev2 (1500) > original (1000)
    }

    #[test]
    fn test_rrf_with_empty_result_set() {
        let results = vec![
            vec![ScoredResult {
                id: "a".into(),
                content: "alpha".into(),
                score: 1.0,
                source: "keyword".into(),
                path: "".into(),
                updated_at: None,
                zone: None,
            }],
            vec![],
        ];

        let fused = reciprocal_rank_fusion(results, 60);
        assert_eq!(fused.len(), 1);
        assert_eq!(fused[0].id, "a");
    }

    #[test]
    fn test_rrf_precise_mathematical_weights() {
        // We'll perform explicit RRF weighted calculation and assert the scores match precisely.
        let set_1 = vec![ScoredResult {
            id: "doc_a".into(),
            content: "A content".into(),
            score: 1.0,
            source: "keyword".into(),
            path: "path/a".into(),
            updated_at: Some(100),
            zone: None,
        }];
        let set_2 = vec![
            ScoredResult {
                id: "doc_b".into(),
                content: "B content".into(),
                score: 1.0,
                source: "vector".into(),
                path: "path/b".into(),
                updated_at: Some(100),
                zone: None,
            },
            ScoredResult {
                id: "doc_a_alt".into(),
                content: "A alt content".into(),
                score: 0.8,
                source: "vector".into(),
                path: "path/a".into(), // Same path as doc_a, but rank 2 (index 1)
                updated_at: Some(150), // More recent timestamp
                zone: None,
            },
        ];

        let rrf_k = 60;
        let weight_1 = 0.6;
        let weight_2 = 0.4;

        let results = vec![(set_1, weight_1), (set_2, weight_2)];

        let fused = reciprocal_rank_fusion_weighted(results, rrf_k);

        // Let's compute expected scores:
        // doc_a (path/a):
        //   - Rank 1 in set_1: contribution_1 = 0.6 / (60 + 1) = 0.6 / 61
        //   - Rank 2 in set_2: contribution_2 = 0.4 / (60 + 2) = 0.4 / 62
        //   - Total RRF Score = (0.6 / 61) + (0.4 / 62)
        // doc_b (path/b):
        //   - Rank 1 in set_2: contribution_1 = 0.4 / (60 + 1) = 0.4 / 61
        //   - Total RRF Score = 0.4 / 61

        let expected_score_a = (weight_1 / 61.0) + (weight_2 / 62.0);
        let expected_score_b = weight_2 / 61.0;

        assert_eq!(fused.len(), 2);

        // Since expected_score_a > expected_score_b, fused[0] must be path/a, and fused[1] must be path/b
        assert_eq!(fused[0].path, "path/a");
        assert_eq!(fused[1].path, "path/b");

        // Verify the score tolerance
        let tolerance = 1e-6;
        assert!((fused[0].score - expected_score_a).abs() < tolerance);
        assert!((fused[1].score - expected_score_b).abs() < tolerance);

        // Path deduplication must have occurred. Since doc_a_alt had updated_at 150 > 100 (doc_a),
        // the final fused result for path/a must have preserved doc_a_alt's properties:
        assert_eq!(fused[0].id, "doc_a_alt");
        assert_eq!(fused[0].content, "A alt content");
        assert_eq!(fused[0].updated_at, Some(150));
    }

    #[test]
    fn test_rrf_tie_breaker_by_id() {
        // Tying score and weight, check if tie-breaker falls back to alphabetical id ordering.
        let set_1 = vec![
            ScoredResult {
                id: "z_id".into(),
                content: "Z".into(),
                score: 1.0,
                source: "keyword".into(),
                path: "path/z".into(),
                updated_at: Some(100),
                zone: None,
            },
            ScoredResult {
                id: "m_id".into(),
                content: "M".into(),
                score: 0.9,
                source: "keyword".into(),
                path: "path/m".into(),
                updated_at: Some(100),
                zone: None,
            },
        ];
        let set_2 = vec![ScoredResult {
            id: "a_id".into(),
            content: "A".into(),
            score: 1.0,
            source: "vector".into(),
            path: "path/a".into(),
            updated_at: Some(100),
            zone: None,
        }];

        // Let's run reciprocal_rank_fusion with k=60.
        // z_id at rank 1 in set_1: 1.0 / 61
        // a_id at rank 1 in set_2: 1.0 / 61
        // Since weights are identical, both will have a total_rrf of 1.0 / 61 and total_weight of 1.0.
        // The alphabetical tie-breaker should place a_id before z_id.
        let fused = reciprocal_rank_fusion(vec![set_1, set_2], 60);

        assert_eq!(fused[0].id, "a_id");
        assert_eq!(fused[1].id, "z_id");
        assert_eq!(fused[2].id, "m_id"); // Rank 2 in set_1: 1.0 / 62
    }

    #[test]
    fn test_rrf_deduplication_by_path_most_recent_timestamp() {
        // Construct multiple ScoredResults with the exact same path.
        // Ensure that the one with the maximum timestamp is kept.
        let set_1 = vec![ScoredResult {
            id: "doc_old".into(),
            content: "Old Version".into(),
            score: 1.0,
            source: "keyword".into(),
            path: "shared_path".into(),
            updated_at: Some(100),
            zone: None,
        }];
        let set_2 = vec![ScoredResult {
            id: "doc_new".into(),
            content: "New Version".into(),
            score: 1.0,
            source: "vector".into(),
            path: "shared_path".into(),
            updated_at: Some(300),
            zone: None,
        }];
        let set_3 = vec![ScoredResult {
            id: "doc_mid".into(),
            content: "Mid Version".into(),
            score: 1.0,
            source: "hybrid".into(),
            path: "shared_path".into(),
            updated_at: Some(200),
            zone: None,
        }];

        let fused = reciprocal_rank_fusion(vec![set_1, set_2, set_3], 60);
        assert_eq!(fused.len(), 1);
        assert_eq!(fused[0].id, "doc_new");
        assert_eq!(fused[0].content, "New Version");
        assert_eq!(fused[0].updated_at, Some(300));
    }
}
