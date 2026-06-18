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
use crate::settings::XavierSettings;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
}

impl AutoImprovementEngine {
    pub fn new() -> Self {
        Self {
            memory: None,
            booster: None,
            history: Arc::new(Mutex::new(Vec::new())),
            autonomous_mode: false,
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
            .map(|conn| run_integrity_check(conn).map(|m| m == "ok").unwrap_or(false))
            .unwrap_or(false);

        // Cache hit rate
        let cache_hit_rate = 0.0; // TODO: wire up PredictiveCacheWarmup stats

        // Run search benchmarks if memory is available
        let (recall_at_k, precision, avg_latency, p99_latency, iterations) =
            if let Some(memory) = &self.memory {
                self.run_search_benchmark(memory, 50).await
            } else {
                (0.0, 0.0, 0.0, 0.0, 0)
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
                        let r_at_k = if results.len() >= 5 { 1.0 } else {
                            results.len() as f64 / 5.0
                        };

                        // Precision: docs with significant content are 'relevant'
                        let p = if results.is_empty() { 0.0 } else {
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

        // Phase 4: Validate
        let accepted = if self.autonomous_mode && !experiments.is_empty() {
            validate_and_report(&experiments).await
        } else {
            vec![]
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
}

impl Default for AutoImprovementEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Analyze gaps between current benchmarks and targets
fn analyze_gaps(
    current: &BenchmarkSnapshot,
    previous: Option<&BenchmarkSnapshot>,
) -> Vec<Gap> {
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
            severity: if gap > 0.3 { GapSeverity::Critical } else if gap > 0.15 { GapSeverity::Major } else { GapSeverity::Minor },
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
            severity: if gap > 0.25 { GapSeverity::Major } else { GapSeverity::Minor },
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
            severity: if gap > 500.0 { GapSeverity::Critical } else if gap > 100.0 { GapSeverity::Major } else { GapSeverity::Minor },
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
            suggested_experiments: vec![
                "Run VACUUM".to_string(),
                "Rebuild indexes".to_string(),
            ],
        });
    }

    // Regression detection
    if let Some(prev) = previous {
        if current.recall_at_k > 0.0 && prev.recall_at_k > 0.0
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

/// Generate experiment configs from gaps
fn generate_experiments(gaps: &[Gap], now: u64) -> Vec<Experiment> {
    let mut experiments = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    for gap in gaps.iter().take(5) {
        for exp_name in &gap.suggested_experiments {
            if seen_names.contains(exp_name) {
                continue; // deduplicate
            }
            seen_names.insert(exp_name.clone());

            experiments.push(Experiment {
                name: exp_name.clone(),
                description: format!(
                    "Auto-generated: improve '{}' (current: {:.2}, target: {:.2})",
                    gap.metric, gap.current, gap.target
                ),
                config_overrides: HashMap::new(),
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

/// Validate experiments — in autonomous mode, mark all pending as accepted.
/// In manual mode, return empty (human decides).
async fn validate_and_report(experiments: &[Experiment]) -> Vec<String> {
    let mut accepted = Vec::new();
    for exp in experiments.iter().filter(|e| matches!(e.status, ExperimentStatus::Pending)) {
        tracing::info!(
            experiment = %exp.name,
            description = %exp.description,
            "Auto-improvement experiment accepted"
        );
        accepted.push(exp.name.clone());
    }
    accepted
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
            recall_at_k: 0.5, ..Default::default()
        };
        let previous = BenchmarkSnapshot {
            recall_at_k: 0.9, ..Default::default()
        };
        let gaps = analyze_gaps(&current, Some(&previous));
        assert!(gaps.iter().any(|g| g.metric == "recall_regression"));
    }

    #[test]
    fn test_generate_experiments_deduplicates() {
        let gaps = vec![
            Gap {
                metric: "recall@k".into(),
                current: 0.0, target: 1.0, gap_pct: 100.0,
                severity: GapSeverity::Critical,
                suggested_experiments: vec!["exp1".into(), "exp2".into()],
            },
            Gap {
                metric: "precision".into(),
                current: 0.0, target: 1.0, gap_pct: 50.0,
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
        let _engine = AutoImprovementEngine::new().with_autonomous(true);
        // Create benchmark with a gap
        let current = BenchmarkSnapshot {
            recall_at_k: 0.3,
            precision: 0.5,
            ..Default::default()
        };
        let gaps = analyze_gaps(&current, None);
        let experiments = generate_experiments(&gaps, 0);

        if !experiments.is_empty() {
            let accepted = validate_and_report(&experiments).await;
            assert!(!accepted.is_empty());
        }
    }

    #[tokio::test]
    async fn test_cycle_does_not_panic_with_default_settings() {
        let engine = AutoImprovementEngine::new();
        let settings = XavierSettings::default();

        // This should complete without panicking even without memory
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            engine.run_cycle(&settings, None),
        ).await;

        assert!(result.is_ok(), "Cycle timed out or panicked");
    }
}
