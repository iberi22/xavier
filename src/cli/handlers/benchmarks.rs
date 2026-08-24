//! Automated Benchmark Runner (Issue #1446)
//!
//! Provides an extensible automated benchmark runner supporting:
//! 1. `BenchmarkRunner` struct with configurable suites.
//! 2. RRF (Reciprocal Rank Fusion) Recall benchmark.
//! 3. Latency benchmark.
//! 4. Throughput benchmark.
//! 5. JSON results serialization.
//! 6. Comprehensive unit tests (>= 8 tests).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};

/// Identifies the benchmark category/type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkType {
    RrfRecall,
    Latency,
    Throughput,
    Custom(String),
}

/// Global or suite-level execution parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkConfig {
    pub name: String,
    pub iterations: usize,
    pub warmup_iterations: usize,
    pub concurrency: usize,
    pub target_k: usize,
    pub rrf_k_parameter: f64,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            name: "default_benchmark_suite".to_string(),
            iterations: 100,
            warmup_iterations: 10,
            concurrency: 1,
            target_k: 10,
            rrf_k_parameter: 60.0,
        }
    }
}

/// Performance and quality metrics collected during a benchmark run.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BenchmarkMetrics {
    pub recall_at_k: Option<f64>,
    pub mrr: Option<f64>,
    pub avg_latency_ms: Option<f64>,
    pub min_latency_ms: Option<f64>,
    pub max_latency_ms: Option<f64>,
    pub p50_latency_ms: Option<f64>,
    pub p95_latency_ms: Option<f64>,
    pub p99_latency_ms: Option<f64>,
    pub ops_per_second: Option<f64>,
    pub total_operations: Option<usize>,
    pub duration_ms: Option<f64>,
    pub custom_metrics: HashMap<String, f64>,
}

/// Result produced by executing a single benchmark suite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub suite_name: String,
    pub suite_type: BenchmarkType,
    pub success: bool,
    pub iterations: usize,
    pub metrics: BenchmarkMetrics,
    pub timestamp: String,
    pub error_message: Option<String>,
}

/// Full execution report compiling multiple benchmark results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub runner_name: String,
    pub timestamp: String,
    pub total_suites: usize,
    pub passed_suites: usize,
    pub failed_suites: usize,
    pub total_duration_ms: f64,
    pub results: Vec<BenchmarkResult>,
}

impl BenchmarkReport {
    /// Serialize report to pretty JSON string.
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

/// Trait defining a benchmark suite for `BenchmarkRunner`.
pub trait BenchmarkSuite: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn suite_type(&self) -> BenchmarkType;
    fn run(&self, config: &BenchmarkConfig) -> BenchmarkResult;
}

/// Extensible runner for benchmark suites.
pub struct BenchmarkRunner {
    config: BenchmarkConfig,
    suites: Vec<Box<dyn BenchmarkSuite>>,
}

impl Default for BenchmarkRunner {
    fn default() -> Self {
        Self::new(BenchmarkConfig::default())
    }
}

impl BenchmarkRunner {
    /// Create a new `BenchmarkRunner` with custom configuration.
    pub fn new(config: BenchmarkConfig) -> Self {
        Self {
            config,
            suites: Vec::new(),
        }
    }

    /// Add standard default benchmark suites (RRF, Latency, Throughput).
    pub fn with_default_suites(mut self) -> Self {
        self.suites.push(Box::new(RrfRecallBenchmark::default()));
        self.suites.push(Box::new(LatencyBenchmark::default()));
        self.suites.push(Box::new(ThroughputBenchmark::default()));
        self
    }

    /// Builder method to append a custom benchmark suite.
    pub fn with_suite<S: BenchmarkSuite + 'static>(mut self, suite: S) -> Self {
        self.suites.push(Box::new(suite));
        self
    }

    /// Mutable method to add a suite boxed.
    pub fn add_suite(&mut self, suite: Box<dyn BenchmarkSuite>) {
        self.suites.push(suite);
    }

    /// Get total configured suites count.
    pub fn suites_count(&self) -> usize {
        self.suites.len()
    }

    /// Get current benchmark config reference.
    pub fn config(&self) -> &BenchmarkConfig {
        &self.config
    }

    /// Run all configured benchmark suites and produce a aggregated `BenchmarkReport`.
    pub fn run_all(&self) -> BenchmarkReport {
        let start_time = Instant::now();
        let timestamp = chrono::Utc::now().to_rfc3339();
        let mut results = Vec::new();
        let mut passed_suites = 0;
        let mut failed_suites = 0;

        for suite in &self.suites {
            let res = suite.run(&self.config);
            if res.success {
                passed_suites += 1;
            } else {
                failed_suites += 1;
            }
            results.push(res);
        }

        let total_duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;

        BenchmarkReport {
            runner_name: self.config.name.clone(),
            timestamp,
            total_suites: self.suites.len(),
            passed_suites,
            failed_suites,
            total_duration_ms,
            results,
        }
    }

    /// Run a specific suite by name.
    pub fn run_suite(&self, name: &str) -> Option<BenchmarkResult> {
        self.suites
            .iter()
            .find(|s| s.name().eq_ignore_ascii_case(name))
            .map(|s| s.run(&self.config))
    }
}

// ============================================================================
// 1) RRF Recall Benchmark
// ============================================================================

/// Represents a single RRF retrieval query benchmark case.
#[derive(Debug, Clone)]
pub struct RrfTestCase {
    pub query: String,
    pub ground_truth: Vec<String>,
    pub channel_rankings: Vec<Vec<String>>,
}

/// Compute RRF (Reciprocal Rank Fusion) scores for doc IDs across channels.
/// Formula: RRF(d) = sum_{m} 1 / (k + rank_m(d))
pub fn calculate_rrf_scores(channel_rankings: &[Vec<String>], k_param: f64) -> Vec<(String, f64)> {
    let mut scores: HashMap<String, f64> = HashMap::new();

    for channel in channel_rankings {
        for (rank_idx, doc_id) in channel.iter().enumerate() {
            let rank = (rank_idx + 1) as f64;
            let score = 1.0 / (k_param + rank);
            *scores.entry(doc_id.clone()).or_insert(0.0) += score;
        }
    }

    let mut score_vec: Vec<(String, f64)> = scores.into_iter().collect();
    score_vec.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    score_vec
}

/// Benchmark suite for testing RRF recall and MRR accuracy.
pub struct RrfRecallBenchmark {
    test_cases: Vec<RrfTestCase>,
}

impl Default for RrfRecallBenchmark {
    fn default() -> Self {
        Self {
            test_cases: Self::default_test_cases(),
        }
    }
}

impl RrfRecallBenchmark {
    pub fn new(test_cases: Vec<RrfTestCase>) -> Self {
        Self { test_cases }
    }

    /// Generates a set of synthetic test cases for default evaluation.
    pub fn default_test_cases() -> Vec<RrfTestCase> {
        vec![
            RrfTestCase {
                query: "code graph indexing".to_string(),
                ground_truth: vec!["doc_cg_1".to_string(), "doc_cg_2".to_string()],
                channel_rankings: vec![
                    vec!["doc_cg_1".to_string(), "doc_other".to_string()],
                    vec!["doc_cg_2".to_string(), "doc_cg_1".to_string()],
                ],
            },
            RrfTestCase {
                query: "rrf search fusion".to_string(),
                ground_truth: vec!["doc_rrf_a".to_string(), "doc_rrf_b".to_string()],
                channel_rankings: vec![
                    vec![
                        "doc_rrf_a".to_string(),
                        "doc_x".to_string(),
                        "doc_rrf_b".to_string(),
                    ],
                    vec!["doc_rrf_b".to_string(), "doc_rrf_a".to_string()],
                ],
            },
        ]
    }
}

impl BenchmarkSuite for RrfRecallBenchmark {
    fn name(&self) -> &str {
        "rrf_recall"
    }

    fn description(&self) -> &str {
        "Evaluates Reciprocal Rank Fusion (RRF) Recall@K and MRR accuracy"
    }

    fn suite_type(&self) -> BenchmarkType {
        BenchmarkType::RrfRecall
    }

    fn run(&self, config: &BenchmarkConfig) -> BenchmarkResult {
        let start_time = Instant::now();
        if self.test_cases.is_empty() {
            return BenchmarkResult {
                suite_name: self.name().to_string(),
                suite_type: self.suite_type(),
                success: false,
                iterations: 0,
                metrics: BenchmarkMetrics::default(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                error_message: Some(
                    "No test cases configured for RRF recall benchmark".to_string(),
                ),
            };
        }

        let mut total_recall = 0.0;
        let mut total_mrr = 0.0;
        let k = config.target_k;

        for tc in &self.test_cases {
            let fused = calculate_rrf_scores(&tc.channel_rankings, config.rrf_k_parameter);
            let top_k_docs: Vec<String> = fused.iter().take(k).map(|(id, _)| id.clone()).collect();

            // Recall@K
            if !tc.ground_truth.is_empty() {
                let hits = tc
                    .ground_truth
                    .iter()
                    .filter(|gt| top_k_docs.contains(gt))
                    .count();
                total_recall += hits as f64 / tc.ground_truth.len() as f64;
            }

            // MRR (Mean Reciprocal Rank)
            let mut rr = 0.0;
            for (idx, (doc_id, _)) in fused.iter().enumerate() {
                if tc.ground_truth.contains(doc_id) {
                    rr = 1.0 / ((idx + 1) as f64);
                    break;
                }
            }
            total_mrr += rr;
        }

        let num_cases = self.test_cases.len() as f64;
        let avg_recall = total_recall / num_cases;
        let avg_mrr = total_mrr / num_cases;
        let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;

        let metrics = BenchmarkMetrics {
            recall_at_k: Some(avg_recall),
            mrr: Some(avg_mrr),
            duration_ms: Some(duration_ms),
            ..BenchmarkMetrics::default()
        };

        BenchmarkResult {
            suite_name: self.name().to_string(),
            suite_type: self.suite_type(),
            success: true,
            iterations: self.test_cases.len(),
            metrics,
            timestamp: chrono::Utc::now().to_rfc3339(),
            error_message: None,
        }
    }
}

// ============================================================================
// 2) Latency Benchmark
// ============================================================================

/// Type alias for latency workload closure.
pub type LatencyWorkload = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;

/// Benchmark suite for measuring latency (avg, min, max, p50, p95, p99).
#[derive(Default)]
pub struct LatencyBenchmark {
    workload: Option<LatencyWorkload>,
}

impl LatencyBenchmark {
    pub fn new(workload: LatencyWorkload) -> Self {
        Self {
            workload: Some(workload),
        }
    }
}

impl BenchmarkSuite for LatencyBenchmark {
    fn name(&self) -> &str {
        "latency"
    }

    fn description(&self) -> &str {
        "Measures operation execution latency distributions (avg, p50, p95, p99)"
    }

    fn suite_type(&self) -> BenchmarkType {
        BenchmarkType::Latency
    }

    fn run(&self, config: &BenchmarkConfig) -> BenchmarkResult {
        let start_time = Instant::now();

        // Default synthetic workload if none provided
        let workload: LatencyWorkload = self.workload.clone().unwrap_or_else(|| {
            Arc::new(|| {
                // Micro-workload simulation: matrix trace computation
                let mut sum = 0u64;
                for i in 0..500 {
                    sum = sum.wrapping_add(i * i);
                }
                let _ = sum;
                Ok(())
            })
        });

        // Warmup
        for _ in 0..config.warmup_iterations {
            let _ = workload();
        }

        let mut latencies: Vec<f64> = Vec::with_capacity(config.iterations);
        let mut errors = 0;

        for _ in 0..config.iterations {
            let op_start = Instant::now();
            match workload() {
                Ok(_) => {
                    let elapsed_ms = op_start.elapsed().as_secs_f64() * 1000.0;
                    latencies.push(elapsed_ms);
                }
                Err(_) => {
                    errors += 1;
                }
            }
        }

        let total_duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;

        if latencies.is_empty() {
            return BenchmarkResult {
                suite_name: self.name().to_string(),
                suite_type: self.suite_type(),
                success: false,
                iterations: config.iterations,
                metrics: BenchmarkMetrics::default(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                error_message: Some(format!("All {} workload runs failed", config.iterations)),
            };
        }

        latencies.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let len = latencies.len();
        let sum: f64 = latencies.iter().sum();
        let avg = sum / len as f64;
        let min = latencies[0];
        let max = latencies[len - 1];

        let p50 = latencies[(len as f64 * 0.50) as usize % len];
        let p95 = latencies[((len as f64 * 0.95) as usize).min(len - 1)];
        let p99 = latencies[((len as f64 * 0.99) as usize).min(len - 1)];

        let mut metrics = BenchmarkMetrics {
            avg_latency_ms: Some(avg),
            min_latency_ms: Some(min),
            max_latency_ms: Some(max),
            p50_latency_ms: Some(p50),
            p95_latency_ms: Some(p95),
            p99_latency_ms: Some(p99),
            duration_ms: Some(total_duration_ms),
            ..BenchmarkMetrics::default()
        };
        metrics
            .custom_metrics
            .insert("error_count".to_string(), errors as f64);

        BenchmarkResult {
            suite_name: self.name().to_string(),
            suite_type: self.suite_type(),
            success: errors == 0,
            iterations: config.iterations,
            metrics,
            timestamp: chrono::Utc::now().to_rfc3339(),
            error_message: if errors > 0 {
                Some(format!("Encountered {} errors during execution", errors))
            } else {
                None
            },
        }
    }
}

// ============================================================================
// 3) Throughput Benchmark
// ============================================================================

/// Type alias for throughput workload closure.
pub type ThroughputWorkload = Arc<dyn Fn() -> Result<usize, String> + Send + Sync>;

/// Benchmark suite for measuring ops/sec throughput.
#[derive(Default)]
pub struct ThroughputBenchmark {
    workload: Option<ThroughputWorkload>,
}

impl ThroughputBenchmark {
    pub fn new(workload: ThroughputWorkload) -> Self {
        Self {
            workload: Some(workload),
        }
    }
}

impl BenchmarkSuite for ThroughputBenchmark {
    fn name(&self) -> &str {
        "throughput"
    }

    fn description(&self) -> &str {
        "Measures throughput in operations per second (ops/sec)"
    }

    fn suite_type(&self) -> BenchmarkType {
        BenchmarkType::Throughput
    }

    fn run(&self, config: &BenchmarkConfig) -> BenchmarkResult {
        let workload: ThroughputWorkload = self.workload.clone().unwrap_or_else(|| {
            Arc::new(|| {
                // Synthetic operation: string formatting & hashing simulation
                let s = format!("benchmark_item_{}", 42);
                Ok(s.len())
            })
        });

        // Warmup
        for _ in 0..config.warmup_iterations {
            let _ = workload();
        }

        let start_time = Instant::now();
        let mut total_ops = 0usize;
        let mut errors = 0usize;

        for _ in 0..config.iterations {
            match workload() {
                Ok(ops) => {
                    total_ops += if ops == 0 { 1 } else { ops };
                }
                Err(_) => {
                    errors += 1;
                }
            }
        }

        let duration_secs = start_time.elapsed().as_secs_f64();
        let duration_ms = duration_secs * 1000.0;
        let ops_per_sec = if duration_secs > 0.0 {
            total_ops as f64 / duration_secs
        } else {
            0.0
        };

        let mut metrics = BenchmarkMetrics {
            ops_per_second: Some(ops_per_sec),
            total_operations: Some(total_ops),
            duration_ms: Some(duration_ms),
            ..BenchmarkMetrics::default()
        };
        metrics
            .custom_metrics
            .insert("error_count".to_string(), errors as f64);

        BenchmarkResult {
            suite_name: self.name().to_string(),
            suite_type: self.suite_type(),
            success: errors == 0,
            iterations: config.iterations,
            metrics,
            timestamp: chrono::Utc::now().to_rfc3339(),
            error_message: if errors > 0 {
                Some(format!(
                    "Encountered {} errors during throughput run",
                    errors
                ))
            } else {
                None
            },
        }
    }
}

// ============================================================================
// CLI Handler Entry Point
// ============================================================================

/// Primary handler for the benchmark CLI command.
pub async fn handle_benchmark_command(
    suite_filter: Option<String>,
    json_output: bool,
    iterations: Option<usize>,
) -> anyhow::Result<BenchmarkReport> {
    let mut config = BenchmarkConfig::default();
    if let Some(iters) = iterations {
        config.iterations = iters;
    }

    let runner = BenchmarkRunner::new(config).with_default_suites();

    let report = if let Some(filter) = suite_filter {
        let mut single_report_results = Vec::new();
        let start_time = Instant::now();

        if let Some(result) = runner.run_suite(&filter) {
            let passed = if result.success { 1 } else { 0 };
            let failed = if result.success { 0 } else { 1 };
            single_report_results.push(result);

            BenchmarkReport {
                runner_name: runner.config().name.clone(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                total_suites: 1,
                passed_suites: passed,
                failed_suites: failed,
                total_duration_ms: start_time.elapsed().as_secs_f64() * 1000.0,
                results: single_report_results,
            }
        } else {
            anyhow::bail!("Benchmark suite '{}' not found", filter);
        }
    } else {
        runner.run_all()
    };

    if json_output {
        println!("{}", report.to_json()?);
    } else {
        println!("\n=== Xavier Benchmark Execution Summary ===");
        println!("Runner: {}", report.runner_name);
        println!("Timestamp: {}", report.timestamp);
        println!("Total Suites: {}", report.total_suites);
        println!(
            "Passed: {} | Failed: {}",
            report.passed_suites, report.failed_suites
        );
        println!("Total Duration: {:.2} ms\n", report.total_duration_ms);

        for res in &report.results {
            let status_icon = if res.success {
                "[✓] PASS"
            } else {
                "[✗] FAIL"
            };
            println!("{} Suite: {}", status_icon, res.suite_name);
            if let Some(r) = res.metrics.recall_at_k {
                println!("   Recall@K: {:.4}", r);
            }
            if let Some(mrr) = res.metrics.mrr {
                println!("   MRR: {:.4}", mrr);
            }
            if let Some(avg) = res.metrics.avg_latency_ms {
                println!(
                    "   Avg Latency: {:.4} ms (P95: {:.4} ms, P99: {:.4} ms)",
                    avg,
                    res.metrics.p95_latency_ms.unwrap_or(0.0),
                    res.metrics.p99_latency_ms.unwrap_or(0.0)
                );
            }
            if let Some(ops) = res.metrics.ops_per_second {
                println!(
                    "   Throughput: {:.2} ops/sec (Total ops: {})",
                    ops,
                    res.metrics.total_operations.unwrap_or(0)
                );
            }
            println!();
        }
    }

    Ok(report)
}

// ============================================================================
// Unit Tests (Requirement 6: AT LEAST 8 Unit Tests)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_1_benchmark_config_defaults() {
        let config = BenchmarkConfig::default();
        assert_eq!(config.iterations, 100);
        assert_eq!(config.warmup_iterations, 10);
        assert_eq!(config.target_k, 10);
        assert!((config.rrf_k_parameter - 60.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_2_rrf_score_calculation() {
        let rankings = vec![
            vec!["docA".to_string(), "docB".to_string()],
            vec!["docB".to_string(), "docA".to_string()],
        ];
        let k = 60.0;
        let scores = calculate_rrf_scores(&rankings, k);
        assert_eq!(scores.len(), 2);
        assert!((scores[0].1 - scores[1].1).abs() < 1e-9);
    }

    #[test]
    fn test_3_rrf_recall_benchmark_execution() {
        let benchmark = RrfRecallBenchmark::default();
        let config = BenchmarkConfig::default();
        let res = benchmark.run(&config);

        assert_eq!(res.suite_name, "rrf_recall");
        assert_eq!(res.suite_type, BenchmarkType::RrfRecall);
        assert!(res.success);
        assert!(res.metrics.recall_at_k.unwrap() > 0.0);
        assert!(res.metrics.mrr.unwrap() > 0.0);
    }

    #[test]
    fn test_4_latency_benchmark_percentiles() {
        let workload: LatencyWorkload = Arc::new(|| {
            std::thread::sleep(std::time::Duration::from_micros(100));
            Ok(())
        });

        let benchmark = LatencyBenchmark::new(workload);
        let config = BenchmarkConfig {
            iterations: 20,
            warmup_iterations: 2,
            ..BenchmarkConfig::default()
        };

        let res = benchmark.run(&config);
        assert_eq!(res.suite_name, "latency");
        assert!(res.success);
        assert!(res.metrics.avg_latency_ms.unwrap() > 0.0);
        assert!(res.metrics.p50_latency_ms.unwrap() >= res.metrics.min_latency_ms.unwrap());
        assert!(res.metrics.p99_latency_ms.unwrap() <= res.metrics.max_latency_ms.unwrap());
    }

    #[test]
    fn test_5_throughput_benchmark_ops_per_sec() {
        let workload: ThroughputWorkload = Arc::new(|| Ok(5));
        let benchmark = ThroughputBenchmark::new(workload);
        let config = BenchmarkConfig {
            iterations: 10,
            warmup_iterations: 1,
            ..BenchmarkConfig::default()
        };

        let res = benchmark.run(&config);
        assert_eq!(res.suite_name, "throughput");
        assert!(res.success);
        assert_eq!(res.metrics.total_operations.unwrap(), 50);
        assert!(res.metrics.ops_per_second.unwrap() > 0.0);
    }

    #[test]
    fn test_6_benchmark_runner_configurable_suites() {
        let mut runner = BenchmarkRunner::default();
        assert_eq!(runner.suites_count(), 0);

        runner = runner.with_default_suites();
        assert_eq!(runner.suites_count(), 3);

        let report = runner.run_all();
        assert_eq!(report.total_suites, 3);
        assert_eq!(report.passed_suites, 3);
        assert_eq!(report.failed_suites, 0);
    }

    #[test]
    fn test_7_json_results_serialization() {
        let runner = BenchmarkRunner::default().with_default_suites();
        let report = runner.run_all();
        let json_str = report.to_json().expect("JSON serialization must succeed");

        assert!(json_str.contains("\"runner_name\":"));
        assert!(json_str.contains("\"results\":"));
        assert!(json_str.contains("\"rrf_recall\""));

        let deserialized: BenchmarkReport =
            serde_json::from_str(&json_str).expect("JSON deserialization must succeed");
        assert_eq!(deserialized.total_suites, report.total_suites);
    }

    #[test]
    fn test_8_benchmark_runner_single_suite_filter() {
        let runner = BenchmarkRunner::default().with_default_suites();
        let res = runner.run_suite("latency");
        assert!(res.is_some());
        let result = res.unwrap();
        assert_eq!(result.suite_name, "latency");
        assert!(result.success);

        let missing = runner.run_suite("non_existent_suite");
        assert!(missing.is_none());
    }

    #[test]
    fn test_9_failed_suite_handling() {
        struct FailingSuite;
        impl BenchmarkSuite for FailingSuite {
            fn name(&self) -> &str {
                "failing_suite"
            }
            fn description(&self) -> &str {
                "Intentionally fails"
            }
            fn suite_type(&self) -> BenchmarkType {
                BenchmarkType::Custom("failing".to_string())
            }
            fn run(&self, _config: &BenchmarkConfig) -> BenchmarkResult {
                BenchmarkResult {
                    suite_name: self.name().to_string(),
                    suite_type: self.suite_type(),
                    success: false,
                    iterations: 1,
                    metrics: BenchmarkMetrics::default(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    error_message: Some("Simulated failure".to_string()),
                }
            }
        }

        let runner = BenchmarkRunner::default().with_suite(FailingSuite);
        let report = runner.run_all();
        assert_eq!(report.passed_suites, 0);
        assert_eq!(report.failed_suites, 1);
        assert!(!report.results[0].success);
    }

    #[tokio::test]
    async fn test_10_handle_benchmark_command() {
        let report = handle_benchmark_command(Some("rrf_recall".to_string()), false, Some(5))
            .await
            .expect("Command handler should succeed");
        assert_eq!(report.total_suites, 1);
        assert_eq!(report.results[0].suite_name, "rrf_recall");
    }
}
