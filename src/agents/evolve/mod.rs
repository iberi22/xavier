// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Evolve Module - Autonomous Self-Improving Memory System
//!
//! Based on Karpathy's autoresearch loop pattern:
//! - Fixed time budget per experiment
//! - Single metric optimization
//! - Keep/discard based on metric improvement
//! - Crash recovery
//! - Never stop until human interrupts
//!
//! The Evolve Module autonomously improves Xavier's memory architecture by:
//! 1. Researcher: scans for new memory techniques
//! 2. Experimenter: modifies memory code with hypotheses
//! 3. Evaluator: runs benchmarks (LoCoMo, Evo-Memory)
//! 4. Reflector: analyzes results, generates new hypotheses
//! 5. Integrator: keeps winning changes, discards losers

pub mod config;
pub mod evaluator;
pub mod experiment;
pub mod gap_analyzer;
pub mod integrator;
pub mod mutator;
pub mod reflector;
pub mod researcher;
pub mod results;

#[cfg(test)]
mod tests;

use anyhow::Result;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::agents::evolve::experiment::Hypothesis;
use crate::observability::analyzer::{ErrorDiagnosis, Urgency};
use crate::observability::service_log::LogLevel;

pub use config::EvolveConfig;
pub use results::ExperimentResult;

/// Evolve Module - Main coordinator for the autonomous improvement loop
pub struct EvolveModule {
    config: EvolveConfig,
    state: Arc<RwLock<EvolveState>>,
    #[allow(dead_code)]
    researcher: researcher::Researcher,
    evaluator: evaluator::Evaluator,
    integrator: integrator::Integrator,
    #[allow(dead_code)]
    gap_analyzer: gap_analyzer::GapAnalyzer,
    mutator: mutator::Mutator,
    reflector: reflector::Reflector,
}

#[derive(Debug, Clone)]
pub struct EvolveState {
    pub current_tag: String,
    pub experiments_run: u64,
    pub experiments_kept: u64,
    pub experiments_discarded: u64,
    pub experiments_crashed: u64,
    pub last_metric: Option<f32>,
    pub best_metric: Option<f32>,
    pub running: bool,
}

impl Default for EvolveState {
    fn default() -> Self {
        Self {
            current_tag: current_date_tag(),
            experiments_run: 0,
            experiments_kept: 0,
            experiments_discarded: 0,
            experiments_crashed: 0,
            last_metric: None,
            best_metric: None,
            running: false,
        }
    }
}

/// Result of an evolution cycle
#[derive(Debug, Clone)]
pub struct EvolutionResult {
    pub experiment: ExperimentResult,
    pub improved: bool,
}

impl EvolveModule {
    /// Create a new Evolve Module
    pub async fn new(config: EvolveConfig) -> Result<Self> {
        Ok(Self {
            state: Arc::new(RwLock::new(EvolveState::default())),
            researcher: researcher::Researcher::new(),
            evaluator: evaluator::Evaluator::new(config.benchmark),
            integrator: integrator::Integrator::new(),
            gap_analyzer: gap_analyzer::GapAnalyzer::new().await?,
            mutator: mutator::Mutator::new(),
            reflector: reflector::Reflector::new(),
            config,
        })
    }

    /// Start the autonomous evolution loop
    pub async fn run(&self) -> Result<()> {
        {
            let mut state = self.state.write().await;
            if state.running {
                warn!("Evolve Module already running");
                return Ok(());
            }
            state.running = true;
            info!("🚀 Starting Evolve Module - autonomous loop");
            info!("Tag: {}", state.current_tag);
            info!(
                "Time budget per experiment: {}s",
                self.config.time_budget_secs
            );
            info!("Metric: {}", self.config.metric);
        }

        loop {
            let should_stop = {
                let state = self.state.read().await;
                !state.running
            };
            if should_stop {
                break;
            }

            match self.run_evolution_cycle().await {
                Ok(evolution_result) => {
                    let result = evolution_result.experiment;
                    let mut state = self.state.write().await;
                    state.experiments_run += 1;
                    state.last_metric = Some(result.metric_value);

                    if evolution_result.improved {
                        state.best_metric = Some(result.metric_value);
                        state.experiments_kept += 1;
                        info!(
                            experiments_run = state.experiments_run,
                            experiments_kept = state.experiments_kept,
                            metric = result.metric_value,
                            "✅ Kept improvement"
                        );
                    } else {
                        state.experiments_discarded += 1;
                        info!(
                            experiments_run = state.experiments_run,
                            experiments_discarded = state.experiments_discarded,
                            "❌ Discarded - no improvement"
                        );
                    }
                }
                Err(e) => {
                    let mut state = self.state.write().await;
                    state.experiments_crashed += 1;
                    warn!(
                        experiments_crashed = state.experiments_crashed,
                        error = %e,
                        "💥 Experiment crashed"
                    );
                }
            }

            // Log results to TSV
            self.log_results().await?;
        }

        info!("🛑 Evolve Module stopped");
        Ok(())
    }

    /// Run a single evolution cycle: verify → mutate → evaluate → accept/reject
    pub async fn run_evolution_cycle(&self) -> Result<EvolutionResult> {
        // 1. Verify (Baseline Evaluation)
        info!("Step 1: Verifying baseline...");
        let pre_metric = self.evaluator.evaluate().await?;

        // 2. Mutate
        info!("Step 2: Generating mutation...");
        // In a real scenario, we would use reflector insights.
        // For now, we'll use empty insights to get a default mutation.
        let insights = self.reflector.analyze(&[]).await?;
        let mutations = self.mutator.generate_mutations(&insights)?;
        let mutation = mutations
            .first()
            .ok_or_else(|| anyhow::anyhow!("No mutations generated"))?;
        let hypothesis = self.mutator.mutation_to_hypothesis(mutation);

        // 3. Apply and Evaluate
        info!(hypothesis = %hypothesis.description, "Step 3: Applying and evaluating hypothesis...");
        let backup = self.integrator.backup_memory_modules().await?;
        let modified = self.integrator.apply_hypothesis(&hypothesis).await?;

        if !modified {
            self.integrator.restore(backup).await?;
            return Ok(EvolutionResult {
                experiment: ExperimentResult::baseline(),
                improved: false,
            });
        }

        let post_metric_result = tokio::time::timeout(
            std::time::Duration::from_secs(self.config.time_budget_secs),
            self.evaluator.evaluate(),
        )
        .await;

        // 4. Accept or Reject
        match post_metric_result {
            Ok(Ok(post_metric)) => {
                let is_lower_better = self.config.metric == config::MetricType::ValBpb;
                let improved = self
                    .evaluator
                    .compare(pre_metric, post_metric, is_lower_better);
                let regression = self
                    .evaluator
                    .is_regression(post_metric, is_lower_better)
                    .await;

                if improved && !regression {
                    info!(
                        pre = pre_metric,
                        post = post_metric,
                        "✅ Improvement detected and accepted"
                    );
                    self.integrator.commit(&hypothesis).await?;
                    Ok(EvolutionResult {
                        experiment: ExperimentResult {
                            hypothesis_id: hypothesis.id,
                            metric_value: post_metric,
                            status: experiment::ExperimentStatus::Kept,
                            commit_hash: None,
                            crashed: false,
                        },
                        improved: true,
                    })
                } else {
                    info!(
                        pre = pre_metric,
                        post = post_metric,
                        regression = regression,
                        "❌ No improvement or regression detected. Rejecting."
                    );
                    self.integrator.restore(backup).await?;
                    self.integrator.reset_to_baseline().await?;
                    Ok(EvolutionResult {
                        experiment: ExperimentResult {
                            hypothesis_id: hypothesis.id,
                            metric_value: post_metric,
                            status: experiment::ExperimentStatus::Discarded,
                            commit_hash: None,
                            crashed: false,
                        },
                        improved: false,
                    })
                }
            }
            Ok(Err(e)) => {
                self.integrator.restore(backup).await?;
                Err(e)
            }
            Err(_) => {
                self.integrator.restore(backup).await?;
                self.integrator.reset_to_baseline().await?;
                Err(anyhow::anyhow!("Experiment timed out"))
            }
        }
    }

    /// Stop the evolution loop
    pub async fn stop(&self) {
        let mut state = self.state.write().await;
        state.running = false;
        info!(
            "Stopping Evolve Module after {} experiments",
            state.experiments_run
        );
    }

    /// Run a single experiment (deprecated in favor of run_evolution_cycle)
    #[allow(dead_code)]
    async fn run_single_experiment(&self) -> Result<ExperimentResult> {
        // 0. Analyze gaps to guide research
        let res = self.run_evolution_cycle().await?;
        Ok(res.experiment)
    }

    /// Check if metric is an improvement (lower is better for val_bpb)
    #[allow(dead_code)]
    async fn is_improvement(&self, metric: f32) -> bool {
        let state = self.state.read().await;
        match state.best_metric {
            Some(best) => {
                let is_lower_better = self.config.metric == config::MetricType::ValBpb;
                if is_lower_better {
                    metric < best
                } else {
                    metric > best
                }
            }
            None => true, // First experiment is always improvement
        }
    }

    /// Check if improvement is significant enough for a PR (e.g. > 2%)
    #[allow(dead_code)]
    async fn is_significant_improvement(&self, metric: f32) -> bool {
        let state = self.state.read().await;
        match state.best_metric {
            Some(best) if best > 0.0 => {
                let diff = (best - metric) / best;
                diff > 0.02
            }
            _ => true,
        }
    }

    #[allow(dead_code)]
    async fn create_improvement_pr(&self, hypothesis: &Hypothesis, _metric: f32) {
        info!("Significant improvement detected, creating PR...");

        let fixer = crate::observability::Fixer::new();
        let diagnosis = ErrorDiagnosis {
            pattern: crate::observability::service_log::ErrorPattern {
                module: "evolve".to_string(),
                level: LogLevel::Info,
                frequency: 1,
                sample_message: format!("Autonomous improvement: {}", hypothesis.description),
                first_seen: chrono::Utc::now().to_rfc3339(),
                last_seen: chrono::Utc::now().to_rfc3339(),
            },
            analyzed_at: chrono::Utc::now().to_rfc3339(),
            root_cause: format!("Identified optimization: {}", hypothesis.description),
            source_location: Some(hypothesis.files.join(", ")),
            suggested_fix: hypothesis.patch.clone(),
            confidence: 0.95,
            urgency: Urgency::High,
        };

        let result = fixer.process_diagnosis(&diagnosis).await;
        // TODO: wire TelegramNotified after successful fix.
        //       e.g. if result.success { crate::observability::notifier::maybe_notify_telegram_fix(&result.action); }
        if result.success {
            info!("Improvement PR created: {:?}", result.url);
        } else {
            warn!("Failed to create improvement PR: {}", result.message);
        }
    }

    /// Log results to TSV
    async fn log_results(&self) -> Result<()> {
        let state = self.state.read().await;
        let results_path = self.config.results_path();

        let line = format!(
            "{}\t{:.6}\t{:.1}\t{}\texp_{}\n",
            current_commit_hash(),
            state.last_metric.unwrap_or(0.0),
            state.experiments_kept as f32 * 44.0, // memory_gb estimate
            match state.last_metric {
                Some(m) if m <= state.best_metric.unwrap_or(f32::MAX) => "keep",
                _ => "discard",
            },
            state.experiments_run
        );

        if let Some(parent) = results_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&results_path)
            .await?
            .write_all(line.as_bytes())
            .await?;

        Ok(())
    }

    /// Get current state
    pub async fn state(&self) -> EvolveState {
        self.state.read().await.clone()
    }
}

/// Get current date-based tag (e.g., "mar24")
fn current_date_tag() -> String {
    let now = chrono::Local::now();
    format!(
        "{}{}",
        now.format("%b").to_string().to_lowercase(),
        now.format("%d")
    )
}

/// Get current git commit hash (short)
fn current_commit_hash() -> String {
    // This would be replaced with actual git command in production
    "local".to_string()
}
