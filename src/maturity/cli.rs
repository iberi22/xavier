//! CLI handler for `xavier maturity` commands.

use clap::Subcommand;
use std::path::PathBuf;

use crate::maturity::{MaturityResult, MaturityScanner, ProgressCallback};
use anyhow::Result;
use chrono::Utc;

/// Maturity scanning & reporting subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum MaturityCommand {
    /// Run a full maturity scan and update feature-maturity.json
    Scan {
        /// Path to the codebase (defaults to self)
        #[arg(short, long)]
        codebase: Option<PathBuf>,
        /// Output report to stdout (JSON)
        #[arg(short, long)]
        json: bool,
        /// Output report as markdown
        #[arg(short, long)]
        markdown: bool,
        /// Path to anchors file (default: .xavier/maturity-anchors.json)
        #[arg(long)]
        anchors: Option<PathBuf>,
        /// Write result to feature-maturity.json
        #[arg(short, long)]
        write: bool,
    },
    /// Run deep scan (v2) — includes memory, session, and conversation evidence
    DeepScan {
        /// Path to the codebase (defaults to self)
        #[arg(short, long)]
        codebase: Option<PathBuf>,
        /// Output report to stdout (JSON)
        #[arg(short, long)]
        json: bool,
        /// Output report as markdown
        #[arg(short, long)]
        markdown: bool,
        /// Path to anchors file (default: .xavier/maturity-anchors.json)
        #[arg(long)]
        anchors: Option<PathBuf>,
        /// Write result to feature-maturity.json
        #[arg(short, long)]
        write: bool,
    },
    /// Show current maturity status from cached file
    Status {
        /// Show detailed per-subcomponent breakdown
        #[arg(short, long)]
        detailed: bool,
    },
    /// Show the breakdown formula for a specific feature
    Explain {
        /// Feature ID to explain, e.g. "mesh-network"
        feature: String,
        #[arg(short, long)]
        detailed: bool,
    },
}

/// Entry point called by the CLI dispatcher.
pub async fn handle_maturity_command(cmd: MaturityCommand) -> Result<()> {
    match cmd {
        MaturityCommand::Scan { ref codebase, json, markdown, ref anchors, write } |
        MaturityCommand::DeepScan { ref codebase, json, markdown, ref anchors, write } => {
            let root = codebase.as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| ".".to_string());
            let anchor_path = anchors.clone().unwrap_or_else(|| PathBuf::from(".xavier/maturity-anchors.json"));

            // DeepScan uses with_deep_scan()
            let is_deep = matches!(&cmd, MaturityCommand::DeepScan { .. });
            let is_write = write;
            let result = if is_deep {
                // For --write, pass a progress callback that flushes partial results after each layer
                if is_write {
                    let report_path = PathBuf::from(".xavier/feature-maturity.json");
                    run_deep_maturity_scan_with_callback(
                        &root,
                        &anchor_path,
                        Box::new(move |json_str: &str| {
                            // Write partial JSON so even if mid-scan crash, we have progress
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
                                if val.get("event").and_then(|v| v.as_str()) == Some("layer_complete") {
                                    if let Ok(partial_json) = serde_json::to_string_pretty(&serde_json::json!({
                                        "progress_event": json_str,
                                        "note": "Partial scan — run 'maturity deep-scan' again to complete",
                                        "timestamp": Utc::now().to_rfc3339(),
                                    })) {
                                        let _ = std::fs::write(&report_path, &partial_json);
                                        eprintln!("  ✓ Layer '{}' complete ({}ms)",
                                            val.get("layer").and_then(|v| v.as_str()).unwrap_or("?"),
                                            val.get("timing_ms").and_then(|v| v.as_u64()).unwrap_or(0));
                                    }
                                }
                            }
                        }),
                    )?
                } else {
                    run_deep_maturity_scan(&root, &anchor_path)?
                }
            } else {
                run_maturity_scan(&root, &anchor_path)?
            };

            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else if markdown {
                println!("## Maturity Scan Results");
                println!();
                println!("| Feature | Score | Status |");
                println!("|---|---|---|");
                for feat in &result.features {
                    println!("| {} | {}% | {} |", feat.id, feat.overall, feat.status);
                }
                println!();
                println!("**Overall:** {}%", result.summary.overall_maturity);
                println!();
                println!("**Scanner:** {}", result.scanner_version);
                if is_deep {
                    let t = &result.layers;
                    println!("**Layer timing:** static={}ms dynamic={}ms memory={}ms conversations={}ms total={}ms",
                        t.static_ms, t.dynamic_ms, t.memory_ms, t.conversations_ms, t.total_ms);
                }
            } else {
                print_status(&result, false);
            }

            if write {
                let report_path = PathBuf::from(".xavier/feature-maturity.json");
                let report_json = serde_json::to_string_pretty(&result)?;
                std::fs::write(&report_path, &report_json)?;
                println!("\nWritten to {}", report_path.display());
            }

            Ok(())
        }
        MaturityCommand::Status { detailed } => {
            let path = PathBuf::from(".xavier/feature-maturity.json");
            if !path.exists() {
                anyhow::bail!("No maturity report found. Run `xavier maturity scan` or `xavier maturity deep-scan` first.");
            }
            let content = std::fs::read_to_string(&path)?;
            let result: MaturityResult = serde_json::from_str(&content)?;
            print_status(&result, detailed);
            Ok(())
        }
        MaturityCommand::Explain { feature, detailed } => {
            let path = PathBuf::from(".xavier/feature-maturity.json");
            let content = std::fs::read_to_string(&path)?;
            let result: MaturityResult = serde_json::from_str(&content)?;
            print_explanation(&result, &feature, detailed);
            Ok(())
        }
    }
}

/// Run the maturity scan (v1) and return the result.
pub fn run_maturity_scan(codebase_root: &str, anchors_path: &PathBuf) -> Result<MaturityResult> {
    let scanner = MaturityScanner::new(anchors_path, codebase_root)?;
    Ok(scanner.scan())
}

/// Run the deep maturity scan (v2) and return the result.
pub fn run_deep_maturity_scan(codebase_root: &str, anchors_path: &PathBuf) -> Result<MaturityResult> {
    let scanner = MaturityScanner::new(anchors_path, codebase_root)?.with_deep_scan();
    Ok(scanner.scan())
}

/// Run the deep maturity scan (v2) with a progress callback after each layer.
pub fn run_deep_maturity_scan_with_callback(
    codebase_root: &str,
    anchors_path: &PathBuf,
    callback: ProgressCallback,
) -> Result<MaturityResult> {
    let scanner = MaturityScanner::new(anchors_path, codebase_root)?
        .with_deep_scan()
        .with_progress(callback);
    Ok(scanner.scan())
}

/// Print a summary table of the current maturity.
pub fn print_status(result: &MaturityResult, detailed: bool) {
    println!();
    println!("Xavier Maturity Report");
    println!("{}", "=".repeat(60));
    println!(
        "Overall: {}% | Production Ready: {}/{} | Needs Work: {} | In Progress: {} | Errors: {}",
        result.summary.overall_maturity,
        result.summary.production_ready,
        result.summary.total_features,
        result.summary.needs_work,
        result.summary.in_progress,
        result.summary.scan_errors,
    );
    println!("Scanner: {} | Scanned at: {}", result.scanner_version, result.scanned_at);
    println!("HEAD: {}", result.head_commit);
    if result.layers.total_ms > 0 {
        let t = &result.layers;
        println!("Timing: static={}ms dynamic={}ms memory={}ms conversations={}ms total={}ms",
            t.static_ms, t.dynamic_ms, t.memory_ms, t.conversations_ms, t.total_ms);
    }
    println!();

    for feat in &result.features {
        let icon = match feat.status.as_str() {
            "production_ready" => "\u{2705}",
            "needs_work" => "\u{26A0}\u{FE0F}",
            _ => "\u{1F527}",
        };
        println!("  {} {} -- {}% ({})", icon, feat.id, feat.overall, feat.status);

        if detailed {
            for sub in &feat.subcomponents {
                println!(
                    "    * {}: {}% (tests: {}/{}, symbols: {}/{})",
                    sub.name, sub.maturity, sub.tests_passing, sub.tests_total,
                    sub.symbols_found, sub.symbols_total
                );
                if sub.memory_usage > 0 || sub.issue_health > 0 {
                    println!(
                        "      memory: {}%, issues: {}% | {}",
                        sub.memory_usage, sub.issue_health, sub.evidence_detail
                    );
                }
            }
        }
    }
}

/// Print a detailed explanation for a single feature.
pub fn print_explanation(result: &MaturityResult, feature_id: &str, detailed: bool) {
    let feature = result.features.iter().find(|f| f.id == feature_id);
    match feature {
        None => println!("Feature '{}' not found in scan results.", feature_id),
        Some(f) => {
            println!();
            println!("Feature: {} ({})", f.id, f.name);
            println!("{}", "=".repeat(60));
            println!("Overall: {}% | Status: {}", f.overall, f.status);
            println!();

            for sub in &f.subcomponents {
                println!("  Subcomponent: {}", sub.name);
                println!("     Weight: {}%", sub.weight);
                println!("     Maturity: {}%", sub.maturity);
                if detailed {
                    println!(
                        "     Static check: {}/{} symbols found ({}%)",
                        sub.symbols_found, sub.symbols_total, sub.static_pass_rate
                    );
                    println!(
                        "     Tests: {}/{} passing ({}%)",
                        sub.tests_passing, sub.tests_total, sub.test_pass_rate
                    );
                    if sub.memory_usage > 0 || sub.issue_health > 0 {
                        println!(
                            "     Memory evidence: {}% | Issue health: {}%",
                            sub.memory_usage, sub.issue_health
                        );
                        println!("     Detail: {}", sub.evidence_detail);
                    }
                }
                println!();
            }

            if result.scanner_version == "v2" {
                println!("Scoring Formula (v2 — 5 metrics):");
                println!("   sub_score = static_pass_rate x weight x 0.35");
                println!("             + test_pass_rate x weight x 0.35");
                println!("             + gate_ok x weight x 0.10");
                println!("             + memory_evidence x weight x 0.10");
                println!("             + conversation_evidence x weight x 0.10");
            } else {
                println!("Scoring Formula (v1 — 3 metrics):");
                println!("   sub_score = static_pass_rate x weight x 0.40");
                println!("             + test_pass_rate x weight x 0.50");
                println!("             + gate_ok x weight x 0.10");
            }
            println!();
            println!("   feature_score = sum(sub_scores) / sum(weights) x 100");
        }
    }
}
