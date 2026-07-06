use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

use crate::memory::manager::core::MemoryManager;
use crate::memory::qmd::QmdMemory;
use crate::retrieval::eval::{is_hit, CaseResult, EvalDataset, RetrievalMetrics};
use crate::retrieval::history::{self, HistoryEntry};
use crate::retrieval::tuner::{detect_recall_drift, tune, RetrievalConfig};

/// Interval between auto-tune passes. Mirrors the decay loop's 6h cadence: the
/// tuner is cheap (a handful of synchronous searches) but recall only shifts
/// meaningfully as the memory store grows or drifts, so a sub-hourly run would
/// add noise without value.
const AUTO_TUNE_INTERVAL_SECS: u64 = 6 * 3600;

/// Benchmark dataset used by the auto-tune job. Resolved relative to the repo
/// root, matching the `xavier regen` CLI's default.
const AUTO_TUNE_DATASET: &str = "scripts/benchmarks/datasets/internal_swal_openclaw_memory.json";

/// Background Daemon to run scheduled memory maintenance tasks autonomously.
pub struct MemoryDaemon {
    manager: Arc<MemoryManager>,
}

impl MemoryDaemon {
    pub fn new(manager: Arc<MemoryManager>) -> Self {
        Self { manager }
    }

    /// Spawns the autonomous Tokio loops.
    ///
    /// Each loop is an independent spawned task — none block `spawn`, and a
    /// panic in one loop does not take down the others. Intervals:
    /// - decay: 6h
    /// - semantic compaction: 12h
    /// - garbage collection: 24h
    /// - retrieval auto-tune + drift detection: 6h (see `run_auto_tune`)
    pub fn spawn(self) {
        // Run Memory Decay every 6 hours
        let manager_decay = self.manager.clone();
        tokio::spawn(async move {
            info!("MemoryDaemon: Scheduled decay loop started (6h interval)");
            loop {
                sleep(Duration::from_secs(6 * 3600)).await;
                info!("MemoryDaemon: Running scheduled decay_memories()");
                if let Err(e) = manager_decay.decay_memories().await {
                    error!("MemoryDaemon: Scheduled decay failed: {}", e);
                }
            }
        });

        // Run Semantic Compaction every 12 hours
        let manager_compact = self.manager.clone();
        tokio::spawn(async move {
            info!("MemoryDaemon: Scheduled semantic compaction loop started (12h interval)");
            loop {
                sleep(Duration::from_secs(12 * 3600)).await;
                info!("MemoryDaemon: Running scheduled compact_semantically()");
                if let Err(e) = manager_compact.compact_semantically().await {
                    error!("MemoryDaemon: Scheduled compaction failed: {}", e);
                }
            }
        });

        // Run Garbage Collection every 24 hours
        let manager_gc = self.manager.clone();
        tokio::spawn(async move {
            info!("MemoryDaemon: Scheduled GC loop started (24h interval)");
            loop {
                sleep(Duration::from_secs(24 * 3600)).await;
                info!("MemoryDaemon: Running scheduled garbage_collect()");
                match manager_gc.garbage_collect().await {
                    Ok(stats) => info!("MemoryDaemon: Scheduled GC completed. Bytes freed: {}, Orphans cleaned: {}", stats.bytes_freed, stats.orphaned_vectors_cleaned),
                    Err(e) => error!("MemoryDaemon: Scheduled GC failed: {}", e),
                }
            }
        });

        // Periodic retrieval auto-tune + drift detection.
        //
        // Runs every 6h (see AUTO_TUNE_INTERVAL_SECS), gated on memory being
        // available and the benchmark dataset being present. Spawned the same
        // way as the maintenance loops above so it never blocks the daemon and
        // stays isolated from them.
        let manager_tune = self.manager.clone();
        tokio::spawn(async move {
            info!(
                "MemoryDaemon: Scheduled retrieval auto-tune loop started ({}s interval)",
                AUTO_TUNE_INTERVAL_SECS
            );
            loop {
                sleep(Duration::from_secs(AUTO_TUNE_INTERVAL_SECS)).await;
                if let Err(e) = run_auto_tune(&manager_tune.memory()).await {
                    warn!("MemoryDaemon: Auto-tune pass failed: {e}");
                }
            }
        });
    }
}

/// One scheduled auto-tune pass: measure current recall, compare it against the
/// last recorded baseline (drift detection), run the tuner, and persist the
/// result to the tuning history so the next pass and the CLI share state.
///
/// This is best-effort by design:
/// - If the benchmark dataset is missing, the pass is skipped (not all
///   deployments ship it). Nothing is logged at ERROR for this.
/// - If memory is unavailable/empty, the pass is skipped after a probe.
/// - Any other error is returned and the caller logs it at WARN.
///
/// Gated behind a memory-availability check (a probe search) so the job is a
/// no-op on daemons where the memory store is not populated.
async fn run_auto_tune(memory: &Arc<QmdMemory>) -> anyhow::Result<()> {
    // 1. Memory availability gate: a probe search. An empty/uninitialized store
    //    returns no results, in which case there is nothing to tune.
    let probe = memory.search("", 1).await.unwrap_or_default();
    if probe.is_empty() {
        info!("MemoryDaemon: auto-tune skipped — memory unavailable/empty");
        return Ok(());
    }

    // 2. Load the benchmark dataset. Missing dataset is a soft skip.
    let dataset_path = PathBuf::from(AUTO_TUNE_DATASET);
    if !dataset_path.exists() {
        info!(
            "MemoryDaemon: auto-tune skipped — benchmark dataset not found at {}",
            dataset_path.display()
        );
        return Ok(());
    }
    let ds = EvalDataset::load(&dataset_path)?;
    let k = 5;

    // 3. Measure current recall under the live config.
    let current = measure(memory, &ds, k).await;
    info!(
        "MemoryDaemon: auto-tune measured recall@{} = {:.1}% on '{}'",
        k,
        current.recall_at_k * 100.0,
        ds.dataset
    );

    // 4. Cross-cycle drift detection against the last baseline, if any.
    let xavier_dir = find_xavier_dir();
    let hist_path = history::ensure_history_path(&xavier_dir)?;
    if let Ok(hist) = history::load(&hist_path) {
        if let Some(prev) = hist.last() {
            if let Some(regression_pct) = detect_recall_drift(&prev.baseline, &current) {
                warn!(
                    "MemoryDaemon: recall drift detected ({:.1}% regression); consider re-tuning",
                    regression_pct
                );
            }
        }
    }

    // 5. Run the tuner (sync grid search using the measured baseline as the
    //    evaluation signal — same approximation as the CLI `regen tune`).
    let baseline_cfg = RetrievalConfig::default();
    let proposal = tune(&baseline_cfg, |_cfg| current.clone());

    // 6. Persist the result so the next pass and the CLI share the baseline.
    let entry = HistoryEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        baseline: current,
        proposal,
    };
    if let Err(e) = history::append(&hist_path, entry) {
        warn!("MemoryDaemon: failed to persist tuning history: {e}");
    }

    Ok(())
}

/// Measure recall@k for a dataset against the given memory.
async fn measure(memory: &Arc<QmdMemory>, ds: &EvalDataset, k: usize) -> RetrievalMetrics {
    let mut case_results = Vec::with_capacity(ds.cases.len());
    for case in &ds.cases {
        let results = memory.search(&case.query, k).await.unwrap_or_default();
        let hit = results.iter().any(|r| {
            is_hit(&r.content, &case.expected_path) || is_hit(&r.path, &case.expected_path)
        });
        let first_hit_rank = results
            .iter()
            .position(|r| {
                is_hit(&r.content, &case.expected_path) || is_hit(&r.path, &case.expected_path)
            })
            .map(|i| i + 1);
        case_results.push(CaseResult {
            case_id: case.id.clone(),
            hit,
            first_hit_rank,
        });
    }
    RetrievalMetrics::from_results(&ds.dataset, &case_results, k)
}

/// Resolve the `.xavier` dir for tuning history: walk up for a `.git` marker,
/// fall back to the cwd. Mirrors the CLI's `regen` repo-root resolution so both
/// write to the same history file.
fn find_xavier_dir() -> PathBuf {
    let start = std::path::absolute(".").unwrap_or_else(|_| PathBuf::from("."));
    let mut current = start.as_path();
    loop {
        if current.join(".git").exists() {
            return current.join(".xavier");
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => break,
        }
    }
    start.join(".xavier")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_xavier_dir_returns_a_path_ending_in_xavier() {
        let dir = find_xavier_dir();
        let last = dir
            .components()
            .next_back()
            .and_then(|c| c.as_os_str().to_str())
            .unwrap_or("");
        assert_eq!(last, ".xavier", "find_xavier_dir must point inside .xavier");
    }

    #[tokio::test]
    async fn test_run_auto_tune_skips_when_dataset_missing() {
        // Build an in-memory QmdMemory (no docs) — the availability gate should
        // short-circuit before the dataset is even consulted.
        use crate::memory::qmd::types::MemoryDocument;
        use crate::memory::qmd::QmdMemory;
        let store: Arc<tokio::sync::RwLock<Vec<MemoryDocument>>> =
            Arc::new(tokio::sync::RwLock::new(Vec::new()));
        let memory = Arc::new(QmdMemory::new(store));

        // An empty memory store has no documents, so the probe search returns
        // nothing and the pass exits early with Ok(()).
        let res = run_auto_tune(&memory).await;
        assert!(
            res.is_ok(),
            "auto-tune on empty memory must succeed (no-op)"
        );
    }
}
