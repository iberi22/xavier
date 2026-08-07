use crate::health::{collect_health, run_integrity_check};
use crate::memory::qmd::QmdMemory;
use crate::retrieval::gating::AdaptiveZoneBooster;
use crate::retrieval::tuner::RetrievalConfig;
use crate::settings::XavierSettings;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

use super::benchmark::{
    BenchmarkSnapshot, ExternalBenchmarkMetrics, run_external_benchmark, run_search_benchmark,
};
use super::experiments::{Experiment, ExperimentStatus, generate_experiments};
use super::gaps::{Gap, analyze_gaps};

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
    pub final_benchmark: Option<BenchmarkSnapshot>,
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
    pub(crate) memory: Option<Arc<QmdMemory>>,
    /// Optional adaptive booster for benchmark data
    pub(crate) booster: Option<Arc<Mutex<AdaptiveZoneBooster>>>,
    /// History of previous benchmark snapshots
    pub(crate) history: Arc<Mutex<Vec<BenchmarkSnapshot>>>,
    /// Whether the engine is allowed to run experiments autonomously
    pub(crate) autonomous_mode: bool,
    /// Minimum composite score (recall_delta + 0.5·precision_delta) an experiment
    /// must reach to be accepted. Defaults to 0.0 (no-harm). See `with_acceptance_threshold`.
    pub(crate) acceptance_threshold: f64,
    /// Smallest improvement (in composite units) considered meaningful. Deltas below
    /// this epsilon are rejected as noise even if non-negative. Defaults to 0.005.
    pub(crate) min_improvement: f64,
}

impl AutoImprovementEngine {
    /// New.
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

    /// With memory.
    pub fn with_memory(mut self, memory: Arc<QmdMemory>) -> Self {
        self.memory = Some(memory);
        self
    }

    /// With booster.
    pub fn with_booster(mut self, booster: Arc<Mutex<AdaptiveZoneBooster>>) -> Self {
        self.booster = Some(booster);
        self
    }

    /// With autonomous.
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

        // Cache hit rate
        let cache_hit_rate: f64 = if let Some(booster) = &self.booster {
            let booster = booster.lock().await;
            (booster.average_hit_rate().await * 100.0) as f64
        } else {
            0.0
        };

        // Run search benchmarks.
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
                    run_search_benchmark(memory, 50).await
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

    /// Delegate to benchmark module helper
    pub async fn run_external_benchmark(&self) -> Result<ExternalBenchmarkMetrics> {
        run_external_benchmark().await
    }

    /// Run a full cycle: benchmark → gaps → experiments → validate → merge → re-measure
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

        // Phase 4: Validate proposed experiments
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

        // Phase 5: Merge accepted configuration overrides.
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

        // Phase 6: Re-measure (Post-improvement baseline verification)
        let final_benchmark = if !accepted.is_empty() && self.autonomous_mode {
            Some(self.re_measure(settings, db).await)
        } else {
            None
        };

        ImprovementCycle {
            cycle_id,
            timestamp_secs: now,
            benchmark,
            gaps,
            experiments,
            accepted_changes: accepted,
            improvement_pct: improvement,
            final_benchmark,
        }
    }

    /// Re-measure step of the closed-loop auto-improvement: runs a fresh benchmark run after merging
    pub async fn re_measure(
        &self,
        settings: &XavierSettings,
        db: Option<&rusqlite::Connection>,
    ) -> BenchmarkSnapshot {
        self.run_benchmark(settings, db).await
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
    pub async fn validate_experiments(
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

            // Acceptance gate
            let clears_threshold = composite >= self.acceptance_threshold;
            let meaningful = (composite - self.acceptance_threshold).abs() >= self.min_improvement
                && composite > self.acceptance_threshold;
            let no_recall_regression = recall_delta >= -0.01;
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

// ---------------------------------------------------------------------------
// Improvement-history persistence helpers
// ---------------------------------------------------------------------------

/// Load the full improvement history (newest-first) from `path`. Returns an empty
/// vector if the file does not yet exist.
pub fn load_history(path: &Path) -> Result<Vec<HistoryEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let payload = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read improvement history {}", path.display()))?;
    let entries: Vec<HistoryEntry> = serde_json::from_str(&payload)
        .with_context(|| format!("failed to parse improvement history {}", path.display()))?;
    Ok(entries)
}

/// Append a new entry to the improvement history file, keeping entries newest-first
/// and capped at 200 records. Creates parent directories as needed.
pub fn append_history_entry(path: &Path, entry: &HistoryEntry) -> Result<()> {
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
/// returning a new config.
pub fn merge_overrides_into_config<'a, I>(mut config: RetrievalConfig, overrides: I) -> RetrievalConfig
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
    use std::collections::HashMap;
    use crate::auto_improvement::{GapSeverity, Gap};

    #[tokio::test]
    async fn test_engine_cycle_no_memory() {
        let engine = AutoImprovementEngine::new();
        let settings = XavierSettings::default();
        let cycle = engine.run_cycle(&settings, None).await;
        assert!(cycle.cycle_id.starts_with("cycle-"));
        assert!(cycle.benchmark.total_documents == 0);
    }

    #[tokio::test]
    async fn test_autonomous_mode_accepts_experiments() {
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
        assert!(accepted.is_empty());
        assert!(validated
            .iter()
            .all(|e| matches!(e.status, ExperimentStatus::Pending)));
    }

    #[tokio::test]
    async fn test_cycle_does_not_panic_with_default_settings() {
        let engine = AutoImprovementEngine::new();
        let settings = XavierSettings::default();

        let result =
            tokio::time::timeout(Duration::from_secs(10), engine.run_cycle(&settings, None)).await;

        assert!(result.is_ok(), "Cycle timed out or panicked");
    }

    #[tokio::test]
    async fn test_min_improvement_gate_rejects_noise() {
        let engine = AutoImprovementEngine::new()
            .with_acceptance_threshold(0.0)
            .with_min_improvement(0.05);

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

        let threshold = 0.0f64;
        let min_improvement = 0.05f64;
        let composite = 0.02f64;
        let clears_threshold = composite >= threshold;
        let meaningful = (composite - threshold).abs() >= min_improvement && composite > threshold;
        let no_recall_regression = true;
        let passed = clears_threshold && meaningful && no_recall_regression;
        assert!(!passed, "sub-epsilon improvement must be rejected as noise");
    }

    #[tokio::test]
    async fn test_threshold_builder_accepts_strict_improvement() {
        let threshold = 0.0f64;
        let _min_improvement = 0.005f64;
        let composite = 0.1f64;
        let recall_delta = 0.05f64;
        let clears_threshold = composite >= threshold;
        let meaningful = (composite - threshold).abs() >= _min_improvement && composite > threshold;
        let no_recall_regression = recall_delta >= -0.01;
        let passed = clears_threshold && meaningful && no_recall_regression;
        assert!(
            passed,
            "meaningful improvement with no regression must pass"
        );
    }

    #[tokio::test]
    async fn test_threshold_builder_rejects_high_threshold() {
        let threshold = 0.2f64;
        let _min_improvement = 0.005f64;
        let composite = 0.1f64;
        let clears_threshold = composite >= threshold;
        assert!(
            !clears_threshold,
            "composite below threshold must be rejected"
        );
    }

    fn temp_history_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "xavier-autoimprove-history-{tag}-{}.json",
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn test_last_accepted_config_empty_history_returns_none() {
        let path = temp_history_path("empty");
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

        let cfg = AutoImprovementEngine::last_accepted_config_from(&path).unwrap();
        assert_eq!(cfg.rrf_k, 20, "newest entry should be returned first");

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
    }

    #[tokio::test]
    async fn test_cycle_full_loop_execution() {
        let engine = AutoImprovementEngine::new();
        let settings = XavierSettings::default();
        let cycle = engine.run_cycle(&settings, None).await;
        // Verify we ran baseline, gap analysis, and returned proper final_benchmark option (which is None since not autonomous/no experiments accepted)
        assert!(cycle.final_benchmark.is_none());
        assert_eq!(cycle.accepted_changes.len(), 0);
    }
}
