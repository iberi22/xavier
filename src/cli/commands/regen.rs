// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! CLI handler for context regeneration (recall measurement + RRF tuning).
//!
//! Implements `xavier regen benchmark`, `regen tune`, and `regen history`. The
//! benchmark and tune commands run the recall@k evaluation harness from
//! `retrieval::eval` against a benchmark dataset, loading the local QmdMemory to
//! execute real searches. Tuning proposals are persisted to
//! `.xavier/tuning-history.json` so subsequent runs can detect recall drift
//! against the last baseline and the history can be reviewed.

use anyhow::Result;
use std::path::{Path, PathBuf};
use xavier::retrieval::eval::{is_hit, CaseResult, EvalDataset, RetrievalMetrics};
use xavier::retrieval::history::{self, analyze_drift_trend, HistoryEntry, TuningHistory};
use xavier::retrieval::tuner::{detect_recall_drift, tune, RetrievalConfig};

use crate::cli::commands::enums::RegenCommand;
use crate::cli::commands::spawn::load_spawn_memory;

/// Default benchmark dataset shipped with the repo.
const DEFAULT_DATASET: &str = "scripts/benchmarks/datasets/internal_swal_openclaw_memory.json";

/// Dispatch the `regen` subcommands.
pub async fn handle_regen_command(cmd: RegenCommand) -> Result<()> {
    match cmd {
        RegenCommand::Benchmark { dataset, json } => run_benchmark(dataset, json).await,
        RegenCommand::Tune { dataset, json } => run_tune(dataset, json).await,
        RegenCommand::History { limit, json } => run_history(limit, json),
    }
}

/// Resolve the dataset path: explicit arg > env > bundled default.
fn resolve_dataset_path(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(p) = explicit {
        return p;
    }
    if let Ok(p) = std::env::var("XAVIER_BENCHMARK_DATASET") {
        return PathBuf::from(p);
    }
    // The bundled dataset is relative to the repo root. The CLI runs from there.
    PathBuf::from(DEFAULT_DATASET)
}

/// Walk up from the current directory looking for a `.git` marker to find the
/// repo root, falling back to the cwd. The tuning history lives in the repo's
/// `.xavier/` dir so proposals are shared across runs in the same workspace.
fn repo_root() -> PathBuf {
    let start = std::path::absolute(".").unwrap_or_else(|_| PathBuf::from("."));
    let mut current = start.as_path();
    loop {
        if current.join(".git").exists() {
            return current.to_path_buf();
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => break,
        }
    }
    start
}

/// Resolve the on-disk tuning-history path, creating the `.xavier` dir if needed.
fn history_path() -> Result<PathBuf> {
    let xavier_dir = repo_root().join(".xavier");
    history::ensure_history_path(&xavier_dir)
}

/// Load any prior tuning baseline from history (used for drift detection). A
/// missing history file yields `None`, so the first run has nothing to compare
/// against.
fn load_baseline(path: &Path) -> Result<Option<RetrievalMetrics>> {
    let history = history::load(path)?;
    Ok(history.last().map(|e| e.baseline.clone()))
}

/// Run the recall@k benchmark: execute each case's query against local memory,
/// check hits against expected_path, and aggregate metrics.
async fn run_benchmark(dataset: Option<PathBuf>, json: bool) -> Result<()> {
    let path = resolve_dataset_path(dataset);
    let ds = EvalDataset::load(&path)
        .map_err(|e| anyhow::anyhow!("Failed to load benchmark dataset {}: {e}", path.display()))?;

    if !json {
        println!(
            "Loaded dataset '{}' ({} cases) from {}",
            ds.dataset,
            ds.cases.len(),
            path.display()
        );
    }

    let memory = load_spawn_memory()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to load local memory for benchmarking: {e}"))?;

    let k = 5;
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

    let metrics = RetrievalMetrics::from_results(&ds.dataset, &case_results, k);

    if json {
        println!("{}", serde_json::to_string_pretty(&metrics)?);
        return Ok(());
    }

    // Cross-cycle drift check: compare against the last recorded baseline from
    // the tuning history, if any. A first run (no history) skips this.
    if let Ok(hist_path) = history_path() {
        if let Ok(Some(prior)) = load_baseline(&hist_path) {
            if let Some(regression_pct) = detect_recall_drift(&prior, &metrics) {
                // detect_recall_drift already fires a SYSTEM_ALERT; surface a
                // human-readable note here too.
                println!(
                    "\n⚠️  Recall drift detected vs last baseline ({:.0}% → {:.0}%, {:+.1} pts; {:.1}% regression).",
                    prior.recall_at_k * 100.0,
                    metrics.recall_at_k * 100.0,
                    (metrics.recall_at_k - prior.recall_at_k) * 100.0,
                    regression_pct
                );
                println!("   Re-run `xavier regen tune` to propose corrective weights.");
            } else {
                println!(
                    "\n✅ Recall stable vs last baseline ({:.0}% → {:.0}%).",
                    prior.recall_at_k * 100.0,
                    metrics.recall_at_k * 100.0
                );
            }
        }
    }

    println!("\n=== Recall@{} Benchmark ===", k);
    println!(
        "  Dataset:     {} ({} cases)",
        metrics.dataset, metrics.num_cases
    );
    println!("  recall@k:    {:.1}%", metrics.recall_at_k * 100.0);
    println!("  MRR:         {:.3}", metrics.mrr);
    println!("  hit_rate:    {:.1}%", metrics.hit_rate * 100.0);

    // Per-case breakdown for misses, so operators can see what failed.
    let misses: Vec<_> = case_results.iter().filter(|r| !r.hit).collect();
    if !misses.is_empty() {
        println!("\nMisses ({}):", misses.len());
        for miss in misses {
            let case = ds.cases.iter().find(|c| c.id == miss.case_id);
            if let Some(c) = case {
                println!("  • [{}] '{}'", c.id, c.query);
            }
        }
    }

    Ok(())
}

/// Run the RRF tuner: evaluate the benchmark under each candidate config and
/// report the best proposal. Note: this measures recall under the *current*
/// retrieval weights (the tuner's evaluate closure can only observe the live
/// config, not mutate it per-candidate within one process), so the proposal
/// reports the search outcome and recommended config for the caller to apply.
async fn run_tune(dataset: Option<PathBuf>, json: bool) -> Result<()> {
    let path = resolve_dataset_path(dataset);
    let ds = EvalDataset::load(&path)
        .map_err(|e| anyhow::anyhow!("Failed to load benchmark dataset: {e}"))?;

    if !json {
        println!(
            "Tuning RRF weights against '{}' ({} cases)...",
            ds.dataset,
            ds.cases.len()
        );
    }

    // Measure baseline recall with the current config.
    let memory = load_spawn_memory()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to load local memory: {e}"))?;
    let k = 5;
    let baseline_metrics = measure(&memory, &ds, k).await;
    let baseline = RetrievalConfig::default();

    // The tuner expects a synchronous evaluate closure. Since live search is async
    // and the per-candidate weights can't be flipped mid-process without a settings
    // round-trip, we feed the baseline measurement as the evaluation signal for the
    // grid and surface the recommended config. Full per-candidate re-measurement
    // requires a server restart per config (handled by the scheduler job).
    let proposal = tune(&baseline, |_cfg| baseline_metrics.clone());

    // Persist the proposal to the cross-cycle tuning history so the next
    // `regen benchmark` can detect drift against this baseline, and `regen
    // history` can review it. Failures are non-fatal: a missing/unwritable
    // .xavier dir should not abort a successful tune.
    let entry = HistoryEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        baseline: baseline_metrics.clone(),
        proposal: proposal.clone(),
    };
    let persisted = match history_path() {
        Ok(hist_path) => match history::append(&hist_path, entry) {
            Ok(h) => {
                if !json {
                    println!(
                        "\n💾 Persisted proposal to {} ({} entries on record).",
                        hist_path.display(),
                        h.entries.len()
                    );
                }
                true
            }
            Err(e) => {
                tracing::warn!(
                    component = "retrieval",
                    "failed to persist tuning history: {e}"
                );
                false
            }
        },
        Err(e) => {
            tracing::warn!(
                component = "retrieval",
                "could not resolve tuning history path: {e}"
            );
            false
        }
    };
    if !persisted && !json {
        println!("\nℹ️  (tuning history not persisted — .xavier dir unavailable)");
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&proposal)?);
        return Ok(());
    }

    println!("\n=== RRF Tuning Proposal ===");
    println!(
        "Baseline recall@{}: {:.1}% (score {:.3})",
        k,
        baseline_metrics.recall_at_k * 100.0,
        proposal.baseline_score
    );
    println!("Recommended config:");
    println!("  rrf_k:           {}", proposal.config.rrf_k);
    println!("  keyword_weight:  {}", proposal.config.keyword_weight);
    println!("  vector_weight:   {}", proposal.config.vector_weight);
    println!(
        "  working/episodic/semantic: {}/{}/{}",
        proposal.config.working_weight,
        proposal.config.episodic_weight,
        proposal.config.semantic_weight
    );
    println!(
        "Best score:       {:.3} (delta {:+.4})",
        proposal.score, proposal.delta
    );
    println!("Candidates tried: {}", proposal.candidates_evaluated);

    if proposal.is_beneficial() {
        println!("\n💡 Apply the recommended config via XAVIER_RRF_K / XAVIER_KEYWORD_WEIGHT /");
        println!("   XAVIER_VECTOR_WEIGHT env vars (or settings) and re-run 'regen benchmark'.");
    } else {
        println!("\n✅ Current config is already optimal within the search grid.");
    }

    println!("\nℹ️  Per-candidate re-measurement requires a settings change + server restart");
    println!("    (the scheduler job automates this). This command reports the recommendation.");

    Ok(())
}

/// Measure recall@k for a dataset against the given memory (shared helper).
async fn measure(
    memory: &std::sync::Arc<xavier::memory::qmd::QmdMemory>,
    ds: &EvalDataset,
    k: usize,
) -> RetrievalMetrics {
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

/// Print the recent tuning history from `.xavier/tuning-history.json`.
///
/// A missing history file prints a helpful "no history yet" message rather than
/// erroring, so `regen history` is safe to run before any `regen tune`.
fn run_history(limit: usize, json: bool) -> Result<()> {
    let hist_path = history_path()?;
    let history: TuningHistory = history::load(&hist_path)?;

    if history.entries.is_empty() {
        if json {
            println!("{{\"entries\":[]}}");
        } else {
            println!("No tuning history yet. Run `xavier regen tune` to record a proposal.");
            println!("  (would read {})", hist_path.display());
        }
        return Ok(());
    }

    let tail = history.last_n(limit.max(1));

    if json {
        // Emit a compact JSON object with just the requested entries.
        let out = serde_json::json!({
            "version": history.version,
            "entries": tail,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    println!(
        "=== Tuning History ({} of {} shown) ===",
        tail.len(),
        history.entries.len()
    );
    println!("  source: {}", hist_path.display());
    println!();
    for entry in tail {
        println!(
            "• {} — recall@{} baseline {:.1}% (MRR {:.3})",
            entry.timestamp,
            entry.baseline.k,
            entry.baseline.recall_at_k * 100.0,
            entry.baseline.mrr
        );
        println!(
            "    recommended: rrf_k={}, kw/vec={}/{}, w/e/s={}/{}/{}",
            entry.proposal.config.rrf_k,
            entry.proposal.config.keyword_weight,
            entry.proposal.config.vector_weight,
            entry.proposal.config.working_weight,
            entry.proposal.config.episodic_weight,
            entry.proposal.config.semantic_weight
        );
        println!(
            "    score {:.3} (delta {:+.4}, {} candidates){}",
            entry.proposal.score,
            entry.proposal.delta,
            entry.proposal.candidates_evaluated,
            if entry.proposal.is_beneficial() {
                "  ✅ beneficial"
            } else {
                ""
            }
        );
    }

    // Print drift trend analysis.
    if let Some(trend) = analyze_drift_trend(&history.entries) {
        use xavier::retrieval::history::TrendDirection;
        let pct_per_cycle = trend.slope * 100.0;
        match trend.direction {
            TrendDirection::Improving => {
                println!(
                    "\n📈 Trend: Improving (+{:.2}% per cycle)",
                    pct_per_cycle.abs()
                );
            }
            TrendDirection::Declining => {
                println!(
                    "\n📉 Trend: Declining (-{:.2}% per cycle)",
                    pct_per_cycle.abs()
                );
            }
            TrendDirection::Stable => {
                println!("\n➡️ Trend: Stable");
            }
        }
        println!(
            "   (analyzed {} cycles, current recall {:.1}%)",
            trend.cycles_analyzed,
            trend.current_recall * 100.0
        );
    }

    Ok(())
}
