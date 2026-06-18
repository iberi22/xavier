//! Auto-Improvement Loop
//!
//! Closed-loop auto-improvement inside Xavier:
//! benchmark → gap analysis → generate experiment → validate → merge → re-measure
//!
//! Inspired by the autoresearch program at E:\scripts-python\autoresearch.
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

use crate::health::collect_health;
use crate::settings::XavierSettings;
use crate::agents::evolve::{EvolveModule, EvolveConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// KPI snapshot for a single benchmark run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkSnapshot {
    pub timestamp_secs: u64,
    pub recall_at_k: f64,
    pub precision: f64,
    pub avg_latency_ms: f64,
    pub memory_hit_rate: f64,
    pub mesh_peers_reachable: u32,
    pub health_status: String,
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

/// Main engine for auto-improvement
pub struct AutoImprovementEngine {
    settings: XavierSettings,
    evolve_module: Arc<EvolveModule>,
}

impl AutoImprovementEngine {
    pub fn new(settings: XavierSettings) -> Self {
        let evolve_config = EvolveConfig::new("auto-improve".to_string());
        Self {
            settings,
            evolve_module: Arc::new(EvolveModule::new(evolve_config)),
        }
    }

    /// Trigger an evolution cycle
    pub async fn trigger_evolution(&self) -> anyhow::Result<()> {
        self.evolve_module.run_evolution_cycle().await.map(|_| ())
    }

    /// Run full improvement cycle
    pub async fn run_cycle(&self, previous: Option<&BenchmarkSnapshot>) -> ImprovementCycle {
        run_improvement_cycle(&self.settings, previous).await
    }
}

/// Run a full cycle: benchmark → gaps → experiments → validate
pub async fn run_improvement_cycle(
    settings: &XavierSettings,
    previous: Option<&BenchmarkSnapshot>,
) -> ImprovementCycle {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let cycle_id = format!("cycle-{:x}", now);

    // Phase 1: Benchmark
    let health = collect_health(settings, None).await;
    let benchmark = BenchmarkSnapshot {
        timestamp_secs: now,
        recall_at_k: 0.0,   // TODO: integrate real recall metrics
        precision: 0.0,      // TODO: integrate real precision metrics
        avg_latency_ms: health.embedding.latency_ms,
        memory_hit_rate: 0.0,
        mesh_peers_reachable: health.mesh.connected_peers,
        health_status: health.status.clone(),
    };

    // Phase 2: Gap analysis
    let gaps = analyze_gaps(&benchmark, previous);

    // Phase 3: Generate experiments
    let experiments = generate_experiments(&gaps, now);

    // Phase 4: Validate (if any experiment)
    let accepted = if experiments.is_empty() {
        vec![]
    } else {
        validate_and_report(&experiments).await
    };

    let improvement = if let Some(prev) = previous {
        let delta = benchmark.recall_at_k - prev.recall_at_k;
        delta.max(0.0) * 100.0
    } else {
        0.0
    };

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

/// Analyze gaps between current benchmarks and targets
fn analyze_gaps(
    current: &BenchmarkSnapshot,
    previous: Option<&BenchmarkSnapshot>,
) -> Vec<Gap> {
    let mut gaps = Vec::new();

    // Recall target: 95%
    if current.recall_at_k < 0.95 {
        let gap = 0.95 - current.recall_at_k;
        gaps.push(Gap {
            metric: "recall@k".to_string(),
            current: current.recall_at_k,
            target: 0.95,
            gap_pct: gap * 100.0,
            severity: if gap > 0.2 { GapSeverity::Critical } else if gap > 0.1 { GapSeverity::Major } else { GapSeverity::Minor },
            suggested_experiments: vec![
                "Increase RRF k value".to_string(),
                "Adjust BM25 b parameter".to_string(),
                "Add query expansion".to_string(),
            ],
        });
    }

    // Precision target: 90%
    if current.precision < 0.90 {
        let gap = 0.90 - current.precision;
        gaps.push(Gap {
            metric: "precision".to_string(),
            current: current.precision,
            target: 0.90,
            gap_pct: gap * 100.0,
            severity: if gap > 0.15 { GapSeverity::Major } else { GapSeverity::Minor },
            suggested_experiments: vec![
                "Adjust relevance threshold".to_string(),
                "Enable entity extraction filter".to_string(),
            ],
        });
    }

    // Latency target: < 100ms
    if current.avg_latency_ms > 100.0 {
        let gap = current.avg_latency_ms - 100.0;
        gaps.push(Gap {
            metric: "avg_latency".to_string(),
            current: current.avg_latency_ms,
            target: 100.0,
            gap_pct: (gap / 100.0) * 100.0,
            severity: if gap > 200.0 { GapSeverity::Critical } else if gap > 50.0 { GapSeverity::Major } else { GapSeverity::Minor },
            suggested_experiments: vec![
                "Enable cache warming".to_string(),
                "Reduce embedding batch size".to_string(),
            ],
        });
    }

    // Regress/progress tracking
    if let Some(prev) = previous {
        if current.recall_at_k < prev.recall_at_k - 0.02 {
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
    for gap in gaps.iter().take(3) {
        // Only generate for first 3 gaps to avoid experiment explosion
        if let Some(exp_name) = gap.suggested_experiments.first() {
            experiments.push(Experiment {
                name: exp_name.clone(),
                description: format!("Auto-generated experiment to address gap: {}", gap.metric),
                config_overrides: HashMap::new(),
                acceptance_criteria: vec![format!(
                    "Improve {} by at least 50% of gap ({:.1}%)",
                    gap.metric, gap.gap_pct
                )],
                created_at_secs: now,
                status: ExperimentStatus::Pending,
            });
        }
    }
    experiments
}

/// Validate experiments (accept/reject based on improvement).
/// For now, always accepts pending experiments.
async fn validate_and_report(experiments: &[Experiment]) -> Vec<String> {
    let mut accepted = Vec::new();
    for exp in experiments.iter().filter(|e| matches!(e.status, ExperimentStatus::Pending)) {
        tracing::info!(experiment = %exp.name, "Auto-improvement experiment accepted");
        accepted.push(exp.name.clone());
    }
    accepted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_gaps_identifies_low_recall() {
        let current = BenchmarkSnapshot {
            timestamp_secs: 0,
            recall_at_k: 0.5,
            precision: 0.9,
            avg_latency_ms: 50.0,
            memory_hit_rate: 0.0,
            mesh_peers_reachable: 0,
            health_status: "healthy".to_string(),
        };
        let gaps = analyze_gaps(&current, None);
        assert!(gaps.iter().any(|g| g.metric == "recall@k"));
    }

    #[test]
    fn test_analyze_gaps_detects_regression() {
        let current = BenchmarkSnapshot {
            recall_at_k: 0.7, ..Default::default()
        };
        let previous = BenchmarkSnapshot {
            recall_at_k: 0.9, ..Default::default()
        };
        let gaps = analyze_gaps(&current, Some(&previous));
        assert!(gaps.iter().any(|g| g.metric == "recall_regression"));
    }

    #[test]
    fn test_generate_experiments_caps_at_3() {
        let gaps = vec![
            Gap { metric: "a".into(), current: 0.0, target: 1.0, gap_pct: 100.0, severity: GapSeverity::Critical, suggested_experiments: vec!["exp1".into()] },
            Gap { metric: "b".into(), current: 0.0, target: 1.0, gap_pct: 50.0, severity: GapSeverity::Major, suggested_experiments: vec!["exp2".into()] },
            Gap { metric: "c".into(), current: 0.0, target: 1.0, gap_pct: 10.0, severity: GapSeverity::Minor, suggested_experiments: vec!["exp3".into()] },
            Gap { metric: "d".into(), current: 0.0, target: 1.0, gap_pct: 5.0, severity: GapSeverity::Minor, suggested_experiments: vec!["exp4".into()] },
        ];
        let exps = generate_experiments(&gaps, 0);
        assert!(exps.len() <= 3);
    }
}

impl Default for BenchmarkSnapshot {
    fn default() -> Self {
        Self {
            timestamp_secs: 0,
            recall_at_k: 0.0,
            precision: 0.0,
            avg_latency_ms: 0.0,
            memory_hit_rate: 0.0,
            mesh_peers_reachable: 0,
            health_status: "unknown".to_string(),
        }
    }
}
