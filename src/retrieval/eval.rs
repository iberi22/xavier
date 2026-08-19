//! Context Regeneration — recall@k evaluation harness.
//!
//! Measures retrieval quality (recall@k, MRR, precision@k) against a labelled
//! benchmark set, so the retrieval stack can be auto-tuned toward better recall.
//!
//! The benchmark format mirrors `scripts/benchmarks/datasets/internal_swal_openclaw_memory.json`:
//! a list of cases, each with a `query` and an `expected_path` (ground truth). A
//! result is a hit if its path/identifier matches the expected value.
//!
//! This module is the measurement half of context regeneration. The tuning half
//! lives in `tuner.rs`, which feeds these metrics into the RRF/layer weight knobs.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// One labelled evaluation case: a query and the path that SHOULD be retrieved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalCase {
    pub id: String,
    pub query: String,
    /// Ground-truth identifier (path, title, or content substring) that defines a hit.
    pub expected_path: String,
    /// Optional category for per-category tuning (defaults to "general").
    #[serde(default = "default_category")]
    pub category: String,
}

fn default_category() -> String {
    "general".to_string()
}

/// A loaded benchmark dataset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalDataset {
    pub dataset: String,
    pub cases: Vec<EvalCase>,
}

impl EvalDataset {
    /// Load a dataset from a JSON file in the benchmark format.
    pub fn load(path: &Path) -> Result<Self, EvalError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| EvalError::Io(format!("read {}: {}", path.display(), e)))?;
        let raw: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| EvalError::Parse(format!("parse JSON: {}", e)))?;

        // Accept both the full {dataset, documents, cases} shape and a bare array.
        let dataset_name = raw
            .get("dataset")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let cases = if let Some(arr) = raw.get("cases").and_then(|c| c.as_array()) {
            parse_cases(arr)
        } else if let Some(arr) = raw.as_array() {
            parse_cases(arr)
        } else {
            return Err(EvalError::Parse(
                "expected 'cases' array or top-level array".to_string(),
            ));
        };

        Ok(Self {
            dataset: dataset_name,
            cases,
        })
    }

    /// Build a synthetic dataset from (query, expected) pairs — useful for tests.
    pub fn from_pairs(name: &str, pairs: &[(&str, &str)]) -> Self {
        let cases = pairs
            .iter()
            .enumerate()
            .map(|(i, (q, e))| EvalCase {
                id: format!("case-{i}"),
                query: q.to_string(),
                expected_path: e.to_string(),
                category: "general".to_string(),
            })
            .collect();
        Self {
            dataset: name.to_string(),
            cases,
        }
    }
}

fn parse_cases(arr: &[serde_json::Value]) -> Vec<EvalCase> {
    arr.iter()
        .filter_map(|c| {
            Some(EvalCase {
                id: c.get("id").and_then(|v| v.as_str())?.to_string(),
                query: c.get("query").and_then(|v| v.as_str())?.to_string(),
                expected_path: c
                    .get("expected_path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                category: c
                    .get("category")
                    .and_then(|v| v.as_str())
                    .unwrap_or("general")
                    .to_string(),
            })
        })
        .collect()
}

/// The result of a single retrieval attempt for one case.
#[derive(Debug, Clone)]
pub struct CaseResult {
    pub case_id: String,
    pub hit: bool,
    /// Rank of the first hit (1-indexed); None if no hit.
    pub first_hit_rank: Option<usize>,
}

/// Aggregate metrics over a set of case results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalMetrics {
    pub dataset: String,
    pub num_cases: usize,
    /// Fraction of cases where the expected result appeared in the top-k.
    pub recall_at_k: f64,
    /// Mean Reciprocal Rank: averages 1/rank of first hit.
    pub mrr: f64,
    /// Fraction of cases with at least one hit (independent of k).
    pub hit_rate: f64,
    /// The k used for this measurement.
    pub k: usize,
    /// Precision@k: hits / (num_cases * k) — how many of the retrieved slots were correct.
    pub precision_at_k: f64,
    /// Rank deviation σ = sqrt( 1/N * sum( (actual_rank - expected_rank)^2 ) ).
    ///
    /// Evaluates how far actual hit ranks deviate from the expected rank (defaulting to 1).
    /// Unhit queries contribute penalty rank (k + 1). Perfect retrieval yields σ = 0.0.
    #[serde(default)]
    pub sigma: f64,
    /// If this metrics was computed for a specific category group, the category name.
    pub category: Option<String>,
}

impl RetrievalMetrics {
    /// Compute metrics from per-case outcomes.
    pub fn from_results(dataset: &str, results: &[CaseResult], k: usize) -> Self {
        Self::from_results_with_expected(dataset, results, k, &[])
    }

    /// Compute metrics from per-case outcomes with optional per-case expected ranks.
    ///
    /// Rank deviation σ is calculated as:
    ///   σ = sqrt( 1/N * sum( (actual_rank_i - expected_rank_i)^2 ) )
    /// where actual_rank_i is the 1-based rank of the first hit (or k+1 if missed).
    pub fn from_results_with_expected(
        dataset: &str,
        results: &[CaseResult],
        k: usize,
        expected_ranks: &[usize],
    ) -> Self {
        let num = results.len();
        if num == 0 {
            return Self {
                dataset: dataset.to_string(),
                num_cases: 0,
                recall_at_k: 0.0,
                mrr: 0.0,
                hit_rate: 0.0,
                k,
                precision_at_k: 0.0,
                sigma: 0.0,
                category: None,
            };
        }
        let hits = results.iter().filter(|r| r.hit).count();
        let recall = hits as f64 / num as f64;
        let precision = hits as f64 / (num as f64 * k as f64);
        let mrr = results
            .iter()
            .filter_map(|r| r.first_hit_rank.map(|rank| 1.0 / rank as f64))
            .sum::<f64>()
            / num as f64;

        let sum_sq_diff: f64 = results
            .iter()
            .enumerate()
            .map(|(idx, r)| {
                let exp = expected_ranks.get(idx).copied().unwrap_or(1) as f64;
                let actual = r
                    .first_hit_rank
                    .map(|rank| rank as f64)
                    .unwrap_or((k + 1) as f64);
                let diff = actual - exp;
                diff * diff
            })
            .sum();

        let sigma = (sum_sq_diff / num as f64).sqrt();

        Self {
            dataset: dataset.to_string(),
            num_cases: num,
            recall_at_k: recall,
            mrr,
            hit_rate: recall, // with a single expected_path per case, hit_rate == recall
            k,
            precision_at_k: precision,
            sigma,
            category: None,
        }
    }
}

/// Error type for evaluation operations.
#[derive(Debug)]
pub enum EvalError {
    Io(String),
    Parse(String),
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvalError::Io(m) | EvalError::Parse(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for EvalError {}

/// Check whether a retrieved result hits the expected ground-truth path.
///
/// A hit is a case-insensitive substring match on path/title/content, so it works
/// regardless of whether the retriever returns a filesystem path, a memory title,
/// or document content.
pub fn is_hit(result_text: &str, expected: &str) -> bool {
    if expected.is_empty() {
        return false;
    }
    result_text
        .to_ascii_lowercase()
        .contains(&expected.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_hit_case_insensitive_substring() {
        assert!(is_hit("src/repo/xavier/main.rs", "repo/xavier"));
        assert!(is_hit("REPO/XAVIER/schema", "repo/xavier"));
        assert!(!is_hit("src/lib.rs", "repo/xavier"));
        assert!(!is_hit("anything", ""));
    }

    #[test]
    fn test_from_pairs_builds_dataset() {
        let ds = EvalDataset::from_pairs("test", &[("q1", "e1"), ("q2", "e2")]);
        assert_eq!(ds.dataset, "test");
        assert_eq!(ds.cases.len(), 2);
        assert_eq!(ds.cases[0].query, "q1");
        assert_eq!(ds.cases[1].expected_path, "e2");
    }

    #[test]
    fn test_metrics_perfect_recall() {
        let results = vec![
            CaseResult {
                case_id: "a".into(),
                hit: true,
                first_hit_rank: Some(1),
            },
            CaseResult {
                case_id: "b".into(),
                hit: true,
                first_hit_rank: Some(2),
            },
        ];
        let m = RetrievalMetrics::from_results("t", &results, 5);
        assert!((m.recall_at_k - 1.0).abs() < 1e-9);
        assert!((m.mrr - 0.75).abs() < 1e-9); // (1/1 + 1/2) / 2
    }

    #[test]
    fn test_metrics_zero_recall() {
        let results = vec![
            CaseResult {
                case_id: "a".into(),
                hit: false,
                first_hit_rank: None,
            },
            CaseResult {
                case_id: "b".into(),
                hit: false,
                first_hit_rank: None,
            },
        ];
        let m = RetrievalMetrics::from_results("t", &results, 5);
        assert!(m.recall_at_k.abs() < 1e-9);
        assert!(m.mrr.abs() < 1e-9);
    }

    #[test]
    fn test_metrics_partial_recall() {
        let results = vec![
            CaseResult {
                case_id: "a".into(),
                hit: true,
                first_hit_rank: Some(1),
            },
            CaseResult {
                case_id: "b".into(),
                hit: false,
                first_hit_rank: None,
            },
            CaseResult {
                case_id: "c".into(),
                hit: true,
                first_hit_rank: Some(3),
            },
        ];
        let m = RetrievalMetrics::from_results("t", &results, 5);
        assert!((m.recall_at_k - (2.0 / 3.0)).abs() < 1e-9);
        // MRR = (1/1 + 0 + 1/3) / 3
        assert!((m.mrr - (4.0 / 9.0)).abs() < 1e-9);
    }

    #[test]
    fn test_metrics_empty() {
        let m = RetrievalMetrics::from_results("t", &[], 5);
        assert_eq!(m.num_cases, 0);
        assert!(m.recall_at_k.abs() < 1e-9);
    }

    #[test]
    fn test_precision_all_hits_is_one_over_k() {
        // When all cases hit, precision = num_cases / (num_cases * k) = 1/k.
        let results = vec![
            CaseResult {
                case_id: "a".into(),
                hit: true,
                first_hit_rank: Some(1),
            },
            CaseResult {
                case_id: "b".into(),
                hit: true,
                first_hit_rank: Some(2),
            },
        ];
        let m = RetrievalMetrics::from_results("t", &results, 5);
        // precision = 2 / (2 * 5) = 0.2 = 1/k
        assert!((m.precision_at_k - 0.2).abs() < 1e-9);
        // With k=1, precision == recall when all hit
        let m1 = RetrievalMetrics::from_results("t", &results, 1);
        assert!((m1.precision_at_k - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_precision_partial_recall_less_than_one() {
        // Partial hits: precision < 1/k.
        let results = vec![
            CaseResult {
                case_id: "a".into(),
                hit: true,
                first_hit_rank: Some(1),
            },
            CaseResult {
                case_id: "b".into(),
                hit: false,
                first_hit_rank: None,
            },
            CaseResult {
                case_id: "c".into(),
                hit: true,
                first_hit_rank: Some(3),
            },
        ];
        let m = RetrievalMetrics::from_results("t", &results, 5);
        // precision = 2 / (3 * 5) = 2/15 ≈ 0.1333
        assert!(m.precision_at_k < 1.0);
        assert!((m.precision_at_k - (2.0 / 15.0)).abs() < 1e-9);
    }

    #[test]
    fn test_sigma_rank_deviation_calculation() {
        // Perfect rank 1 retrieval for all expected rank 1 -> sigma = 0.0
        let results = vec![
            CaseResult {
                case_id: "a".into(),
                hit: true,
                first_hit_rank: Some(1),
            },
            CaseResult {
                case_id: "b".into(),
                hit: true,
                first_hit_rank: Some(1),
            },
        ];
        let m = RetrievalMetrics::from_results_with_expected("t", &results, 5, &[1, 1]);
        assert_eq!(m.sigma, 0.0);

        // Actual ranks [2, 1] vs expected [1, 1] -> diffs [1, 0] -> sq_diffs [1, 0] -> mean 0.5 -> sigma = sqrt(0.5) ≈ 0.7071
        let results_dev = vec![
            CaseResult {
                case_id: "a".into(),
                hit: true,
                first_hit_rank: Some(2),
            },
            CaseResult {
                case_id: "b".into(),
                hit: true,
                first_hit_rank: Some(1),
            },
        ];
        let m_dev = RetrievalMetrics::from_results_with_expected("t", &results_dev, 5, &[1, 1]);
        assert!((m_dev.sigma - (0.5_f64).sqrt()).abs() < 1e-6);
    }
}
