//! CLI handler for the auto-improvement loop.
//!
//! Implements `xavier improve run` and `xavier improve status`. The handler builds
//! an `AutoImprovementEngine` against the locally-loaded QmdMemory (same loader the
//! offline `stats` path uses) and runs a full cycle: benchmark → gaps → experiments
//! → (optionally) validate.

use anyhow::Result;
use xavier::auto_improvement::AutoImprovementEngine;
use xavier::settings::XavierSettings;

use crate::cli::commands::enums::ImproveCommand;
use crate::cli::commands::spawn::load_spawn_memory;

/// Dispatch the `improve` subcommands.
pub async fn handle_improve_command(ci: bool, cmd: Option<ImproveCommand>) -> Result<()> {
    match cmd {
        Some(ImproveCommand::Run { autonomous, json, ci: run_ci }) => {
            run_cycle(autonomous || ci || run_ci, json, ci || run_ci).await
        }
        Some(ImproveCommand::Status) => {
            show_status().await
        }
        None => {
            // Default to running a cycle in CI mode if ci flag was specified, otherwise run standard cycle.
            let autonomous = ci;
            run_cycle(autonomous, false, ci).await
        }
    }
}

/// Run a full auto-improvement cycle against the local memory store.
async fn run_cycle(autonomous: bool, json: bool, ci: bool) -> Result<()> {
    let settings = XavierSettings::default();
    let memory = load_spawn_memory()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to load local memory for benchmarking: {e}"))?;

    let engine = AutoImprovementEngine::new()
        .with_memory(memory)
        .with_autonomous(autonomous);

    if !json {
        println!(
            "Running auto-improvement cycle (autonomous={}, ci={})...",
            autonomous, ci
        );
    }

    let cycle = engine.run_cycle(&settings, None).await;

    if json {
        println!("{}", serde_json::to_string_pretty(&cycle)?);
        if ci {
            let has_regression_or_critical = cycle.gaps.iter().any(|gap| {
                gap.metric == "recall_regression" || matches!(gap.severity, xavier::auto_improvement::GapSeverity::Critical)
            });
            if has_regression_or_critical {
                return Err(anyhow::anyhow!(
                    "CI failed: critical gaps or regressions detected under auto-improvement cycle!"
                ));
            }
        }
        return Ok(());
    }

    // Human-readable report.
    println!("\n=== Auto-Improvement Cycle {} ===", cycle.cycle_id);
    println!("\nBenchmark:");
    println!("  recall@k:        {:.3}", cycle.benchmark.recall_at_k);
    println!("  precision:       {:.3}", cycle.benchmark.precision);
    println!(
        "  avg latency:     {:.1} ms",
        cycle.benchmark.avg_latency_ms
    );
    println!(
        "  p99 latency:     {:.1} ms",
        cycle.benchmark.p99_latency_ms
    );
    println!("  cache hit rate:  {:.1}%", cycle.benchmark.cache_hit_rate);
    println!("  documents:       {}", cycle.benchmark.total_documents);
    println!("  health:          {}", cycle.benchmark.health_status);

    if cycle.gaps.is_empty() {
        println!("\n✅ No gaps detected — all metrics within target.");
    } else {
        println!("\nGaps detected ({}):", cycle.gaps.len());
        for gap in &cycle.gaps {
            println!(
                "  • {:<18} {:.2} → {:.2}  ({:?}, gap {:.1}%)",
                gap.metric, gap.current, gap.target, gap.severity, gap.gap_pct
            );
        }
    }

    if !cycle.experiments.is_empty() {
        println!("\nExperiments generated ({}):", cycle.experiments.len());
        for exp in &cycle.experiments {
            let delta = exp
                .result_metric_delta
                .map(|d| format!("{:+.4}", d))
                .unwrap_or_else(|| "—".to_string());
            println!(
                "  • {:<32} {:?}  overrides={}  delta={}",
                exp.name,
                exp.status,
                exp.config_overrides.len(),
                delta
            );
        }
    }

    if autonomous {
        if cycle.accepted_changes.is_empty() {
            println!("\n⚠️  No experiments accepted (none improved the baseline).");
        } else {
            println!("\n✅ Accepted changes:");
            for name in &cycle.accepted_changes {
                println!("   + {}", name);
            }
        }
    } else {
        println!("\n💡 Re-run with --autonomous to validate and apply beneficial experiments.");
    }

    println!("\nImprovement vs previous: {:.2}%", cycle.improvement_pct);

    if ci {
        let has_regression_or_critical = cycle.gaps.iter().any(|gap| {
            gap.metric == "recall_regression" || matches!(gap.severity, xavier::auto_improvement::GapSeverity::Critical)
        });

        if has_regression_or_critical {
            return Err(anyhow::anyhow!(
                "CI failed: critical gaps or regressions detected under auto-improvement cycle!"
            ));
        }
        println!("\n✅ CI check passed: no critical gaps or regressions detected.");
    }

    Ok(())
}

/// Show the last cycle's benchmark history (best-effort: re-runs a benchmark since
/// the engine history is in-memory and not persisted across CLI invocations).
async fn show_status() -> Result<()> {
    let settings = XavierSettings::default();
    let memory = load_spawn_memory()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to load local memory: {e}"))?;

    let engine = AutoImprovementEngine::new().with_memory(memory);
    let snapshot = engine.run_benchmark(&settings, None).await;

    println!("=== Current Benchmark (fresh measurement) ===");
    println!("  recall@k:        {:.3}", snapshot.recall_at_k);
    println!("  precision:       {:.3}", snapshot.precision);
    println!("  avg latency:     {:.1} ms", snapshot.avg_latency_ms);
    println!("  p99 latency:     {:.1} ms", snapshot.p99_latency_ms);
    println!("  cache hit rate:  {:.1}%", snapshot.cache_hit_rate);
    println!("  documents:       {}", snapshot.total_documents);
    println!("  mesh peers:      {}", snapshot.mesh_peers_reachable);
    println!("  health:          {}", snapshot.health_status);
    println!("  db integrity:    {}", snapshot.db_integrity_ok);

    // Note: persistent history across CLI runs requires a storage hook (planned).
    println!("\nℹ️  Cross-cycle history is tracked within a long-running engine instance");
    println!("    (server/scheduler). The CLI measures a fresh snapshot each invocation.");

    Ok(())
}
