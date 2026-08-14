//! Auto-Improvement Loop v2
//!
//! Closed-loop auto-improvement inside Xavier:
//! benchmark → gap analysis → generate experiment → validate → merge → re-measure

pub mod benchmark;
pub mod cycle;
pub mod experiments;
pub mod gaps;

// Re-export public structs, enums, functions, and the core engine for perfect backwards-compatibility
pub use benchmark::{benchmark_entity_resolution, BenchmarkSnapshot, ExternalBenchmarkMetrics};
pub use cycle::{AutoImprovementEngine, HistoryEntry, ImprovementCycle};
pub use experiments::{generate_experiments, Experiment, ExperimentStatus};
pub use gaps::{analyze_gaps, Gap, GapSeverity};

/// Dispatch benchmark slice by name, returning score on 0..=100 scale if recognized.
pub fn run_slice(name: &str) -> Option<f64> {
    match name {
        "entity-resolution" => Some(benchmark_entity_resolution()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_slice_entity_resolution_returns_valid_score() {
        // Exercise the entity-resolution benchmark slice via run_slice dispatch
        let score_opt = run_slice("entity-resolution");
        assert!(
            score_opt.is_some(),
            "run_slice('entity-resolution') should return Some(score)"
        );

        let score = score_opt.unwrap();
        assert!(
            (0.0..=100.0).contains(&score),
            "entity resolution score {score} must be within range 0.0..=100.0"
        );

        // Verify direct function output equals run_slice dispatch output
        let direct_score = benchmark_entity_resolution();
        assert_eq!(
            score, direct_score,
            "run_slice dispatch score must match direct function call"
        );

        // Verify invalid benchmark slice returns None
        let invalid = run_slice("non-existent-slice");
        assert!(
            invalid.is_none(),
            "run_slice for unknown benchmark slice should return None"
        );
    }
}
