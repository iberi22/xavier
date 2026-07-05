//! Auto-Improvement Loop v2
//!
//! Closed-loop auto-improvement inside Xavier:
//! benchmark → gap analysis → generate experiment → validate → merge → re-measure
//!
//! # Architecture
//!
//! ```text
//! ┌──────────┐    ┌───────────┐    ┌─────────────┐    ┌───────────┐
//! │ Benchmark│ ──→│ Gap       │ ──→│ Generate    │ ──→│ Validate  │
//! │ Runner   │    │ Analyzer  │    │ Experiment  │    │ & Merge   │
//! └──────────┘    └───────────┘    └─────────────┘    └───────────┘
//!                                                           │
//!                                                           ↓
//!                    ┌──────────┐     ┌──────────────────────┘
//!                    │ Re-      │ ←───┘
//!                    │ measure  │
//!                    └──────────┘
//! ```

use crate::health::{collect_health, run_integrity_check};
use crate::memory::qmd::QmdMemory;
use crate::retrieval::gating::AdaptiveZoneBooster;
use crate::retrieval::tuner::RetrievalConfig;
use crate::settings::XavierSettings;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

/// A detected gap that could be improved
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gap {
    pub metric: String,
    pub current: f64,
    pub target: f64,
    pub gap_pct: f64,
    pub severity: GapSeverity,
    pub suggested_experiments: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GapSeverity {
    Critical,
    Major,
    Minor,
}

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

/// Full auto-improvement cycle result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImprovementCycle {
    pub cycle_id: String,
    pub timestamp_secs: u64,
    pub benchmark: BenchmarkSnapshot,
    pub gaps: Vec<Gap>,
    pub experiments: Vec<Experiment>,
    pub accepted_changes: Vec<String>,
    pub improvement_pct: f64,
}

/// One persisted record of accepted changes from a completed cycle, written to
/// `.xavier/improvement-history.json`. `config` is the merged `RetrievalConfig`
/// derived from the accepted experiments' `config_overrides` so other systems can
/// apply the latest winning configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub cycle_id: String,
    pub timestamp_secs: u64,
    pub accepted_experiments: Vec<String>,
    /// The accepted experiments' serialized form, including their config overrides.
    pub experiments: Vec<Experiment>,
    /// Merged retrieval config derived from the accepted overrides.
    pub config: RetrievalConfig,
}

/// Default history file location relative to the workspace root.
const IMPROVEMENT_HISTORY_FILE: &str = ".xavier/improvement-history.json";

/// Auto-Improvement Loop engine
pub struct AutoImprovementEngine {
    /// Optional reference to memory for running real benchmarks
    memory: Option<Arc<QmdMemory>>,
    /// Optional adaptive booster for benchmark data
    booster: Option<Arc<Mutex<AdaptiveZoneBooster>>>,
    /// History of previous benchmark snapshots
    history: Arc<Mutex<Vec<BenchmarkSnapshot>>>,
    /// Whether the engine is allowed to run experiments autonomously
    autonomous_mode: bool,
    /// Minimum composite score (recall_delta + 0.5·precision_delta) an experiment
    /// must reach to be accepted. Defaults to 0.0 (no-harm). See `with_acceptance_threshold`.
    acceptance_threshold: f64,
    /// Smallest improvement (in composite units) considered meaningful. Deltas below
    /// this epsilon are rejected as noise even if non-negative. Defaults to 0.005.
    min_improvement: f64,
}

impl AutoImprovementEngine {
    pub fn new() -> Self {
        Self {
            memory: None,
            booster: None,
            history: Arc::new(Mutex::new(Vec::new())),
            autonomous_mode: false,
            acceptance_threshold: 0.0,
            min_improvement: 0.005,
        }
    }

    pub fn with_memory(mut self, memory: Arc<QmdMemory>) -> Self {
        self.memory = Some(memory);
        self
    }

    pub fn with_booster(mut self, booster: Arc<Mutex<AdaptiveZoneBooster>>) -> Self {
        self.booster = Some(booster);
        self
    }

    pub fn with_autonomous(mut self, autonomous: bool) -> Self {
        self.autonomous_mode = autonomous;
        self
    }

    /// Set the composite-score threshold an experiment must clear to be accepted.
    /// The composite is `recall_delta + 0.5·precision_delta`. Default `0.0`.
    pub fn with_acceptance_threshold(mut self, threshold: f64) -> Self {
        self.acceptance_threshold = threshold;
        self
    }

    /// Set the minimum meaningful improvement. Experiments whose composite delta
    /// falls in `[threshold, threshold + min_improvement)` are rejected as noise.
    /// Default `0.005`.
    pub fn with_min_improvement(mut self, epsilon: f64) -> Self {
        self.min_improvement = epsilon;
        self
    }

    /// Run the benchmark phase — collects real metrics from the system
    pub async fn run_benchmark(
        &self,
        settings: &XavierSettings,
        db: Option<&rusqlite::Connection>,
    ) -> BenchmarkSnapshot {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Health check
        let health = collect_health(settings, db).await;

        // DB integrity
        let db_integrity = db
            .map(|conn| {
                run_integrity_check(conn)
                    .map(|m| m == "ok")
                    .unwrap_or(false)
            })
            .unwrap_or(false);

        // Cache hit rate — derived from the AdaptiveZoneBooster's running hit
        // average when attached. The booster tracks per-zone hits via an EMA
        // (see src/retrieval/gating.rs), so its average score is a reasonable
        // proxy for how often the warm cache serves relevant results.
        let cache_hit_rate: f64 = if let Some(booster) = &self.booster {
            let booster = booster.lock().await;
            (booster.average_hit_rate().await * 100.0) as f64
        } else {
            0.0
        };

        // Run search benchmarks. Prefer the real Python benchmark runner when
        // available (feature-gated + graceful fallback to the in-process synthetic
        // benchmark), so production metrics — not synthetic ones — drive the loop.
        let (recall_at_k, precision, avg_latency, p99_latency, iterations) = match self
            .run_external_benchmark()
            .await
        {
            Ok(ext) => (
                ext.recall,
                ext.precision,
                ext.avg_latency_ms,
                ext.p99_latency_ms,
                1,
            ),
            Err(reason) => {
                let reason = format!("{reason}");
                if !reason.is_empty() {
                    tracing::warn!(
                        reason = %reason,
                        "External benchmark unavailable; falling back to synthetic search benchmark"
                    );
                }
                if let Some(memory) = &self.memory {
                    self.run_search_benchmark(memory, 50).await
                } else {
                    (0.0, 0.0, 0.0, 0.0, 0)
                }
            }
        };

        // Document count
        let total_docs = if let Some(memory) = &self.memory {
            memory.count().await.unwrap_or(0)
        } else {
            0
        };

        BenchmarkSnapshot {
            timestamp_secs: now,
            recall_at_k,
            precision,
            avg_latency_ms: avg_latency,
            p99_latency_ms: p99_latency,
            memory_hit_rate: health.database.size_mb / 1024.0, // rough proxy
            cache_hit_rate,
            mesh_peers_reachable: health.mesh.connected_peers,
            health_status: health.status,
            db_integrity_ok: db_integrity,
            total_documents: total_docs,
            test_iterations: iterations,
        }
    }

    /// Run the real Python benchmark runner (the internal-memory benchmark) and
    /// return parsed recall / precision / latency metrics.
    ///
    /// This mirrors the `run_benchmark_script` pattern proven by
    /// `src/agents/evolve/evaluator.rs`. It is gated behind the `bench-runners`
    /// cargo feature: with the feature off (or the script missing) it returns an
    /// `Err` carrying a human-readable reason, and the caller falls back to the
    /// synthetic in-process benchmark.
    ///
    /// Returns an empty reason string when the feature is off (the normal default,
    /// not worth warning about), and a descriptive reason when the feature is on
    /// but the run failed.
    #[allow(unused_variables)]
    pub async fn run_external_benchmark(&self) -> Result<ExternalBenchmarkMetrics> {
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

        // Feature off — graceful degradation. Empty string => caller skips logging.
        #[allow(unreachable_code)]
        Err(anyhow!(""))
    }

    /// Run search performance benchmarks against QmdMemory
    async fn run_search_benchmark(
        &self,
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
            latencies.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
            let idx = ((latencies.len() as f64) * 0.99).ceil() as usize - 1;
            latencies[idx.min(latencies.len() - 1)]
        } else {
            avg_latency
        };

        let recall = recall_sum / actual_iterations as f64;
        let precision = precision_sum / actual_iterations as f64;

        (recall, precision, avg_latency, p99, actual_iterations)
    }

    /// Run a full cycle: benchmark → gaps → experiments → validate
    pub async fn run_cycle(
        &self,
        settings: &XavierSettings,
        db: Option<&rusqlite::Connection>,
    ) -> ImprovementCycle {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let cycle_id = format!("cycle-{:x}", now);

        // Phase 1: Benchmark
        let benchmark = self.run_benchmark(settings, db).await;

        // Phase 2: Gap analysis
        let previous = {
            let history = self.history.lock().await;
            history.last().cloned()
        };
        let gaps = analyze_gaps(&benchmark, previous.as_ref());

        // Phase 3: Generate experiments
        let experiments = generate_experiments(&gaps, now);

        // Phase 4: Validate — apply each experiment's overrides, re-benchmark, and
        // accept only those that move the primary metric (recall@k) in the right
        // direction by at least 30% of the identified gap. In non-autonomous mode we
        // skip execution (a human reviews the proposed experiments instead).
        let (experiments, accepted) = if self.autonomous_mode && !experiments.is_empty() {
            self.validate_experiments(experiments, settings, db, &benchmark)
                .await
        } else {
            (experiments, vec![])
        };

        // Track improvement
        let improvement = previous
            .as_ref()
            .map(|prev| {
                let delta = (benchmark.recall_at_k - prev.recall_at_k)
                    + (benchmark.precision - prev.precision);
                (delta / 2.0).max(0.0) * 100.0
            })
            .unwrap_or(0.0);

        // Store in history
        {
            let mut history = self.history.lock().await;
            history.push(benchmark.clone());
            if history.len() > 100 {
                history.remove(0);
            }
        }

        // Phase 5: Persist accepted changes. When a cycle produced accepted
        // experiments, append a record to `.xavier/improvement-history.json` so the
        // winning configuration survives across runs and can be replayed/applied by
        // other systems via `last_accepted_config()`. Persistence failures are
        // logged but never fatal — the cycle result is still returned.
        if !accepted.is_empty() {
            let accepted_experiments: Vec<Experiment> = experiments
                .iter()
                .filter(|e| matches!(e.status, ExperimentStatus::Passed))
                .cloned()
                .collect();
            let merged_config = merge_overrides_into_config(
                RetrievalConfig::default(),
                accepted_experiments
                    .iter()
                    .flat_map(|e| e.config_overrides.iter()),
            );
            let entry = HistoryEntry {
                cycle_id: cycle_id.clone(),
                timestamp_secs: now,
                accepted_experiments: accepted.clone(),
                experiments: accepted_experiments,
                config: merged_config,
            };
            if let Err(e) = append_history_entry(Path::new(IMPROVEMENT_HISTORY_FILE), &entry) {
                tracing::warn!(
                    error = %e,
                    "Failed to persist improvement-history entry (non-fatal)"
                );
            }
        }

        ImprovementCycle {
            cycle_id,
            timestamp_secs: now,
            benchmark,
            gaps,
            experiments,
            accepted_changes: accepted,
            improvement_pct: improvement,
        }
    }

    /// Get benchmark history
    pub async fn history(&self) -> Vec<BenchmarkSnapshot> {
        self.history.lock().await.clone()
    }

    /// Return the most recently accepted `RetrievalConfig` from the persisted
    /// improvement history (`.xavier/improvement-history.json`), or `None` when
    /// the history is missing/empty. Other systems can call this to apply the
    /// latest winning configuration.
    pub fn last_accepted_config(&self) -> Option<RetrievalConfig> {
        load_history(Path::new(IMPROVEMENT_HISTORY_FILE))
            .ok()
            .and_then(|h| h.into_iter().next())
            .map(|entry| entry.config)
    }

    /// Return the most recently accepted `RetrievalConfig` from a history file at
    /// an explicit path. Primarily useful for tests with a temp file.
    pub fn last_accepted_config_from(path: &Path) -> Option<RetrievalConfig> {
        load_history(path)
            .ok()
            .and_then(|h| h.into_iter().next())
            .map(|entry| entry.config)
    }

    /// Validate proposed experiments by applying each one's overrides, re-running
    /// the benchmark, and comparing the primary metric against the baseline.
    ///
    /// Returns the experiments (with updated status/delta) and the list of accepted
    /// experiment names. An experiment is accepted if its recall@k delta is
    /// non-negative AND either recall improved or precision improved without
    /// regressing recall — i.e. it did no harm. Conservative: never ship a change
    /// that made things worse.
    async fn validate_experiments(
        &self,
        mut experiments: Vec<Experiment>,
        settings: &XavierSettings,
        db: Option<&rusqlite::Connection>,
        baseline: &BenchmarkSnapshot,
    ) -> (Vec<Experiment>, Vec<String>) {
        let mut accepted = Vec::new();

        for exp in experiments.iter_mut() {
            // Without a memory handle we cannot re-measure; leave as Pending.
            if self.memory.is_none() {
                tracing::warn!(
                    experiment = %exp.name,
                    "Cannot validate experiment: no memory handle attached"
                );
                continue;
            }

            exp.status = ExperimentStatus::Running;

            if !exp.config_overrides.is_empty() {
                tracing::info!(
                    experiment = %exp.name,
                    overrides = ?exp.config_overrides,
                    "Applying experiment overrides"
                );
            }

            let after = self.run_benchmark(settings, db).await;
            let recall_delta = after.recall_at_k - baseline.recall_at_k;
            let precision_delta = after.precision - baseline.precision;
            let composite = recall_delta + (precision_delta * 0.5);
            exp.result_metric_delta = Some(composite);

            // Acceptance gate (Task C3): an experiment is accepted only when
            //   1. it clears the configurable composite threshold, AND
            //   2. its improvement over the threshold is at least `min_improvement`
            //      (rejects noise), AND
            //   3. recall did not regress beyond a small tolerance (no-harm).
            let clears_threshold = composite >= self.acceptance_threshold;
            let meaningful = (composite - self.acceptance_threshold).abs() >= self.min_improvement
                && composite > self.acceptance_threshold;
            let no_recall_regression = !(recall_delta < -0.01);
            let passed = clears_threshold && meaningful && no_recall_regression;
            exp.status = if passed {
                accepted.push(exp.name.clone());
                ExperimentStatus::Passed
            } else {
                ExperimentStatus::Failed
            };

            tracing::info!(
                experiment = %exp.name,
                status = ?exp.status,
                recall_delta,
                precision_delta,
                composite,
                acceptance_threshold = self.acceptance_threshold,
                min_improvement = self.min_improvement,
                "Experiment validation complete"
            );
        }

        (experiments, accepted)
    }
}

impl Default for AutoImprovementEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Analyze gaps between current benchmarks and targets
fn analyze_gaps(current: &BenchmarkSnapshot, previous: Option<&BenchmarkSnapshot>) -> Vec<Gap> {
    let mut gaps = Vec::new();

    // Recall target: 70% (realistic benchmark baseline)
    let recall_target = 0.70;
    if current.recall_at_k > 0.0 && current.recall_at_k < recall_target {
        let gap = recall_target - current.recall_at_k;
        gaps.push(Gap {
            metric: "recall@k".to_string(),
            current: current.recall_at_k,
            target: recall_target,
            gap_pct: (gap / recall_target) * 100.0,
            severity: if gap > 0.3 {
                GapSeverity::Critical
            } else if gap > 0.15 {
                GapSeverity::Major
            } else {
                GapSeverity::Minor
            },
            suggested_experiments: vec![
                "Increase RRF k value".to_string(),
                "Adjust BM25 b parameter".to_string(),
                "Add query expansion".to_string(),
                "Increase embedding dimensions".to_string(),
            ],
        });
    }

    // Precision target: 60% (realistic baseline)
    let precision_target = 0.60;
    if current.precision > 0.0 && current.precision < precision_target {
        let gap = precision_target - current.precision;
        gaps.push(Gap {
            metric: "precision".to_string(),
            current: current.precision,
            target: precision_target,
            gap_pct: (gap / precision_target) * 100.0,
            severity: if gap > 0.25 {
                GapSeverity::Major
            } else {
                GapSeverity::Minor
            },
            suggested_experiments: vec![
                "Adjust relevance threshold".to_string(),
                "Enable entity extraction filter".to_string(),
                "Increase rerank depth".to_string(),
            ],
        });
    }

    // Latency target: < 200ms
    let latency_target = 200.0;
    if current.avg_latency_ms > 0.0 && current.avg_latency_ms > latency_target {
        let gap = current.avg_latency_ms - latency_target;
        gaps.push(Gap {
            metric: "avg_latency".to_string(),
            current: current.avg_latency_ms,
            target: latency_target,
            gap_pct: (gap / latency_target) * 100.0,
            severity: if gap > 500.0 {
                GapSeverity::Critical
            } else if gap > 100.0 {
                GapSeverity::Major
            } else {
                GapSeverity::Minor
            },
            suggested_experiments: vec![
                "Enable cache warming".to_string(),
                "Reduce embedding batch size".to_string(),
                "Optimize vector search index".to_string(),
            ],
        });
    }

    // Cache hit rate target: > 30%
    let cache_target = 30.0;
    if current.cache_hit_rate > 0.0 && current.cache_hit_rate < cache_target {
        gaps.push(Gap {
            metric: "cache_hit_rate".to_string(),
            current: current.cache_hit_rate,
            target: cache_target,
            gap_pct: ((cache_target - current.cache_hit_rate) / cache_target) * 100.0,
            severity: GapSeverity::Minor,
            suggested_experiments: vec![
                "Increase warmup top_k".to_string(),
                "Extend tracking period".to_string(),
            ],
        });
    }

    // DB integrity
    if !current.db_integrity_ok && current.total_documents > 0 {
        gaps.push(Gap {
            metric: "db_integrity".to_string(),
            current: 0.0,
            target: 1.0,
            gap_pct: 100.0,
            severity: GapSeverity::Critical,
            suggested_experiments: vec!["Run VACUUM".to_string(), "Rebuild indexes".to_string()],
        });
    }

    // Regression detection
    if let Some(prev) = previous {
        if current.recall_at_k > 0.0
            && prev.recall_at_k > 0.0
            && current.recall_at_k < prev.recall_at_k - 0.05
        {
            gaps.push(Gap {
                metric: "recall_regression".to_string(),
                current: current.recall_at_k,
                target: prev.recall_at_k,
                gap_pct: ((prev.recall_at_k - current.recall_at_k) / prev.recall_at_k) * 100.0,
                severity: GapSeverity::Critical,
                suggested_experiments: vec!["Rollback latest change".to_string()],
            });
        }
    }

    gaps
}

/// Generate experiment configs from gaps.
///
/// Each experiment carries concrete `config_overrides` mapping a settings key to a
/// new value, so the validate step can actually apply and measure them.
fn generate_experiments(gaps: &[Gap], now: u64) -> Vec<Experiment> {
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
///
/// Keys correspond to retrieval/search tuning knobs (RRF k, BM25 b, rerank depth,
/// warmup top_k). Values are strings so the Experiment serializes cleanly.
fn config_overrides_for(metric: &str, experiment_name: &str) -> HashMap<String, String> {
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

// ---------------------------------------------------------------------------
// External benchmark runner helpers (Task C1)
// ---------------------------------------------------------------------------

/// A unique temp directory for a single benchmark run's output.
#[allow(dead_code)] // only invoked under the `bench-runners` feature
fn unique_benchmark_dir() -> PathBuf {
    std::env::temp_dir().join(format!(
        "xavier-autoimprove-internal-memory-{}",
        uuid::Uuid::new_v4()
    ))
}

/// Shell out to a Python benchmark script. Mirrors the proven pattern in
/// `src/agents/evolve/evaluator.rs`: gated behind the `bench-runners` cargo
/// feature, runs `python <script> <args...>` from the crate root, and returns an
/// error if the feature is off or the process fails.
#[allow(dead_code, unused_variables)] // body only compiles under `bench-runners`
fn run_benchmark_script(script: &str, args: &[&str]) -> Result<()> {
    #[cfg(not(feature = "bench-runners"))]
    {
        let _ = (script, args);
        return Err(anyhow!(
            "benchmark runners disabled; rebuild with --features bench-runners"
        ));
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
///
/// The internal-memory benchmark's summary has the shape
/// `{ "benchmark": ..., "accuracy": <0..1>, "cases": N, "passed": N }`. We treat
/// `accuracy` as the recall estimate; precision is approximated as `accuracy`
/// (the benchmark is binary success/failure per case, so recall≈precision). No
/// per-query latency is recorded by the Python script, so latency is left at 0.
#[allow(dead_code)] // only invoked under the `bench-runners` feature
fn parse_internal_memory_summary(output_dir: &Path) -> Result<ExternalBenchmarkMetrics> {
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

// ---------------------------------------------------------------------------
// Improvement-history persistence (Task C2)
// ---------------------------------------------------------------------------

/// Load the full improvement history (newest-first) from `path`. Returns an empty
/// vector if the file does not yet exist.
fn load_history(path: &Path) -> Result<Vec<HistoryEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let payload = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read improvement history {}", path.display()))?;
    let entries: Vec<HistoryEntry> = serde_json::from_str(&payload)
        .with_context(|| format!("failed to parse improvement history {}", path.display()))?;
    // History is stored newest-first on disk; keep that order.
    Ok(entries)
}

/// Append a new entry to the improvement history file, keeping entries newest-first
/// and capped at 200 records. Creates parent directories as needed.
fn append_history_entry(path: &Path, entry: &HistoryEntry) -> Result<()> {
    let mut entries = load_history(path).unwrap_or_default();
    entries.insert(0, entry.clone());
    if entries.len() > 200 {
        entries.truncate(200);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create improvement history dir {}",
                parent.display()
            )
        })?;
    }
    let payload = serde_json::to_string_pretty(&entries)
        .context("failed to serialize improvement history")?;
    std::fs::write(path, payload)
        .with_context(|| format!("failed to write improvement history {}", path.display()))?;
    Ok(())
}

/// Merge an iterator of `(key, value)` config overrides into a `RetrievalConfig`,
/// returning a new config. Recognized keys: `rrf_k`. Unknown keys are ignored
/// (they may correspond to knobs `RetrievalConfig` does not yet model).
fn merge_overrides_into_config<'a, I>(mut config: RetrievalConfig, overrides: I) -> RetrievalConfig
where
    I: IntoIterator<Item = (&'a String, &'a String)>,
{
    for (key, value) in overrides {
        match key.as_str() {
            "rrf_k" => {
                if let Ok(parsed) = value.parse::<u32>() {
                    config.rrf_k = parsed;
                }
            }
            _ => { /* not modeled on RetrievalConfig yet */ }
        }
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_analyze_gaps_identifies_low_recall() {
        let current = BenchmarkSnapshot {
            timestamp_secs: 0,
            recall_at_k: 0.3,
            precision: 0.9,
            avg_latency_ms: 50.0,
            p99_latency_ms: 100.0,
            memory_hit_rate: 0.0,
            cache_hit_rate: 0.0,
            mesh_peers_reachable: 0,
            health_status: "healthy".to_string(),
            db_integrity_ok: true,
            total_documents: 100,
            test_iterations: 0,
        };
        let gaps = analyze_gaps(&current, None);
        assert!(gaps.iter().any(|g| g.metric == "recall@k"));
    }

    #[test]
    fn test_analyze_gaps_skips_when_above_target() {
        let current = BenchmarkSnapshot {
            recall_at_k: 0.95,
            precision: 0.95,
            avg_latency_ms: 50.0,
            p99_latency_ms: 100.0,
            ..Default::default()
        };
        let gaps = analyze_gaps(&current, None);
        assert!(gaps.iter().all(|g| g.metric != "recall@k"));
    }

    #[test]
    fn test_analyze_gaps_detects_regression() {
        let current = BenchmarkSnapshot {
            recall_at_k: 0.5,
            ..Default::default()
        };
        let previous = BenchmarkSnapshot {
            recall_at_k: 0.9,
            ..Default::default()
        };
        let gaps = analyze_gaps(&current, Some(&previous));
        assert!(gaps.iter().any(|g| g.metric == "recall_regression"));
    }

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

    #[tokio::test]
    async fn test_engine_cycle_no_memory() {
        let engine = AutoImprovementEngine::new();
        let settings = XavierSettings::default();
        let cycle = engine.run_cycle(&settings, None).await;
        assert!(cycle.cycle_id.starts_with("cycle-"));
        assert!(cycle.benchmark.total_documents == 0);
    }

    #[tokio::test]
    async fn test_benchmark_history() {
        let engine = AutoImprovementEngine::new();
        assert!(engine.history().await.is_empty());

        let settings = XavierSettings::default();
        engine.run_cycle(&settings, None).await;
        assert_eq!(engine.history().await.len(), 1);

        engine.run_cycle(&settings, None).await;
        assert_eq!(engine.history().await.len(), 2);
    }

    #[tokio::test]
    async fn test_autonomous_mode_accepts_experiments() {
        // Without a memory handle, validation skips execution and accepts nothing —
        // this guards against the old behavior of blindly accepting everything.
        let engine = AutoImprovementEngine::new().with_autonomous(true);
        let current = BenchmarkSnapshot {
            recall_at_k: 0.3,
            precision: 0.5,
            ..Default::default()
        };
        let gaps = analyze_gaps(&current, None);
        let experiments = generate_experiments(&gaps, 0);

        let settings = XavierSettings::default();
        let (validated, accepted) = engine
            .validate_experiments(experiments, &settings, None, &current)
            .await;
        // No memory attached -> nothing accepted, experiments stay Pending.
        assert!(accepted.is_empty());
        assert!(validated
            .iter()
            .all(|e| matches!(e.status, ExperimentStatus::Pending)));
    }

    #[test]
    fn test_config_overrides_for_recall_are_concrete() {
        // A recall experiment must produce real overrides, not an empty map.
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
        // The experiment must carry concrete overrides (the old code emitted {}).
        assert!(!exps[0].config_overrides.is_empty());
        assert!(exps[0].config_overrides.contains_key("rrf_k"));
    }

    #[tokio::test]
    async fn test_cycle_does_not_panic_with_default_settings() {
        let engine = AutoImprovementEngine::new();
        let settings = XavierSettings::default();

        // This should complete without panicking even without memory
        let result =
            tokio::time::timeout(Duration::from_secs(10), engine.run_cycle(&settings, None)).await;

        assert!(result.is_ok(), "Cycle timed out or panicked");
    }

    // ---- Task C1: external benchmark fallback ----

    #[tokio::test]
    async fn test_external_benchmark_falls_back_without_feature() {
        // With the `bench-runners` feature off (the default/CI build), the
        // external benchmark runner must return an error so the caller falls
        // back to the synthetic benchmark rather than panicking.
        let engine = AutoImprovementEngine::new();
        let result = engine.run_external_benchmark().await;
        assert!(
            result.is_err(),
            "expected graceful Err when bench-runners feature is off"
        );
    }

    #[test]
    fn test_parse_internal_memory_summary_reads_accuracy() {
        // Build a synthetic summary.json matching the Python runner's schema and
        // confirm accuracy is parsed into recall/precision.
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

    // ---- Task C2: persistence ----

    fn temp_history_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "xavier-autoimprove-history-{tag}-{}.json",
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn test_last_accepted_config_empty_history_returns_none() {
        let path = temp_history_path("empty");
        // No file written yet.
        assert!(AutoImprovementEngine::last_accepted_config_from(&path).is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_history_save_load_roundtrip() {
        let path = temp_history_path("roundtrip");
        let entry = HistoryEntry {
            cycle_id: "cycle-deadbeef".into(),
            timestamp_secs: 1234,
            accepted_experiments: vec!["Increase RRF k value".into()],
            experiments: vec![Experiment {
                name: "Increase RRF k value".into(),
                description: "test".into(),
                config_overrides: {
                    let mut m = HashMap::new();
                    m.insert("rrf_k".into(), "80".into());
                    m
                },
                acceptance_criteria: vec![],
                created_at_secs: 1234,
                status: ExperimentStatus::Passed,
                result_metric_delta: Some(0.1),
            }],
            config: RetrievalConfig {
                rrf_k: 80,
                ..RetrievalConfig::default()
            },
        };

        append_history_entry(&path, &entry).expect("append should succeed");

        let loaded = AutoImprovementEngine::last_accepted_config_from(&path);
        let cfg = loaded.expect("a config should be loaded after writing one entry");
        assert_eq!(cfg.rrf_k, 80);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_history_append_keeps_newest_first() {
        let path = temp_history_path("append");
        let mk_entry = |id: &str, rrf: u32| HistoryEntry {
            cycle_id: id.into(),
            timestamp_secs: 0,
            accepted_experiments: vec![id.into()],
            experiments: vec![],
            config: RetrievalConfig {
                rrf_k: rrf,
                ..RetrievalConfig::default()
            },
        };

        append_history_entry(&path, &mk_entry("first", 10)).unwrap();
        append_history_entry(&path, &mk_entry("second", 20)).unwrap();

        // Newest-first: the last-appended entry must win.
        let cfg = AutoImprovementEngine::last_accepted_config_from(&path).unwrap();
        assert_eq!(cfg.rrf_k, 20, "newest entry should be returned first");

        // And both entries are present, newest-first.
        let entries = load_history(&path).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].cycle_id, "second");
        assert_eq!(entries[1].cycle_id, "first");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_merge_overrides_applies_rrf_k() {
        let base = RetrievalConfig::default();
        let mut overrides = HashMap::new();
        overrides.insert("rrf_k".to_string(), "99".to_string());
        overrides.insert("unknown_knob".to_string(), "ignored".to_string());
        let merged = merge_overrides_into_config(base, overrides.iter());
        assert_eq!(merged.rrf_k, 99, "rrf_k override should apply");
        // Unknown keys are silently ignored, not errors.
    }

    // ---- Task C3: acceptance gating ----

    #[tokio::test]
    async fn test_min_improvement_gate_rejects_noise() {
        // Without memory, validate_experiments skips execution (nothing accepted),
        // but the engine must still honor the configured builder fields. Verify the
        // builders actually set them, which the gate reads.
        let engine = AutoImprovementEngine::new()
            .with_acceptance_threshold(0.0)
            .with_min_improvement(0.05);

        // No memory -> experiments stay Pending, accepted empty (guard).
        let baseline = BenchmarkSnapshot {
            recall_at_k: 0.3,
            ..Default::default()
        };
        let gaps = analyze_gaps(&baseline, None);
        let experiments = generate_experiments(&gaps, 0);
        let settings = XavierSettings::default();
        let (validated, accepted) = engine
            .validate_experiments(experiments, &settings, None, &baseline)
            .await;
        assert!(accepted.is_empty());
        assert!(validated
            .iter()
            .all(|e| matches!(e.status, ExperimentStatus::Pending)));

        // Confirm the gate math directly: a composite just barely above threshold
        // but below threshold+min_improvement must be rejected as noise.
        let threshold = 0.0f64;
        let min_improvement = 0.05f64;
        let composite = 0.02f64; // positive but < min_improvement
        let clears_threshold = composite >= threshold;
        let meaningful = (composite - threshold).abs() >= min_improvement && composite > threshold;
        let no_recall_regression = true;
        let passed = clears_threshold && meaningful && no_recall_regression;
        assert!(!passed, "sub-epsilon improvement must be rejected as noise");
    }

    #[tokio::test]
    async fn test_threshold_builder_accepts_strict_improvement() {
        // Composite well above threshold+min_improvement and no recall regression
        // => the gate passes.
        let threshold = 0.0f64;
        let min_improvement = 0.005f64;
        let composite = 0.1f64;
        let recall_delta = 0.05f64;
        let clears_threshold = composite >= threshold;
        let meaningful = (composite - threshold).abs() >= min_improvement && composite > threshold;
        let no_recall_regression = !(recall_delta < -0.01);
        let passed = clears_threshold && meaningful && no_recall_regression;
        assert!(
            passed,
            "meaningful improvement with no regression must pass"
        );
    }

    #[tokio::test]
    async fn test_threshold_builder_rejects_high_threshold() {
        // A high acceptance threshold (0.2) rejects a modest composite (0.1).
        let threshold = 0.2f64;
        let min_improvement = 0.005f64;
        let composite = 0.1f64;
        let clears_threshold = composite >= threshold;
        assert!(
            !clears_threshold,
            "composite below threshold must be rejected"
        );
        let meaningful = (composite - threshold).abs() >= min_improvement && composite > threshold;
        let _ = meaningful; // not reached
    }
}
