//! Evaluator Agent - Runs benchmarks to measure improvement

use crate::agents::evolve::config::BenchmarkType;
use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// Evaluator - Measures the metric for the current implementation
pub struct Evaluator {
    benchmark: BenchmarkType,
    history: Arc<RwLock<Vec<f32>>>,
}

impl Evaluator {
    /// New.
    pub fn new(benchmark: BenchmarkType) -> Self {
        Self {
            benchmark,
            history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Evaluate the current implementation
    pub async fn evaluate(&self) -> Result<f32> {
        let score = match self.benchmark {
            BenchmarkType::Locomo => self.run_locomo_benchmark().await?,
            BenchmarkType::EvoMemory => self.run_evomemory_benchmark().await?,
            BenchmarkType::Custom => self.run_custom_benchmark().await?,
        };

        // Track history
        let mut history = self.history.write().await;
        history.push(score);

        Ok(score)
    }

    /// Compare pre-mutation vs post-mutation results with statistical significance
    pub fn compare(&self, pre: f32, post: f32, is_lower_better: bool) -> bool {
        if is_lower_better {
            // Check if post is significantly lower than pre
            // Using a simple 1% threshold for "significance" in this implementation
            post < pre * 0.99
        } else {
            // Check if post is significantly higher than pre
            post > pre * 1.01
        }
    }

    /// Check if the latest score represents a regression compared to history
    pub async fn is_regression(&self, current_score: f32, is_lower_better: bool) -> bool {
        let history = self.history.read().await;
        if history.is_empty() {
            return false;
        }

        // Calculate rolling average of last 5 runs
        let window = history.iter().rev().take(5).cloned().collect::<Vec<f32>>();
        let avg: f32 = window.iter().sum::<f32>() / window.len() as f32;

        if is_lower_better {
            current_score > avg * 1.05 // 5% regression threshold
        } else {
            current_score < avg * 0.95 // 5% regression threshold
        }
    }

    async fn run_locomo_benchmark(&self) -> Result<f32> {
        info!("Running LoCoMo benchmark...");
        let output_dir = unique_benchmark_dir("locomo");
        run_benchmark_script(
            "scripts/benchmarks/run_locomo_benchmark.py",
            &[
                "--output-dir",
                output_dir.to_string_lossy().as_ref(),
                "--sample-limit",
                "1",
                "--question-limit",
                "2",
                "--mode",
                "assisted",
            ],
        )?;
        let summary = load_summary(&output_dir)?;
        let score = summary["metrics"]["overall"]["token_f1"]
            .as_f64()
            .or_else(|| summary["modes"]["assisted"]["metrics"]["overall"]["token_f1"].as_f64())
            .ok_or_else(|| anyhow!("LoCoMo summary missing token_f1"))? as f32;

        info!(score = score, "LoCoMo benchmark complete");
        Ok(score)
    }

    async fn run_evomemory_benchmark(&self) -> Result<f32> {
        info!("Running Evo-Memory benchmark...");
        let output_dir = unique_benchmark_dir("internal-memory");
        run_benchmark_script(
            "scripts/benchmarks/run_internal_memory_benchmark.py",
            &["--output-dir", output_dir.to_string_lossy().as_ref()],
        )?;
        let summary = load_summary(&output_dir)?;
        let score = summary["accuracy"]
            .as_f64()
            .ok_or_else(|| anyhow!("internal benchmark summary missing accuracy"))?
            as f32;

        info!(score = score, "Evo-Memory benchmark complete");
        Ok(score)
    }

    async fn run_custom_benchmark(&self) -> Result<f32> {
        info!("Running custom benchmark...");
        self.run_evomemory_benchmark().await
    }
}

fn unique_benchmark_dir(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "xavier-evaluator-{prefix}-{}",
        uuid::Uuid::new_v4()
    ))
}

fn run_benchmark_script(script: &str, args: &[&str]) -> Result<()> {
    #[cfg(not(feature = "bench-runners"))]
    {
        let _ = (script, args);
        Err(anyhow!(
            "benchmark runners disabled; rebuild with --features bench-runners or run benchmarks in CI/admin"
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

fn load_summary(output_dir: &Path) -> Result<serde_json::Value> {
    let path = output_dir.join("summary.json");
    let payload = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read benchmark summary {}", path.display()))?;
    serde_json::from_str(&payload)
        .with_context(|| format!("failed to parse benchmark summary {}", path.display()))
}
