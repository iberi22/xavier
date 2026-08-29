//! XTSP Automated Benchmark Runner for Token Savings & Recall Evaluation
//!
//! Evaluates payload sizes, compression ratios, token savings percentages,
//! recall@k, and precision@k across XTSP search modes (`ids`, `snippet`, `full`).

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Represents a document evaluated during XTSP benchmarking.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct XtspDocument {
    pub id: String,
    pub title: String,
    pub content: String,
}

/// Represents a single query benchmark case with ground truth relevance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XtspTestCase {
    pub query: String,
    pub ground_truth_ids: Vec<String>,
    pub documents: Vec<XtspDocument>,
}

/// Consolidated metrics produced by the XTSP benchmark runner.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct XtspBenchmarkResult {
    pub total_queries: usize,
    pub total_documents: usize,
    pub full_mode_bytes: usize,
    pub snippet_mode_bytes: usize,
    pub ids_mode_bytes: usize,
    pub snippet_compression_ratio: f64,
    pub ids_compression_ratio: f64,
    pub token_savings_pct: f64,
    pub recall_at_k: f64,
    pub precision_at_k: f64,
}

impl XtspBenchmarkResult {
    /// Serialize result report to pretty JSON string.
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

/// Default benchmark dataset for XTSP token economy & recall evaluation.
pub fn default_xtsp_test_cases() -> Vec<XtspTestCase> {
    vec![
        XtspTestCase {
            query: "vector architecture memory".to_string(),
            ground_truth_ids: vec!["doc_arch".to_string(), "doc_vec".to_string()],
            documents: vec![
                XtspDocument {
                    id: "doc_arch".to_string(),
                    title: "Xavier Core Architecture".to_string(),
                    content: "Xavier is built on a Hexagonal Architecture (Ports & Adapters) ensuring core vector memory isolation from external LLM transport protocols and database engines.".to_string(),
                },
                XtspDocument {
                    id: "doc_vec".to_string(),
                    title: "SQLite-Vec Integration".to_string(),
                    content: "As of v0.6+, Xavier utilizes SQLite-Vec as the primary vector storage layer providing zero-infrastructure zero-latency memory retrieval for autonomous agents.".to_string(),
                },
                XtspDocument {
                    id: "doc_other".to_string(),
                    title: "Unrelated CLI Settings".to_string(),
                    content: "General configuration options for local HTTP port bindings, log levels, and environment variable overrides.".to_string(),
                },
            ],
        },
        XtspTestCase {
            query: "security scanner threat".to_string(),
            ground_truth_ids: vec!["doc_sec".to_string()],
            documents: vec![
                XtspDocument {
                    id: "doc_sec".to_string(),
                    title: "Multi-Layer SecurityScanner".to_string(),
                    content: "SecurityScanner uses Aho-Corasick phrase matching, regex rules, and entropy checks to prevent prompt injections and secret leaks in runtime context.".to_string(),
                },
                XtspDocument {
                    id: "doc_perf".to_string(),
                    title: "Performance Benchmarks".to_string(),
                    content: "Throughput metrics and latency distributions for high-concurrency memory queries across distributed data nodes.".to_string(),
                },
            ],
        },
    ]
}

/// Extract query-aware snippet payload representation.
fn extract_snippet(content: &str, query: &str, max_chars: usize) -> String {
    let budget = crate::memory::snippet::SnippetBudget {
        title: 100,
        snippet: max_chars,
    };
    let meta = serde_json::json!({});
    let excerpt = crate::memory::snippet::extract(content, &meta, query, budget);
    if excerpt.snippet.is_empty() {
        crate::memory::snippet::clip_chars(content, max_chars).to_string()
    } else {
        excerpt.snippet
    }
}

/// Calculate relevance score for ranking documents against query.
fn calculate_doc_score(doc: &XtspDocument, query: &str) -> f64 {
    let query_lower = query.to_lowercase();
    let query_terms: Vec<&str> = query_lower.split_whitespace().collect();
    if query_terms.is_empty() {
        return 0.0;
    }

    let title_lower = doc.title.to_lowercase();
    let content_lower = doc.content.to_lowercase();

    let mut match_count = 0usize;
    for term in &query_terms {
        if title_lower.contains(term) {
            match_count += 3;
        }
        if content_lower.contains(term) {
            match_count += 1;
        }
    }

    match_count as f64 / (query_terms.len() * 3) as f64
}

/// Execute automated XTSP token savings and recall benchmark evaluation across search modes (`ids`, `snippet`, `full`).
pub fn run_xtsp_benchmark(
    test_cases: Option<&[XtspTestCase]>,
    target_k: usize,
) -> XtspBenchmarkResult {
    let default_cases;
    let cases = match test_cases {
        Some(c) if !c.is_empty() => c,
        _ => {
            default_cases = default_xtsp_test_cases();
            &default_cases
        }
    };

    let k = if target_k == 0 { 10 } else { target_k };

    let mut total_documents = 0usize;
    let mut full_mode_bytes = 0usize;
    let mut snippet_mode_bytes = 0usize;
    let mut ids_mode_bytes = 0usize;

    let mut total_recall = 0.0f64;
    let mut total_precision = 0.0f64;

    for tc in cases {
        total_documents += tc.documents.len();

        // Rank documents by score
        let mut scored_docs: Vec<(&XtspDocument, f64)> = tc
            .documents
            .iter()
            .map(|doc| (doc, calculate_doc_score(doc, &tc.query)))
            .collect();

        scored_docs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let top_k: Vec<&XtspDocument> = scored_docs.iter().take(k).map(|(doc, _)| *doc).collect();

        // 1. Full Mode Payload
        let full_payload: Vec<serde_json::Value> = top_k
            .iter()
            .map(|doc| {
                serde_json::json!({
                    "id": doc.id,
                    "title": doc.title,
                    "content": doc.content
                })
            })
            .collect();
        let full_json = serde_json::to_string(&full_payload).unwrap_or_default();
        full_mode_bytes += full_json.len();

        // 2. Snippet Mode Payload
        let snippet_payload: Vec<serde_json::Value> = top_k
            .iter()
            .map(|doc| {
                let snippet = extract_snippet(&doc.content, &tc.query, 140);
                serde_json::json!({
                    "id": doc.id,
                    "title": doc.title,
                    "snippet": snippet
                })
            })
            .collect();
        let snippet_json = serde_json::to_string(&snippet_payload).unwrap_or_default();
        snippet_mode_bytes += snippet_json.len();

        // 3. IDs Mode Payload
        let ids_payload: Vec<serde_json::Value> = top_k
            .iter()
            .map(|doc| {
                serde_json::json!({
                    "id": doc.id
                })
            })
            .collect();
        let ids_json = serde_json::to_string(&ids_payload).unwrap_or_default();
        ids_mode_bytes += ids_json.len();

        // Recall & Precision evaluation
        let retrieved_ids: HashSet<String> = top_k.iter().map(|doc| doc.id.clone()).collect();
        let gt_set: HashSet<String> = tc.ground_truth_ids.iter().cloned().collect();

        let hits = retrieved_ids.intersection(&gt_set).count();

        let recall = if !gt_set.is_empty() {
            hits as f64 / gt_set.len() as f64
        } else {
            1.0
        };

        let precision = if !retrieved_ids.is_empty() {
            hits as f64 / retrieved_ids.len() as f64
        } else {
            0.0
        };

        total_recall += recall;
        total_precision += precision;
    }

    let num_queries = cases.len();
    let num_q_f64 = if num_queries > 0 {
        num_queries as f64
    } else {
        1.0
    };

    let snippet_compression_ratio = if full_mode_bytes > 0 {
        snippet_mode_bytes as f64 / full_mode_bytes as f64
    } else {
        1.0
    };

    let ids_compression_ratio = if full_mode_bytes > 0 {
        ids_mode_bytes as f64 / full_mode_bytes as f64
    } else {
        1.0
    };

    let token_savings_pct = if full_mode_bytes > 0 {
        (full_mode_bytes.saturating_sub(snippet_mode_bytes)) as f64 / full_mode_bytes as f64 * 100.0
    } else {
        0.0
    };

    XtspBenchmarkResult {
        total_queries: num_queries,
        total_documents,
        full_mode_bytes,
        snippet_mode_bytes,
        ids_mode_bytes,
        snippet_compression_ratio,
        ids_compression_ratio,
        token_savings_pct,
        recall_at_k: total_recall / num_q_f64,
        precision_at_k: total_precision / num_q_f64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xtsp_benchmark_default_run() {
        let result = run_xtsp_benchmark(None, 5);
        assert_eq!(result.total_queries, 2);
        assert_eq!(result.total_documents, 5);
        assert!(result.full_mode_bytes > result.snippet_mode_bytes);
        assert!(result.snippet_mode_bytes > result.ids_mode_bytes);
        assert!(result.token_savings_pct > 0.0);
        assert!(result.snippet_compression_ratio < 1.0);
        assert!(result.ids_compression_ratio < result.snippet_compression_ratio);
        assert!(result.recall_at_k > 0.0);
        assert!(result.precision_at_k > 0.0);
    }

    #[test]
    fn test_xtsp_benchmark_custom_test_case() {
        let custom_cases = vec![XtspTestCase {
            query: "compiler optimization".to_string(),
            ground_truth_ids: vec!["doc_1".to_string()],
            documents: vec![
                XtspDocument {
                    id: "doc_1".to_string(),
                    title: "LLVM Optimization Pass".to_string(),
                    content: "This document describes compiler optimization passes in LLVM."
                        .to_string(),
                },
                XtspDocument {
                    id: "doc_2".to_string(),
                    title: "Unrelated Web Server".to_string(),
                    content: "An HTTP web server written in asynchronous Rust.".to_string(),
                },
            ],
        }];

        let result = run_xtsp_benchmark(Some(&custom_cases), 1);
        assert_eq!(result.total_queries, 1);
        assert_eq!(result.total_documents, 2);
        assert_eq!(result.recall_at_k, 1.0);
        assert_eq!(result.precision_at_k, 1.0);
    }
}
