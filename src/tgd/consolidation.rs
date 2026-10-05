//! Nightly Consolidation Scheduler for TGD and Memory.
//!
//! Manages background execution of memory consolidation and TGD rule generation
//! on a cron-like schedule.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

pub use crate::domain::cycle_breaks::w30_08::{NightlyTgd, SchedulerState};

use crate::consolidation::ConsolidationTask;
use crate::tgd::TgdEngine;
use crate::workspace::WorkspaceContext;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProgressReport {
    pub processed: usize,
    pub total: usize,
    pub eta_secs: u64,
    pub errors: usize,
    pub status: String,
    /// Why the cycle ended in `failed`/`stalled`; empty otherwise.
    ///
    /// Without it `/health` shows a non-`completed` status with nothing to
    /// explain it, and an operator cannot tell a slow cycle from a dead one.
    #[serde(default)]
    pub reason: String,
}

/// Wall-clock gap enforced between two consolidation cycles.
///
/// The cron expression alone is not a rate limit. `Schedule::upcoming` is
/// rebuilt from the current instant on every iteration, so a schedule that
/// lands on the current second re-enters immediately and the task becomes a
/// busy loop: one core pinned, no progress, `/health` frozen on `running`.
/// This floor is what makes the loop rate-limited whatever the schedule says.
pub const MIN_CYCLE_INTERVAL: Duration = Duration::from_secs(60);

/// Upper bound for a single consolidation cycle.
///
/// A cycle that overruns it is dropped rather than awaited forever, and the
/// shared progress report is flipped to `stalled` so the stall is visible
/// instead of silent.
pub const CYCLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

pub struct TgdConsolidationScheduler {
    workspace: WorkspaceContext,
    tgd_engine: Option<TgdEngine>,
    progress: Arc<RwLock<ProgressReport>>,
    state_path: PathBuf,
    cancellation_token: CancellationToken,
}

impl TgdConsolidationScheduler {
    /// New.
    pub fn new(
        workspace: WorkspaceContext,
        tgd_engine: Option<TgdEngine>,
        state_path: PathBuf,
    ) -> Self {
        Self {
            workspace,
            tgd_engine,
            progress: Arc::new(RwLock::new(ProgressReport::default())),
            state_path,
            cancellation_token: CancellationToken::new(),
        }
    }

    /// Progress.
    pub fn progress(&self) -> Arc<RwLock<ProgressReport>> {
        Arc::clone(&self.progress)
    }

    /// Cancel.
    pub fn cancel(&self) {
        self.cancellation_token.cancel();
    }

    /// Spawn.
    pub async fn spawn(self: Arc<Self>, cron_expr: String) {
        let scheduler = Arc::clone(&self);
        tokio::spawn(async move {
            info!(
                "🚀 TGD Consolidation Scheduler started with cron: {}",
                cron_expr
            );

            let schedule: cron::Schedule = match cron::Schedule::from_str(&cron_expr) {
                Ok(s) => s,
                Err(e) => {
                    error!("❌ Invalid cron expression: {}", e);
                    return;
                }
            };

            run_cycle_loop(
                schedule,
                scheduler.cancellation_token.clone(),
                Arc::clone(&scheduler.progress),
                CYCLE_TIMEOUT,
                MIN_CYCLE_INTERVAL,
                || scheduler.run_once(),
            )
            .await;
            info!("🛑 TGD consolidation scheduler stopped");
        });
    }

    /// Run once.
    pub async fn run_once(&self) -> anyhow::Result<()> {
        match self.run_cycle().await {
            Ok(()) => Ok(()),
            Err(error) => {
                mark_unfinished(&self.progress, "failed", &error.to_string()).await;
                Err(error)
            }
        }
    }

    /// One consolidation pass. Bound by the caller via [`CYCLE_TIMEOUT`].
    async fn run_cycle(&self) -> anyhow::Result<()> {
        let start = std::time::Instant::now();
        {
            let mut p = self.progress.write().await;
            *p = ProgressReport {
                status: "running".to_string(),
                ..Default::default()
            };
        }

        let task = ConsolidationTask {
            enable_tgd_in_consolidation: true,
            ..Default::default()
        };

        // 1. Run Memory Consolidation
        info!("🧠 Phase 1: Memory Consolidation...");
        let stats = task
            .consolidate(&self.workspace, Some(Arc::clone(&self.progress)))
            .await?;

        // 2. Run TGD if enabled
        info!("🧠 Phase 2: TGD Rule Generation...");
        task.run_tgd_if_enabled(&self.workspace, self.tgd_engine.as_ref())
            .await?;

        // 3. Run TGD Memory Refinement
        info!("🧠 Phase 3: TGD Memory Refinement...");
        let refinement_stats = task
            .run_tgd_memory_refinement(&self.workspace, self.tgd_engine.as_ref())
            .await?;

        let mut final_stats = stats;
        final_stats.selected += refinement_stats.selected;
        final_stats.memories_refined = refinement_stats.memories_refined;
        final_stats.avg_score_improvement = refinement_stats.avg_score_improvement;
        final_stats.errors += refinement_stats.errors;

        let duration = start.elapsed();
        let state = SchedulerState {
            last_run_at: Some(Utc::now()),
            last_duration_ms: duration.as_millis() as u64,
            items_processed: final_stats.selected,
        };

        self.save_state(&state).await?;

        {
            let mut p = self.progress.write().await;
            publish_completed(&mut p, final_stats.selected, final_stats.errors);
        }

        info!(
            "✅ Scheduled consolidation completed in {}ms",
            duration.as_millis()
        );

        // Log results to chronicle (via memory document)
        let date = Utc::now().format("%Y-%m-%d").to_string();
        let report_content = format!(
            "# Nightly TGD Consolidation Report\n\n- Date: {}\n- Duration: {}ms\n- Items Processed: {}\n- Memories Refined: {}\n- Avg Improvement: {:.4}\n- Errors: {}\n",
            date,
            duration.as_millis(),
            final_stats.selected,
            final_stats.memories_refined,
            final_stats.avg_score_improvement,
            final_stats.errors
        );

        let report_path = format!("logs/tgd/report-{}.md", date);
        self.workspace
            .workspace
            .memory
            .add_document_typed(
                report_path,
                report_content,
                serde_json::json!({
                    "memory_kind": "tgd_report",
                    "date": date,
                    "stats": final_stats
                }),
                None,
            )
            .await?;

        Ok(())
    }

    async fn save_state(&self, state: &SchedulerState) -> anyhow::Result<()> {
        if let Some(parent) = self.state_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let data = serde_json::to_vec_pretty(state)?;
        tokio::fs::write(&self.state_path, data).await?;
        Ok(())
    }

    /// Load state.
    pub async fn load_state(&self) -> anyhow::Result<SchedulerState> {
        if !self.state_path.exists() {
            return Ok(SchedulerState::default());
        }
        let data = tokio::fs::read_to_string(&self.state_path).await?;
        let state = serde_json::from_str(&data)?;
        Ok(state)
    }
}

/// The scheduler loop, with the cycle and both bounds injected.
///
/// `cycle` is a future *factory*, not a future: a cycle that timed out is
/// dropped mid-flight, and the next cycle has to start from scratch. The two
/// bounds are parameters so a test can shrink them without waiting real
/// minutes; production passes [`CYCLE_TIMEOUT`] and [`MIN_CYCLE_INTERVAL`]:
///
///   * `cycle_timeout` cancels a cycle that stopped advancing, so a hang is
///     reported instead of awaited forever;
///   * `min_interval` separates two cycles, so a schedule that lands on the
///     current instant cannot turn this into a spin loop.
async fn run_cycle_loop<F, Fut>(
    schedule: cron::Schedule,
    cancellation_token: CancellationToken,
    progress: Arc<RwLock<ProgressReport>>,
    cycle_timeout: Duration,
    min_interval: Duration,
    cycle: F,
) where
    F: Fn() -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    while let Some(next) = schedule.upcoming(Utc).next() {
        let now = Utc::now();
        if next > now {
            let sleep_duration = next
                .signed_duration_since(now)
                .to_std()
                .unwrap_or(Duration::from_secs(0));
            info!("📅 Next TGD consolidation scheduled for: {}", next);

            tokio::select! {
                _ = tokio::time::sleep(sleep_duration) => {},
                _ = cancellation_token.cancelled() => {
                    info!("🛑 TGD consolidation scheduler cancelled");
                    return;
                }
            }
        }

        if cancellation_token.is_cancelled() {
            return;
        }

        info!("⚙️ Starting scheduled TGD consolidation...");
        match tokio::time::timeout(cycle_timeout, cycle()).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                error!("❌ Scheduled consolidation failed: {}", error);
                mark_unfinished(&progress, "failed", &error.to_string()).await;
            }
            Err(_elapsed) => {
                error!(
                    "⏱️ Scheduled consolidation exceeded {}s and was cancelled",
                    cycle_timeout.as_secs()
                );
                mark_unfinished(
                    &progress,
                    "stalled",
                    &format!("cycle exceeded {}", cycle_timeout.as_secs()),
                )
                .await;
            }
        }

        // Floor between two cycles: a schedule that lands on the current instant
        // would otherwise re-enter immediately and turn this into a spin loop.
        tokio::select! {
            _ = tokio::time::sleep(min_interval) => {},
            _ = cancellation_token.cancelled() => {
                info!("🛑 TGD consolidation scheduler cancelled");
                return;
            }
        }
    }
}

/// Flag a cycle that did not finish on the shared progress report.
///
/// `progress` is the same `Arc` handed to `HEALTH.set_tgd_progress`, so this is
/// what `/health` reports: a cycle that overran [`CYCLE_TIMEOUT`] reads as
/// `stalled` and one that returned an error as `failed`, instead of both
/// leaving `/health` frozen on `running` with no explanation. `processed` is
/// left as-is so a non-completed cycle stays visibly incomplete.
async fn mark_unfinished(progress: &Arc<RwLock<ProgressReport>>, status: &str, reason: &str) {
    let mut p = progress.write().await;
    p.status = status.to_string();
    p.reason = reason.to_string();
    p.eta_secs = 0;
}

/// Publish the end state of a finished cycle.
///
/// `consolidate` publishes a *running estimate* whose last increment stops
/// short of `total`, so copying only `processed` left every finished cycle
/// reading as permanently behind on `/health`. Both counters come from the
/// same final stat here, and the invariant is `processed == total` whenever
/// the status is `completed` — the state a caller can act on.
fn publish_completed(progress: &mut ProgressReport, selected: usize, errors: usize) {
    progress.status = "completed".to_string();
    progress.total = selected;
    progress.processed = selected;
    progress.errors = errors;
    progress.eta_secs = 0;
    progress.reason.clear();
}

/// Run a standalone nightly TGD consolidation: updates .xavier/tgd.md
/// with new generated rules and runs memory refinement.
pub async fn run_nightly_tgd() -> anyhow::Result<()> {
    use crate::consolidation::ConsolidationTask;
    use crate::workspace::{WorkspaceConfig, WorkspaceContext, WorkspaceState};

    info!("🌙 Running nightly TGD consolidation...");

    // Build a minimal workspace context from env / defaults
    let workspace_id =
        std::env::var("XAVIER_DEFAULT_WORKSPACE_ID").unwrap_or_else(|_| "default".to_string());
    let root = std::env::var("XAVIER_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    let workspace_state = WorkspaceState::new(
        WorkspaceConfig {
            id: workspace_id.clone(),
            token: std::env::var("XAVIER_TOKEN").unwrap_or_default(),
            plan: crate::workspace::PlanTier::Personal,
            memory_backend: crate::memory::store::MemoryBackend::Sqlite,
            storage_limit_bytes: None,
            request_limit: None,
            request_unit_limit: None,
            embedding_provider_mode: crate::workspace::EmbeddingProviderMode::BringYourOwn,
            managed_google_embeddings: false,
            sync_policy: crate::workspace::SyncPolicy::CloudMirror,
            protocol: Default::default(),
            dedup: crate::settings::types::DedupSettings::default(),
        },
        crate::agents::RuntimeConfig::default(),
        root.join(".xavier"),
    )
    .await?;
    let workspace = WorkspaceContext {
        workspace_id,
        workspace: std::sync::Arc::new(workspace_state),
    };

    let task = ConsolidationTask {
        enable_tgd_in_consolidation: true,
        ..Default::default()
    };

    // Create a TGD engine from environment config
    let tgd_engine =
        crate::tgd::TgdEngine::new(crate::agents::provider::ModelProviderClient::from_env());

    // Run TGD rule generation
    task.run_tgd_if_enabled(&workspace, Some(&tgd_engine))
        .await?;

    // Run TGD memory refinement
    let stats = task
        .run_tgd_memory_refinement(&workspace, Some(&tgd_engine))
        .await?;

    // Update .xavier/tgd.md with latest status
    let tgd_status_path = root.join(".xavier/tgd.md");
    if let Some(parent) = tgd_status_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let report = format!(
        "# Nightly TGD Report\n\n- Time: {}\n- Memories refined: {}\n- Avg score improvement: {:.4}\n- Errors: {}\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
        stats.memories_refined,
        stats.avg_score_improvement,
        stats.errors,
    );
    tokio::fs::write(&tgd_status_path, report).await?;

    info!(
        "✅ Nightly TGD complete — {} memories refined",
        stats.memories_refined
    );
    Ok(())
}

impl NightlyTgd for crate::memory::manager::MemoryManager {
    async fn run_nightly_tgd(&self) -> anyhow::Result<()> {
        run_nightly_tgd().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::Instant;

    // ── #2732: a cycle that stops advancing must not spin the loop ────
    // `run_once` used to be awaited inline with no timeout and no gap, so a
    // cycle that stopped advancing held the task forever, `/health` stayed
    // frozen on `running`, and a schedule landing on the current instant
    // re-entered immediately: one core pinned at ~380k read syscalls/s.
    //
    // These tests drive a real clock rather than a paused one: `tokio`'s
    // `pause`/`advance` live behind the `test-util` feature, which this crate
    // does not enable and enabling it is a workspace-wide manifest change
    // outside this change's file scope. The bounds are therefore injected at
    // milliseconds instead of minutes, and every wait below polls for the
    // state it needs with a generous budget rather than sleeping a fixed
    // amount — so a loaded CI box makes the test slower, never wrong.

    /// Bound on one cycle under test.
    const TEST_CYCLE_TIMEOUT: Duration = Duration::from_millis(30);

    /// Minimum gap between two cycles under test.
    const TEST_MIN_INTERVAL: Duration = Duration::from_millis(30);

    /// Budget for a state the loop must reach within a couple of cron ticks.
    ///
    /// The schedule ticks once per second, so two cycles need at most ~2.1s.
    const TEST_BUDGET: Duration = Duration::from_secs(20);

    /// The worst case a cron expression can express: a tick every second.
    fn every_second() -> cron::Schedule {
        cron::Schedule::from_str("* * * * * *").expect("valid cron expression")
    }

    /// Poll `ready` until it holds or [`TEST_BUDGET`] expires.
    async fn wait_until(mut ready: impl FnMut() -> bool) -> bool {
        tokio::time::timeout(TEST_BUDGET, async {
            while !ready() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .is_ok()
    }

    #[tokio::test]
    async fn a_cycle_that_never_advances_is_bounded_and_rate_limited() {
        let calls = Arc::new(Mutex::new(Vec::<Instant>::new()));
        let progress = Arc::new(RwLock::new(ProgressReport::default()));
        let cancellation_token = CancellationToken::new();

        let handle = tokio::spawn({
            let calls = Arc::clone(&calls);
            let cancellation_token = cancellation_token.clone();
            let progress = Arc::clone(&progress);
            async move {
                run_cycle_loop(
                    every_second(),
                    cancellation_token,
                    progress,
                    TEST_CYCLE_TIMEOUT,
                    TEST_MIN_INTERVAL,
                    || {
                        let calls = Arc::clone(&calls);
                        async move {
                            calls.lock().unwrap().push(Instant::now());
                            // A cycle that never finishes: no progress, ever.
                            std::future::pending::<()>().await;
                            Ok(())
                        }
                    },
                )
                .await;
            }
        });

        let came_back = wait_until(|| calls.lock().unwrap().len() >= 3).await;
        cancellation_token.cancel();
        let _ = handle.await;

        let starts = calls.lock().unwrap().clone();
        assert!(
            came_back && starts.len() >= 3,
            "a bounded cycle must let the loop come back around; saw {} cycles, \
             which is the unbounded `run_once().await` behaviour this closes",
            starts.len()
        );
        for pair in starts.windows(2) {
            let gap = pair[1].duration_since(pair[0]);
            assert!(
                gap >= TEST_MIN_INTERVAL,
                "cycles re-entered {gap:?} apart; the loop must wait at least \
                 {TEST_MIN_INTERVAL:?} between them or it is a spin loop"
            );
        }
    }

    #[tokio::test]
    async fn a_stalled_cycle_is_reported_on_the_health_progress() {
        let progress = Arc::new(RwLock::new(ProgressReport {
            status: "running".to_string(),
            total: 7,
            processed: 2,
            ..Default::default()
        }));
        let cancellation_token = CancellationToken::new();

        let handle = tokio::spawn({
            let cancellation_token = cancellation_token.clone();
            let progress = Arc::clone(&progress);
            async move {
                run_cycle_loop(
                    every_second(),
                    cancellation_token,
                    progress,
                    TEST_CYCLE_TIMEOUT,
                    TEST_MIN_INTERVAL,
                    || async {
                        std::future::pending::<()>().await;
                        Ok(())
                    },
                )
                .await;
            }
        });

        let flagged = wait_until(|| progress.try_read().is_ok_and(|p| p.status == "stalled")).await;
        let report = progress.read().await.clone();
        cancellation_token.cancel();
        let _ = handle.await;

        // `progress` is the same `Arc` that `src/cli/server.rs` hands to
        // `HEALTH.set_tgd_progress`, so this is what `/health` reports.
        assert!(
            flagged,
            "an overrun cycle must be flagged, not left `running`"
        );
        assert_eq!(report.status, "stalled");
        assert_eq!(
            report.reason,
            format!("cycle exceeded {}", TEST_CYCLE_TIMEOUT.as_secs()),
            "a stall must say which bound killed it"
        );
        assert_eq!(report.eta_secs, 0);
        assert!(
            report.processed < report.total,
            "a stalled cycle stays visibly incomplete instead of claiming to be done"
        );
    }

    #[tokio::test]
    async fn a_finished_cycle_reports_processed_equal_to_total() {
        // Mid-flight state as `consolidate` leaves it: `processed` is always
        // short of `total`, which used to survive into the completed report.
        let progress = Arc::new(RwLock::new(ProgressReport {
            status: "running".to_string(),
            total: 12,
            processed: 9,
            eta_secs: 900,
            errors: 0,
            reason: "stale earlier failure".to_string(),
        }));

        let mut guard = progress.write().await;
        publish_completed(&mut guard, 12, 2);
        drop(guard);

        let report = progress.read().await;
        assert_eq!(report.status, "completed");
        assert_eq!(report.processed, report.total);
        assert_eq!(report.processed, 12);
        assert_eq!(report.eta_secs, 0);
        assert_eq!(report.errors, 2);
        assert_eq!(report.reason, "", "a clean cycle clears the old reason");
    }

    #[tokio::test]
    async fn a_failing_cycle_reports_why_instead_of_staying_running() {
        let progress = Arc::new(RwLock::new(ProgressReport {
            status: "running".to_string(),
            total: 5,
            processed: 1,
            eta_secs: 300,
            errors: 0,
            reason: String::new(),
        }));

        mark_unfinished(&progress, "failed", "embedding unreachable").await;

        let report = progress.read().await;
        assert_eq!(report.status, "failed");
        assert_eq!(report.reason, "embedding unreachable");
        assert_eq!(report.eta_secs, 0);
        assert!(
            report.processed < report.total,
            "a failed cycle stays visibly incomplete instead of claiming to be done"
        );
    }
}
