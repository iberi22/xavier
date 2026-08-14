use crate::health::{collect_health, run_integrity_check};
use crate::memory::qmd::QmdMemory;
use crate::retrieval::gating::AdaptiveZoneBooster;
use crate::settings::XavierSettings;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

/// KPI snapshot for a single benchmark run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkSnapshot {
    pub timestamp_secs: u64,
    pub recall_at_k: f64,
    pub precision: f64,
    pub avg_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub memory_hit_rate: f64,
    pub cache_hit_rate: f64,
    pub mesh_peers_reachable: u32,
    pub health_status: String,
    pub db_integrity_ok: bool,
    pub total_documents: usize,
    pub test_iterations: u32,
}

impl Default for BenchmarkSnapshot {
    fn default() -> Self {
        Self {
            timestamp_secs: 0,
            recall_at_k: 0.0,
            precision: 0.0,
            avg_latency_ms: 0.0,
            p99_latency_ms: 0.0,
            memory_hit_rate: 0.0,
            cache_hit_rate: 0.0,
            mesh_peers_reachable: 0,
            health_status: "unknown".to_string(),
            db_integrity_ok: false,
            total_documents: 0,
            test_iterations: 0,
        }
    }
}

/// Metrics parsed from the external (Python) benchmark runner.
#[derive(Debug, Clone, Default)]
pub struct ExternalBenchmarkMetrics {
    pub recall: f64,
    pub precision: f64,
    pub avg_latency_ms: f64,
    pub p99_latency_ms: f64,
}

/// A unique temp directory for a single benchmark run's output.
#[cfg_attr(not(feature = "bench-runners"), allow(dead_code))]
pub fn unique_benchmark_dir() -> PathBuf {
    std::env::temp_dir().join(format!(
        "xavier-autoimprove-internal-memory-{}",
        uuid::Uuid::new_v4()
    ))
}

/// Shell out to a Python benchmark script.
#[allow(dead_code, unused_variables)]
pub fn run_benchmark_script(script: &str, args: &[&str]) -> Result<()> {
    #[cfg(not(feature = "bench-runners"))]
    {
        let _ = (script, args);
        Err(anyhow!(
            "benchmark runners disabled; rebuild with --features bench-runners"
        ))
    }

    #[cfg(feature = "bench-runners")]
    {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let script_path = root.join(script);
        let status = std::process::Command::new("python")
            .arg(script_path)
            .args(args)
            .current_dir(&root)
            .status()
            .context("failed to start benchmark runner")?;

        if !status.success() {
            return Err(anyhow!("benchmark runner failed with status {status}"));
        }
        Ok(())
    }
}

/// Parse the `summary.json` written by `run_internal_memory_benchmark.py` into
/// external benchmark metrics.
#[cfg_attr(not(feature = "bench-runners"), allow(dead_code))]
pub fn parse_internal_memory_summary(output_dir: &Path) -> Result<ExternalBenchmarkMetrics> {
    let path = output_dir.join("summary.json");
    let payload = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read benchmark summary {}", path.display()))?;
    let summary: serde_json::Value = serde_json::from_str(&payload)
        .with_context(|| format!("failed to parse benchmark summary {}", path.display()))?;

    let accuracy = summary
        .get("accuracy")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| anyhow!("internal-memory summary missing 'accuracy' field"))?;

    Ok(ExternalBenchmarkMetrics {
        recall: accuracy,
        precision: accuracy,
        avg_latency_ms: 0.0,
        p99_latency_ms: 0.0,
    })
}

/// Run search performance benchmarks against QmdMemory
pub async fn run_search_benchmark(
    memory: &QmdMemory,
    iterations: u32,
) -> (f64, f64, f64, f64, u32) {
    if iterations == 0 {
        return (0.0, 0.0, 0.0, 0.0, 0);
    }

    // Sample queries to benchmark with
    let queries = vec![
        "memory retrieval",
        "code graph",
        "search benchmark",
        "context regeneration",
        "health check",
    ];

    let mut latencies = Vec::with_capacity(iterations as usize);
    let mut recall_sum = 0.0f64;
    let mut precision_sum = 0.0f64;

    let mut actual_iterations = 0u32;
    for _ in 0..iterations {
        for query in &queries {
            let start = Instant::now();
            match memory.search(query, 10).await {
                Ok(results) => {
                    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                    latencies.push(elapsed);

                    // Simple recall metric: how many results returned out of max
                    let r_at_k = if results.len() >= 5 {
                        1.0
                    } else {
                        results.len() as f64 / 5.0
                    };

                    // Precision: docs with significant content are 'relevant'
                    let p = if results.is_empty() {
                        0.0
                    } else {
                        let relevant = results.iter().filter(|r| r.content.len() > 50).count();
                        relevant as f64 / results.len() as f64
                    };

                    recall_sum += r_at_k;
                    precision_sum += p;
                    actual_iterations += 1;
                }
                Err(_) => {
                    // Query failed — don't count this iteration
                }
            }
        }
    }

    if actual_iterations == 0 {
        return (0.0, 0.0, 0.0, 0.0, 0);
    }

    let avg_latency = if !latencies.is_empty() {
        latencies.iter().sum::<f64>() / latencies.len() as f64
    } else {
        0.0
    };

    // P99 latency
    let p99 = if latencies.len() > 1 {
        let mut sorted = latencies.clone();
        sorted.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = ((sorted.len() as f64) * 0.99).ceil() as usize - 1;
        sorted[idx.min(sorted.len() - 1)]
    } else {
        avg_latency
    };

    let recall = recall_sum / actual_iterations as f64;
    let precision = precision_sum / actual_iterations as f64;

    (recall, precision, avg_latency, p99, actual_iterations)
}

/// Run the real Python benchmark runner (the internal-memory benchmark)
pub async fn run_external_benchmark() -> Result<ExternalBenchmarkMetrics> {
    #[cfg(feature = "bench-runners")]
    {
        let output_dir = unique_benchmark_dir();
        let script = "scripts/benchmarks/run_internal_memory_benchmark.py";
        run_benchmark_script(
            script,
            &["--output-dir", output_dir.to_string_lossy().as_ref()],
        )
        .with_context(|| format!("internal-memory benchmark runner failed ({script})"))?;
        return parse_internal_memory_summary(&output_dir)
            .with_context(|| "failed to parse internal-memory benchmark summary".to_string());
    }

    // Feature off — graceful degradation.
    #[allow(unreachable_code)]
    Err(anyhow!(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_benchmark_history() {
        use crate::auto_improvement::AutoImprovementEngine;
        let engine = AutoImprovementEngine::new();
        assert!(engine.history().await.is_empty());

        let settings = XavierSettings::default();
        engine.run_cycle(&settings, None).await;
        assert_eq!(engine.history().await.len(), 1);

        engine.run_cycle(&settings, None).await;
        assert_eq!(engine.history().await.len(), 2);
    }

    #[tokio::test]
    async fn test_external_benchmark_falls_back_without_feature() {
        let result = run_external_benchmark().await;
        assert!(
            result.is_err(),
            "expected graceful Err when bench-runners feature is off"
        );
    }

    #[test]
    fn test_parse_internal_memory_summary_reads_accuracy() {
        let dir = std::env::temp_dir().join(format!(
            "xavier-autoimprove-parse-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let summary = serde_json::json!({
            "benchmark": "internal_swal_openclaw_memory",
            "cases": 4,
            "passed": 3,
            "accuracy": 0.75,
        });
        std::fs::write(dir.join("summary.json"), summary.to_string()).unwrap();

        let metrics = parse_internal_memory_summary(&dir).expect("parse should succeed");
        assert!((metrics.recall - 0.75).abs() < f64::EPSILON);
        assert!((metrics.precision - 0.75).abs() < f64::EPSILON);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_internal_memory_summary_rejects_missing_accuracy() {
        let dir = std::env::temp_dir().join(format!(
            "xavier-autoimprove-parse-missing-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("summary.json"),
            serde_json::json!({ "benchmark": "x", "cases": 0, "passed": 0 }).to_string(),
        )
        .unwrap();

        let result = parse_internal_memory_summary(&dir);
        assert!(result.is_err(), "missing accuracy must yield an error");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_run_search_benchmark_metrics() {
        let docs = Arc::new(tokio::sync::RwLock::new(Vec::new()));
        let memory = crate::memory::qmd_memory::QmdMemory::new(docs);
        let (recall, precision, avg, p99, iter) = run_search_benchmark(&memory, 0).await;
        assert_eq!(iter, 0);
        assert_eq!(recall, 0.0);
        assert_eq!(precision, 0.0);
        assert_eq!(avg, 0.0);
        assert_eq!(p99, 0.0);
    }
}
