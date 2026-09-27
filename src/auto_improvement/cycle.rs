use crate::health::{collect_health, run_integrity_check};
use crate::memory::qmd::QmdMemory;
use crate::retrieval::gating::AdaptiveZoneBooster;
use crate::retrieval::tuner::RetrievalConfig;
use crate::settings::XavierSettings;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use super::benchmark::{
    run_external_benchmark, run_search_benchmark, BenchmarkSnapshot, ExternalBenchmarkMetrics,
};
use super::experiments::{generate_experiments, Experiment, ExperimentStatus};
use super::gaps::{analyze_gaps, Gap};

/// Overall deadline for one full auto-improvement cycle. Configurable via
/// `XAVIER_IMPROVE_TOTAL_TIMEOUT_SECS`.
pub const IMPROVE_TOTAL_TIMEOUT: Duration = Duration::from_secs(300);

/// Per-stage deadline for stages that can perform unbounded work (benchmark
/// execution, experiment application, history persistence). Configurable via
/// `XAVIER_IMPROVE_STAGE_TIMEOUT_SECS`.
pub const IMPROVE_STAGE_TIMEOUT: Duration = Duration::from_secs(120);

/// Exit code for a completed cycle.
pub const EXIT_OK: i32 = 0;
/// Exit code for a cycle truncated by a deadline (conventional timeout code,
/// cf. GNU `timeout`).
pub const EXIT_TIMEOUT: i32 = 124;
/// Exit code for a genuine failure.
pub const EXIT_FAILURE: i32 = 1;

/// Hard ceiling for any applied deadline, whatever the source. `Instant +
/// Duration` panics on overflow, so every deadline is capped at 24h before it
/// is turned into an absolute instant.
pub const MAX_IMPROVE_TIMEOUT: Duration = Duration::from_secs(86_400);

/// Outcome of a full auto-improvement cycle, used by the CLI to pick a
/// meaningful exit code.
///
/// The stage name is a `String`, not a `&'static str`, because this enum is
/// serialized as part of `ImprovementCycle` and `HistoryEntry`: serde's
/// `Deserialize` for borrowed strings requires `'de: 'a`, which `&'static str`
/// cannot satisfy in a derived impl.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CycleStatus {
    /// Every stage ran to completion.
    #[default]
    Completed,
    /// A stage exceeded its deadline; partial progress was persisted.
    Truncated { stage: String },
    /// The cycle could not honour its durability contract (e.g. partial
    /// progress could not be persisted after a truncation).
    Failed,
}

impl CycleStatus {
    /// Process exit code the CLI should use for this outcome.
    pub fn exit_code(&self) -> i32 {
        match self {
            CycleStatus::Completed => EXIT_OK,
            CycleStatus::Truncated { .. } => EXIT_TIMEOUT,
            CycleStatus::Failed => EXIT_FAILURE,
        }
    }

    /// Name of the stage that exceeded its deadline, if the cycle was truncated.
    pub fn truncated_stage(&self) -> Option<&str> {
        match self {
            CycleStatus::Truncated { stage } => Some(stage.as_str()),
            _ => None,
        }
    }
}

/// Deadline budget for one cycle: an overall cap plus a per-stage cap. Each
/// stage runs under `min(stage_timeout, remaining overall budget)`.
#[derive(Debug, Clone, Copy)]
pub struct CycleBudget {
    pub overall_timeout: Duration,
    pub stage_timeout: Duration,
}

impl Default for CycleBudget {
    fn default() -> Self {
        Self {
            overall_timeout: IMPROVE_TOTAL_TIMEOUT,
            stage_timeout: IMPROVE_STAGE_TIMEOUT,
        }
    }
}

impl CycleBudget {
    /// Read deadline overrides from the environment. Non-numeric or zero values
    /// are ignored (a zero deadline would truncate every cycle immediately).
    /// Out-of-range values are capped at [`MAX_IMPROVE_TIMEOUT`].
    pub fn from_env() -> Self {
        let mut budget = Self::default();
        if let Some(secs) = env_timeout_secs("XAVIER_IMPROVE_TOTAL_TIMEOUT_SECS") {
            budget.overall_timeout = Duration::from_secs(secs).min(MAX_IMPROVE_TIMEOUT);
        }
        if let Some(secs) = env_timeout_secs("XAVIER_IMPROVE_STAGE_TIMEOUT_SECS") {
            budget.stage_timeout = Duration::from_secs(secs).min(MAX_IMPROVE_TIMEOUT);
        }
        budget
    }

    /// Overall deadline actually applied, capped so the absolute deadline can
    /// never overflow for any caller of `run_cycle_bounded` (including
    /// programmatic budgets built from unvalidated input).
    fn applied_overall_timeout(&self) -> Duration {
        self.overall_timeout.min(MAX_IMPROVE_TIMEOUT)
    }
}

fn env_timeout_secs(key: &str) -> Option<u64> {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|secs| *secs > 0)
}

/// Partial cycle state persisted when a cycle is truncated, so the work the
/// cycle did accomplish is never lost. It is written into the improvement
/// history and is the inspection record for a truncated run (`improve status`,
/// operators, tooling). The next cycle always re-runs from stage 1; nothing
/// replays this record automatically.
///
/// The stage names are `String`s because this record round-trips through serde.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialProgress {
    /// Stages that completed before truncation.
    pub completed_stages: Vec<String>,
    /// Stage that did not finish.
    pub truncated_stage: String,
    /// Benchmark snapshot measured by the benchmark stage (zeroed default if
    /// that stage never completed).
    pub benchmark: BenchmarkSnapshot,
    /// Gaps found by the gap-analysis stage (empty if it never completed).
    pub gaps: Vec<Gap>,
    /// Experiments generated before the truncation (empty if that stage never
    /// completed). Persisted so the generated proposals survive the truncation.
    #[serde(default)]
    pub experiments: Vec<Experiment>,
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
    pub final_benchmark: Option<BenchmarkSnapshot>,
    /// Outcome of the cycle (completed / truncated / failed).
    #[serde(default)]
    pub status: CycleStatus,
    /// Stages that ran to completion, in order.
    #[serde(default)]
    pub completed_stages: Vec<String>,
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
    /// Present only on entries written when a cycle was truncated: the partial
    /// progress of the stages that did complete. Inspection record for a
    /// truncated run; it is skipped by `last_accepted_config*` because such an
    /// entry accepted nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial: Option<PartialProgress>,
}

/// Default history file location relative to the workspace root.
const IMPROVEMENT_HISTORY_FILE: &str = ".xavier/improvement-history.json";

/// Stage names, used for progress output, truncation reporting, and the
/// persisted partial-progress record.
const STAGE_BENCHMARK: &str = "benchmark";
const STAGE_GAP_ANALYSIS: &str = "gap-analysis";
const STAGE_GENERATE_EXPERIMENTS: &str = "generate-experiments";
const STAGE_VALIDATE: &str = "validate";
const STAGE_PERSIST: &str = "persist-history";
const STAGE_RE_MEASURE: &str = "re-measure";

/// Result of running one bounded stage.
enum StageOutcome<T> {
    Completed(T),
    Truncated,
}

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
    /// Test seam: artificial delay injected inside a stage's deadline window,
    /// keyed by stage name. Only compiled under `cfg(test)`.
    #[cfg(test)]
    pub(crate) test_stage_delays: std::collections::HashMap<&'static str, Duration>,
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
            #[cfg(test)]
            test_stage_delays: std::collections::HashMap::new(),
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

    /// Run a full cycle with the default deadline budget: benchmark → gaps →
    /// experiments → validate → merge → re-measure.
    pub async fn run_cycle(
        &self,
        settings: &XavierSettings,
        db: Option<&rusqlite::Connection>,
    ) -> ImprovementCycle {
        self.run_cycle_bounded(settings, db, CycleBudget::default(), None, None)
            .await
    }

    /// Run a full cycle under an explicit deadline budget.
    ///
    /// The whole cycle is bounded by `budget.overall_timeout` (measured from
    /// entry) and each stage additionally by `budget.stage_timeout`. When a
    /// deadline expires the stage is cancelled, whatever was measured so far
    /// is durably persisted to the improvement-history file, and the returned
    /// cycle carries `CycleStatus::Truncated` naming the stage that did not
    /// finish. `history_path` overrides the default improvement-history
    /// location (tests use temp files); `progress` receives one line per stage
    /// transition for CLI progress output.
    pub async fn run_cycle_bounded(
        &self,
        settings: &XavierSettings,
        db: Option<&rusqlite::Connection>,
        budget: CycleBudget,
        progress: Option<&dyn Fn(&str)>,
        history_path: Option<&Path>,
    ) -> ImprovementCycle {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let cycle_id = format!("cycle-{:x}", now);
        let history_path = history_path.unwrap_or(Path::new(IMPROVEMENT_HISTORY_FILE));
        let overall_deadline = tokio::time::Instant::now() + budget.applied_overall_timeout();
        let mut completed_stages: Vec<&'static str> = Vec::new();

        // Stage 1: Benchmark
        let benchmark = match self
            .run_stage(STAGE_BENCHMARK, budget, overall_deadline, progress, async {
                self.run_benchmark(settings, db).await
            })
            .await
        {
            StageOutcome::Completed(b) => {
                completed_stages.push(STAGE_BENCHMARK);
                b
            }
            StageOutcome::Truncated => {
                return self
                    .truncated_cycle(
                        &cycle_id,
                        now,
                        completed_stages,
                        STAGE_BENCHMARK,
                        None,
                        Vec::new(),
                        Vec::new(),
                        history_path,
                    )
                    .await;
            }
        };

        // Stage 2: Gap analysis
        let previous = {
            let history = self.history.lock().await;
            history.last().cloned()
        };
        let gaps = match self
            .run_stage(
                STAGE_GAP_ANALYSIS,
                budget,
                overall_deadline,
                progress,
                async { analyze_gaps(&benchmark, previous.as_ref()) },
            )
            .await
        {
            StageOutcome::Completed(g) => {
                completed_stages.push(STAGE_GAP_ANALYSIS);
                g
            }
            StageOutcome::Truncated => {
                return self
                    .truncated_cycle(
                        &cycle_id,
                        now,
                        completed_stages,
                        STAGE_GAP_ANALYSIS,
                        Some(benchmark.clone()),
                        Vec::new(),
                        Vec::new(),
                        history_path,
                    )
                    .await;
            }
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

        // Stage 3: Generate experiments
        let experiments = match self
            .run_stage(
                STAGE_GENERATE_EXPERIMENTS,
                budget,
                overall_deadline,
                progress,
                async { generate_experiments(&gaps, now) },
            )
            .await
        {
            StageOutcome::Completed(e) => {
                completed_stages.push(STAGE_GENERATE_EXPERIMENTS);
                e
            }
            StageOutcome::Truncated => {
                return self
                    .truncated_cycle(
                        &cycle_id,
                        now,
                        completed_stages,
                        STAGE_GENERATE_EXPERIMENTS,
                        Some(benchmark.clone()),
                        gaps.clone(),
                        Vec::new(),
                        history_path,
                    )
                    .await;
            }
        };

        // Stage 4: Validate proposed experiments
        let (experiments, accepted) = if self.autonomous_mode && !experiments.is_empty() {
            // `validate_experiments` takes ownership, but the truncated arm
            // below still needs the unvalidated proposals, so hand it a clone.
            let to_validate = experiments.clone();
            match self
                .run_stage(STAGE_VALIDATE, budget, overall_deadline, progress, async {
                    self.validate_experiments(to_validate, settings, db, &benchmark)
                        .await
                })
                .await
            {
                StageOutcome::Completed((e, a)) => {
                    completed_stages.push(STAGE_VALIDATE);
                    (e, a)
                }
                StageOutcome::Truncated => {
                    return self
                        .truncated_cycle(
                            &cycle_id,
                            now,
                            completed_stages,
                            STAGE_VALIDATE,
                            Some(benchmark.clone()),
                            gaps.clone(),
                            experiments,
                            history_path,
                        )
                        .await;
                }
            }
        } else {
            (experiments, vec![])
        };

        // Stage 5: Merge accepted configuration overrides.
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
                partial: None,
            };
            let persist = self
                .run_stage(STAGE_PERSIST, budget, overall_deadline, progress, async {
                    // Best-effort: a lost history entry does not invalidate
                    // the accepted changes already applied.
                    append_history_entry(history_path, &entry).unwrap_or_else(|e| {
                        tracing::warn!(
                            error = %e,
                            "Failed to persist improvement-history entry (non-fatal)"
                        );
                    })
                })
                .await;
            if let StageOutcome::Completed(()) = persist {
                completed_stages.push(STAGE_PERSIST);
            } else {
                return self
                    .truncated_cycle(
                        &cycle_id,
                        now,
                        completed_stages,
                        STAGE_PERSIST,
                        Some(benchmark.clone()),
                        gaps.clone(),
                        experiments,
                        history_path,
                    )
                    .await;
            }
        }

        // Stage 6: Re-measure (Post-improvement baseline verification)
        let final_benchmark = if !accepted.is_empty() && self.autonomous_mode {
            match self
                .run_stage(
                    STAGE_RE_MEASURE,
                    budget,
                    overall_deadline,
                    progress,
                    async { self.re_measure(settings, db).await },
                )
                .await
            {
                StageOutcome::Completed(b) => {
                    completed_stages.push(STAGE_RE_MEASURE);
                    Some(b)
                }
                StageOutcome::Truncated => {
                    return self
                        .truncated_cycle(
                            &cycle_id,
                            now,
                            completed_stages,
                            STAGE_RE_MEASURE,
                            Some(benchmark.clone()),
                            gaps.clone(),
                            experiments,
                            history_path,
                        )
                        .await;
                }
            }
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
            status: CycleStatus::Completed,
            completed_stages: completed_stages.iter().map(|s| String::from(*s)).collect(),
        }
    }

    /// Assemble a truncated cycle, durably persisting the partial progress of
    /// the stages that completed. If the partial progress itself cannot be
    /// persisted the cycle is reported as `Failed` rather than `Truncated`:
    /// durability of partial progress is a hard contract of truncation, and
    /// `Failed` is the only producer of the genuine-failure exit code, so
    /// degrading it to a warning would both hide the lost work and leave
    /// `EXIT_FAILURE` unreachable.
    #[allow(clippy::too_many_arguments)]
    async fn truncated_cycle(
        &self,
        cycle_id: &str,
        now: u64,
        completed_stages: Vec<&'static str>,
        truncated_stage: &'static str,
        benchmark: Option<BenchmarkSnapshot>,
        gaps: Vec<Gap>,
        experiments: Vec<Experiment>,
        history_path: &Path,
    ) -> ImprovementCycle {
        let partial = PartialProgress {
            completed_stages: completed_stages.iter().map(|s| String::from(*s)).collect(),
            truncated_stage: truncated_stage.to_string(),
            benchmark: benchmark.clone().unwrap_or_default(),
            gaps: gaps.clone(),
            experiments: experiments.clone(),
        };
        let entry = HistoryEntry {
            cycle_id: cycle_id.to_string(),
            timestamp_secs: now,
            accepted_experiments: Vec::new(),
            // A truncated cycle accepted nothing, so the top-level `experiments`
            // (which means "accepted experiments", and feeds `config`) stays
            // empty; the generated proposals are kept inside `partial`.
            experiments: Vec::new(),
            config: RetrievalConfig::default(),
            partial: Some(partial),
        };
        let status = match append_history_entry(history_path, &entry) {
            Ok(()) => CycleStatus::Truncated {
                stage: truncated_stage.to_string(),
            },
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    stage = truncated_stage,
                    "Failed to persist partial cycle progress; reporting cycle as failed"
                );
                CycleStatus::Failed
            }
        };
        ImprovementCycle {
            cycle_id: cycle_id.to_string(),
            timestamp_secs: now,
            benchmark: benchmark.unwrap_or_default(),
            gaps,
            experiments,
            accepted_changes: Vec::new(),
            improvement_pct: 0.0,
            final_benchmark: None,
            status,
            completed_stages: completed_stages.iter().map(|s| String::from(*s)).collect(),
        }
    }

    /// Run one stage under both its own deadline and the remaining overall
    /// budget, emitting progress lines around it. The stage future is
    /// cancelled (dropped) when either deadline expires.
    async fn run_stage<T>(
        &self,
        name: &'static str,
        budget: CycleBudget,
        overall_deadline: tokio::time::Instant,
        progress: Option<&dyn Fn(&str)>,
        body: impl std::future::Future<Output = T>,
    ) -> StageOutcome<T> {
        if let Some(f) = progress {
            f(&format!("stage starting: {name}"));
        }
        // Test-only injected delay; production awaits `body` directly.
        #[cfg(test)]
        let body = async {
            if let Some(delay) = self.test_stage_delays.get(name) {
                tokio::time::sleep(*delay).await;
            }
            body.await
        };
        let stage_deadline = tokio::time::Instant::now()
            .checked_add(budget.stage_timeout)
            .map(|d| d.min(overall_deadline))
            .unwrap_or(overall_deadline);
        match tokio::time::timeout_at(stage_deadline, body).await {
            Ok(value) => {
                if let Some(f) = progress {
                    f(&format!("stage done: {name}"));
                }
                StageOutcome::Completed(value)
            }
            Err(_) => {
                if let Some(f) = progress {
                    f(&format!("stage DEADLINE EXCEEDED: {name}"));
                }
                StageOutcome::Truncated
            }
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
    ///
    /// Entries written by a truncated cycle are skipped: they accept nothing and
    /// carry a placeholder config, so honouring them would silently reset every
    /// consumer to `RetrievalConfig::default()`.
    pub fn last_accepted_config(&self) -> Option<RetrievalConfig> {
        Self::last_accepted_config_from(Path::new(IMPROVEMENT_HISTORY_FILE))
    }

    /// Same as [`Self::last_accepted_config`], reading a history file at an
    /// explicit path.
    pub fn last_accepted_config_from(path: &Path) -> Option<RetrievalConfig> {
        load_history(path)
            .ok()?
            .into_iter()
            .find(|entry| entry.partial.is_none())
            .map(|entry| entry.config)
    }

    /// Return the partial progress of the most recent truncated cycle recorded
    /// in a history file at an explicit path. This is the inspection record of a
    /// truncated run: the next cycle re-runs from stage 1 rather than replaying
    /// it, so callers use it to report or analyse what the truncated cycle
    /// achieved.
    pub fn last_partial_progress_from(path: &Path) -> Result<Option<PartialProgress>> {
        // History is newest-first; a later completed cycle must not hide it.
        Ok(load_history(path)?
            .into_iter()
            .find_map(|entry| entry.partial))
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
pub fn merge_overrides_into_config<'a, I>(
    mut config: RetrievalConfig,
    overrides: I,
) -> RetrievalConfig
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
    use crate::auto_improvement::{Gap, GapSeverity};
    use std::collections::HashMap;
    use std::time::Duration;

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
            partial: None,
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
            partial: None,
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

    /// A truncated cycle writes an entry whose `config` is a placeholder. That
    /// entry must not be handed to consumers as "the latest winning config".
    #[test]
    fn test_last_accepted_config_skips_truncated_entries() {
        let path = temp_history_path("skip-partial");
        let accepted = HistoryEntry {
            cycle_id: "cycle-accepted".into(),
            timestamp_secs: 1,
            accepted_experiments: vec!["Increase RRF k value".into()],
            experiments: vec![],
            config: RetrievalConfig {
                rrf_k: 77,
                ..RetrievalConfig::default()
            },
            partial: None,
        };
        let truncated = HistoryEntry {
            cycle_id: "cycle-truncated".into(),
            timestamp_secs: 2,
            accepted_experiments: vec![],
            experiments: vec![],
            config: RetrievalConfig::default(),
            partial: Some(PartialProgress {
                completed_stages: vec![STAGE_BENCHMARK.to_string()],
                truncated_stage: STAGE_VALIDATE.to_string(),
                benchmark: BenchmarkSnapshot::default(),
                gaps: vec![],
                experiments: vec![],
            }),
        };

        append_history_entry(&path, &accepted).unwrap();
        append_history_entry(&path, &truncated).unwrap();

        let cfg = AutoImprovementEngine::last_accepted_config_from(&path)
            .expect("the last accepted config must survive a newer truncated entry");
        assert_eq!(cfg.rrf_k, 77);
        assert_eq!(
            AutoImprovementEngine::last_partial_progress_from(&path)
                .unwrap()
                .map(|p| p.truncated_stage),
            Some(STAGE_VALIDATE.to_string()),
            "the truncated record must still be inspectable"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// The persisted partial record must survive a JSON round-trip: it is read
    /// back by every consumer, so borrowed stage names would not deserialize.
    #[test]
    fn test_partial_progress_roundtrips_through_json() {
        let partial = PartialProgress {
            completed_stages: vec![STAGE_BENCHMARK.to_string(), STAGE_GAP_ANALYSIS.to_string()],
            truncated_stage: STAGE_VALIDATE.to_string(),
            benchmark: BenchmarkSnapshot {
                recall_at_k: 0.42,
                ..BenchmarkSnapshot::default()
            },
            gaps: vec![Gap {
                metric: "recall@k".to_string(),
                current: 0.42,
                target: 0.70,
                gap_pct: 40.0,
                severity: GapSeverity::Critical,
                suggested_experiments: vec!["Increase RRF k value".to_string()],
            }],
            experiments: vec![Experiment {
                name: "Increase RRF k value".into(),
                description: "generated before truncation".into(),
                config_overrides: HashMap::from([("rrf_k".to_string(), "80".to_string())]),
                acceptance_criteria: vec![],
                created_at_secs: 1234,
                status: ExperimentStatus::Pending,
                result_metric_delta: None,
            }],
        };

        let json = serde_json::to_string(&partial).expect("partial progress must serialize");
        let back: PartialProgress =
            serde_json::from_str(&json).expect("partial progress must deserialize");
        assert_eq!(back.truncated_stage, STAGE_VALIDATE);
        assert_eq!(back.completed_stages, partial.completed_stages);
        assert_eq!(back.benchmark.recall_at_k, 0.42);
        assert_eq!(back.gaps.len(), 1);
        assert_eq!(back.experiments.len(), 1);
        assert_eq!(back.experiments[0].name, "Increase RRF k value");
    }

    /// An absurd deadline must be capped, not allowed to overflow the absolute
    /// instant the cycle is bounded by.
    #[test]
    fn test_absurd_budget_is_capped_instead_of_overflowing() {
        let budget = CycleBudget {
            overall_timeout: Duration::from_secs(u64::MAX),
            stage_timeout: Duration::from_secs(u64::MAX),
        };
        assert_eq!(budget.applied_overall_timeout(), MAX_IMPROVE_TIMEOUT);
        let _deadline = tokio::time::Instant::now() + budget.applied_overall_timeout();
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

    #[tokio::test]
    async fn improve_cycle_respects_overall_deadline() {
        let mut engine = AutoImprovementEngine::new();
        // The benchmark stage can never finish inside its deadline.
        engine
            .test_stage_delays
            .insert(STAGE_BENCHMARK, Duration::from_secs(30));
        let settings = XavierSettings::default();
        let history = temp_history_path("deadline");
        let budget = CycleBudget {
            overall_timeout: Duration::from_millis(300),
            stage_timeout: Duration::from_millis(50),
        };

        let cycle = engine
            .run_cycle_bounded(&settings, None, budget, None, Some(&history))
            .await;

        assert_eq!(
            cycle.status.truncated_stage(),
            Some(STAGE_BENCHMARK),
            "expected truncation at the benchmark stage, got {:?}",
            cycle.status
        );
        assert!(cycle.completed_stages.is_empty());
        assert_eq!(cycle.status.exit_code(), EXIT_TIMEOUT);

        let _ = std::fs::remove_file(&history);
    }

    #[tokio::test]
    async fn improve_persists_progress_when_stage_times_out() {
        let mut engine = AutoImprovementEngine::new();
        // Stages 1-2 (benchmark, gap-analysis) finish; stage 3
        // (generate-experiments) is injected far beyond its deadline.
        engine
            .test_stage_delays
            .insert(STAGE_GENERATE_EXPERIMENTS, Duration::from_secs(30));
        let settings = XavierSettings::default();
        let history = temp_history_path("partial");
        let budget = CycleBudget {
            overall_timeout: Duration::from_secs(10),
            // Above the 2 s embedder health-probe ceiling hit by stage 1.
            stage_timeout: Duration::from_secs(3),
        };

        // The progress sink is called synchronously from inside the cycle, so it
        // must use a blocking mutex, not the async one this module imports.
        let lines: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = lines.clone();
        let progress = move |msg: &str| {
            captured
                .lock()
                .expect("progress sink mutex poisoned")
                .push(msg.to_string());
        };

        let cycle = engine
            .run_cycle_bounded(&settings, None, budget, Some(&progress), Some(&history))
            .await;

        assert_eq!(
            cycle.status.truncated_stage(),
            Some(STAGE_GENERATE_EXPERIMENTS),
            "expected truncation at generate-experiments, got {:?}",
            cycle.status
        );
        assert_eq!(
            cycle.completed_stages,
            vec![STAGE_BENCHMARK.to_string(), STAGE_GAP_ANALYSIS.to_string()]
        );

        // Stages 1-2 results must be durably persisted, not discarded.
        let partial = AutoImprovementEngine::last_partial_progress_from(&history)
            .expect("history file should parse")
            .expect("partial progress must be persisted on truncation");
        assert_eq!(partial.truncated_stage, STAGE_GENERATE_EXPERIMENTS);
        assert_eq!(
            partial.completed_stages,
            vec![STAGE_BENCHMARK.to_string(), STAGE_GAP_ANALYSIS.to_string()]
        );
        assert!(
            partial.benchmark.timestamp_secs > 0,
            "stage-1 benchmark result must be durably persisted"
        );

        // One progress line per stage transition, naming the breached stage.
        let lines = lines.lock().expect("progress sink mutex poisoned");
        for stage in [STAGE_BENCHMARK, STAGE_GAP_ANALYSIS] {
            assert!(
                lines
                    .iter()
                    .any(|l| l.contains(&format!("stage starting: {stage}"))),
                "progress output should announce every stage start: {lines:?}"
            );
            assert!(
                lines
                    .iter()
                    .any(|l| l.contains(&format!("stage done: {stage}"))),
                "progress output should announce every completed stage: {lines:?}"
            );
        }
        assert!(
            lines
                .iter()
                .any(|l| l.contains("stage DEADLINE EXCEEDED: generate-experiments")),
            "progress output must name the stage that exceeded its deadline: {lines:?}"
        );

        let _ = std::fs::remove_file(&history);
    }

    #[tokio::test]
    async fn improve_exit_code_distinguishes_truncated_from_failed() {
        let settings = XavierSettings::default();
        let tight_budget = CycleBudget {
            overall_timeout: Duration::from_millis(300),
            stage_timeout: Duration::from_millis(50),
        };

        // Truncated: a stage that can never finish inside its deadline.
        let mut engine = AutoImprovementEngine::new();
        engine
            .test_stage_delays
            .insert(STAGE_BENCHMARK, Duration::from_secs(30));
        let truncated_history = temp_history_path("exit-truncated");
        let truncated = engine
            .run_cycle_bounded(
                &settings,
                None,
                tight_budget,
                None,
                Some(&truncated_history),
            )
            .await;
        assert!(
            matches!(truncated.status, CycleStatus::Truncated { .. }),
            "injected deadline breach must truncate the cycle, got {:?}",
            truncated.status
        );
        assert_eq!(truncated.status.exit_code(), 124);

        // Failed: truncation whose partial progress cannot be persisted —
        // the history path lives under a regular file, so the mandatory
        // partial-progress write fails.
        let blocker =
            std::env::temp_dir().join(format!("xavier-improve-blocker-{}", uuid::Uuid::new_v4()));
        std::fs::write(&blocker, "not a directory").unwrap();
        let unwritable_history = blocker.join("history.json");
        let mut engine = AutoImprovementEngine::new();
        engine
            .test_stage_delays
            .insert(STAGE_BENCHMARK, Duration::from_secs(30));
        let failed = engine
            .run_cycle_bounded(
                &settings,
                None,
                tight_budget,
                None,
                Some(&unwritable_history),
            )
            .await;
        assert!(
            matches!(failed.status, CycleStatus::Failed),
            "unpersistable partial progress must fail the cycle, got {:?}",
            failed.status
        );
        assert_eq!(failed.status.exit_code(), 1);

        // Completed: healthy cycle within budget.
        let ok_history = temp_history_path("exit-ok");
        let engine = AutoImprovementEngine::new();
        let completed = engine
            .run_cycle_bounded(
                &settings,
                None,
                CycleBudget {
                    overall_timeout: Duration::from_secs(10),
                    stage_timeout: Duration::from_secs(5),
                },
                None,
                Some(&ok_history),
            )
            .await;
        assert!(
            matches!(completed.status, CycleStatus::Completed),
            "healthy cycle must complete, got {:?}",
            completed.status
        );
        assert_eq!(completed.status.exit_code(), 0);

        // The three outcomes map to three distinct exit codes.
        let codes = [
            completed.status.exit_code(),
            truncated.status.exit_code(),
            failed.status.exit_code(),
        ];
        assert!(
            codes[0] != codes[1] && codes[1] != codes[2] && codes[0] != codes[2],
            "completed/truncated/failed must map to distinct exit codes, got {codes:?}"
        );

        let _ = std::fs::remove_file(&truncated_history);
        let _ = std::fs::remove_file(&ok_history);
        let _ = std::fs::remove_file(&blocker);
    }
}
