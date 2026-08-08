use super::gaps::Gap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// An experiment configuration to try
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Experiment {
    pub name: String,
    pub description: String,
    pub config_overrides: HashMap<String, String>,
    pub acceptance_criteria: Vec<String>,
    pub created_at_secs: u64,
    pub status: ExperimentStatus,
    pub result_metric_delta: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExperimentStatus {
    Pending,
    Running,
    Passed,
    Failed,
}

/// Generate experiment configs from gaps.
pub fn generate_experiments(gaps: &[Gap], now: u64) -> Vec<Experiment> {
    let mut experiments = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    for gap in gaps.iter().take(5) {
        for exp_name in &gap.suggested_experiments {
            if seen_names.contains(exp_name) {
                continue; // deduplicate
            }
            seen_names.insert(exp_name.clone());

            let overrides = config_overrides_for(&gap.metric, exp_name);

            experiments.push(Experiment {
                name: exp_name.clone(),
                description: format!(
                    "Auto-generated: improve '{}' (current: {:.2}, target: {:.2})",
                    gap.metric, gap.current, gap.target
                ),
                config_overrides: overrides,
                acceptance_criteria: vec![format!(
                    "Improve {} by at least 30% of gap ({:.1}%)",
                    gap.metric, gap.gap_pct
                )],
                created_at_secs: now,
                status: ExperimentStatus::Pending,
                result_metric_delta: None,
            });
        }
    }

    experiments
}

/// Map a (metric, experiment-name) pair to concrete settings overrides.
pub fn config_overrides_for(metric: &str, experiment_name: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let name = experiment_name.to_ascii_lowercase();
    match metric {
        "recall@k" | "recall_regression" => {
            if name.contains("rrf") {
                map.insert("rrf_k".to_string(), "80".to_string());
            } else if name.contains("bm25") {
                map.insert("bm25_b".to_string(), "0.75".to_string());
            } else if name.contains("expansion") {
                map.insert("query_expansion".to_string(), "true".to_string());
            } else if name.contains("embedding") {
                map.insert("embedding_top_k".to_string(), "100".to_string());
            } else {
                map.insert("rrf_k".to_string(), "60".to_string());
                map.insert("rerank_depth".to_string(), "50".to_string());
            }
        }
        "precision" => {
            if name.contains("rerank") {
                map.insert("rerank_depth".to_string(), "50".to_string());
            } else if name.contains("threshold") {
                map.insert("min_relevance_score".to_string(), "0.35".to_string());
            } else {
                map.insert("rerank_depth".to_string(), "50".to_string());
                map.insert("bm25_b".to_string(), "0.75".to_string());
            }
        }
        "avg_latency" => {
            if name.contains("cache") || name.contains("warmup") {
                map.insert("warmup_top_k".to_string(), "20".to_string());
            } else if name.contains("batch") {
                map.insert("embedding_batch_size".to_string(), "32".to_string());
            } else {
                map.insert("vector_search_limit".to_string(), "200".to_string());
            }
        }
        "cache_hit_rate" => {
            map.insert("warmup_top_k".to_string(), "20".to_string());
            map.insert("cache_tracking_secs".to_string(), "3600".to_string());
        }
        _ => {}
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auto_improvement::{Gap, GapSeverity};

    #[test]
    fn test_generate_experiments_deduplicates() {
        let gaps = vec![
            Gap {
                metric: "recall@k".into(),
                current: 0.0,
                target: 1.0,
                gap_pct: 100.0,
                severity: GapSeverity::Critical,
                suggested_experiments: vec!["exp1".into(), "exp2".into()],
            },
            Gap {
                metric: "precision".into(),
                current: 0.0,
                target: 1.0,
                gap_pct: 50.0,
                severity: GapSeverity::Major,
                suggested_experiments: vec!["exp1".into()], // duplicate
            },
        ];
        let exps = generate_experiments(&gaps, 0);
        let unique_names: std::collections::HashSet<_> = exps.iter().map(|e| &e.name).collect();
        assert_eq!(unique_names.len(), exps.len());
    }

    #[test]
    fn test_config_overrides_for_recall_are_concrete() {
        let map = config_overrides_for("recall@k", "Increase RRF k value");
        assert!(map.contains_key("rrf_k"));
        assert_eq!(map.get("rrf_k").unwrap(), "80");

        let map = config_overrides_for("recall@k", "Adjust BM25 b parameter");
        assert!(map.contains_key("bm25_b"));

        let map = config_overrides_for("avg_latency", "Enable cache warming");
        assert!(map.contains_key("warmup_top_k"));
    }

    #[test]
    fn test_config_overrides_for_unknown_metric_is_empty() {
        let map = config_overrides_for("nonexistent_metric", "whatever");
        assert!(map.is_empty(), "unknown metrics should yield no overrides");
    }

    #[test]
    fn test_generate_experiments_emits_overrides() {
        let gaps = vec![Gap {
            metric: "recall@k".into(),
            current: 0.3,
            target: 0.7,
            gap_pct: 57.0,
            severity: GapSeverity::Major,
            suggested_experiments: vec!["Increase RRF k value".into()],
        }];
        let exps = generate_experiments(&gaps, 0);
        assert_eq!(exps.len(), 1);
        assert!(!exps[0].config_overrides.is_empty());
        assert!(exps[0].config_overrides.contains_key("rrf_k"));
    }

    #[test]
    fn test_config_overrides_for_precision() {
        let map = config_overrides_for("precision", "Adjust relevance threshold");
        assert!(map.contains_key("min_relevance_score"));
        assert_eq!(map.get("min_relevance_score").unwrap(), "0.35");
    }
}
