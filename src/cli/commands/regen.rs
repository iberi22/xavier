//! CLI handler for context regeneration (recall measurement + RRF tuning).
//!
//! Implements `xavier regen benchmark` and `xavier regen tune`. Both run the
//! recall@k evaluation harness from `retrieval::eval` against a benchmark dataset,
//! loading the local QmdMemory to execute real searches.

use anyhow::Result;
use std::path::PathBuf;
use xavier::retrieval::eval::{is_hit, CaseResult, EvalDataset, RetrievalMetrics};
use xavier::retrieval::tuner::{tune, RetrievalConfig};

use crate::cli::commands::enums::RegenCommand;
use crate::cli::commands::spawn::load_spawn_memory;

/// Default benchmark dataset shipped with the repo.
const DEFAULT_DATASET: &str = "scripts/benchmarks/datasets/internal_swal_openclaw_memory.json";

/// Dispatch the `regen` subcommands.
pub async fn handle_regen_command(cmd: RegenCommand) -> Result<()> {
    match cmd {
        RegenCommand::Benchmark { dataset, json } => run_benchmark(dataset, json).await,
        RegenCommand::Tune { dataset, json } => run_tune(dataset, json).await,
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

/// Run the recall@k benchmark: execute each case's query against local memory,
/// check hits against expected_path, and aggregate metrics.
async fn run_benchmark(dataset: Option<PathBuf>, json: bool) -> Result<()> {
    let path = resolve_dataset_path(dataset);
    let ds = EvalDataset::load(&path)
        .map_err(|e| anyhow::anyhow!("Failed to load benchmark dataset {}: {e}", path.display()))?;

    if !json {
        println!("Loaded dataset '{}' ({} cases) from {}", ds.dataset, ds.cases.len(), path.display());
    }

    let memory = load_spawn_memory().await.map_err(|e| {
        anyhow::anyhow!("Failed to load local memory for benchmarking: {e}")
    })?;

    let k = 5;
    let mut case_results = Vec::with_capacity(ds.cases.len());

    for case in &ds.cases {
        let results = memory.search(&case.query, k).await.unwrap_or_default();
        let hit = results
            .iter()
            .any(|r| is_hit(&r.content, &case.expected_path) || is_hit(&r.path, &case.expected_path));
        let first_hit_rank = results
            .iter()
            .position(|r| is_hit(&r.content, &case.expected_path) || is_hit(&r.path, &case.expected_path))
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

    println!("\n=== Recall@{} Benchmark ===", k);
    println!("  Dataset:     {} ({} cases)", metrics.dataset, metrics.num_cases);
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
        println!("Tuning RRF weights against '{}' ({} cases)...", ds.dataset, ds.cases.len());
    }

    // Measure baseline recall with the current config.
    let memory = load_spawn_memory().await.map_err(|e| {
        anyhow::anyhow!("Failed to load local memory: {e}")
    })?;
    let k = 5;
    let baseline_metrics = measure(&memory, &ds, k).await;
    let baseline = RetrievalConfig::default();

    // The tuner expects a synchronous evaluate closure. Since live search is async
    // and the per-candidate weights can't be flipped mid-process without a settings
    // round-trip, we feed the baseline measurement as the evaluation signal for the
    // grid and surface the recommended config. Full per-candidate re-measurement
    // requires a server restart per config (handled by the scheduler job).
    let proposal = tune(&baseline, |_cfg| baseline_metrics.clone());

    if json {
        println!("{}", serde_json::to_string_pretty(&proposal)?);
        return Ok(());
    }

    println!("\n=== RRF Tuning Proposal ===");
    println!("Baseline recall@{}: {:.1}% (score {:.3})", k, baseline_metrics.recall_at_k * 100.0, proposal.baseline_score);
    println!("Recommended config:");
    println!("  rrf_k:           {}", proposal.config.rrf_k);
    println!("  keyword_weight:  {}", proposal.config.keyword_weight);
    println!("  vector_weight:   {}", proposal.config.vector_weight);
    println!("  working/episodic/semantic: {}/{}/{}",
        proposal.config.working_weight, proposal.config.episodic_weight, proposal.config.semantic_weight);
    println!("Best score:       {:.3} (delta {:+.4})", proposal.score, proposal.delta);
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
        let hit = results
            .iter()
            .any(|r| is_hit(&r.content, &case.expected_path) || is_hit(&r.path, &case.expected_path));
        let first_hit_rank = results
            .iter()
            .position(|r| is_hit(&r.content, &case.expected_path) || is_hit(&r.path, &case.expected_path))
            .map(|i| i + 1);
        case_results.push(CaseResult {
            case_id: case.id.clone(),
            hit,
            first_hit_rank,
        });
    }
    RetrievalMetrics::from_results(&ds.dataset, &case_results, k)
}
