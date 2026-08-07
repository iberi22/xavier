//! Auto-Improvement Loop v2
//!
//! Closed-loop auto-improvement inside Xavier:
//! benchmark → gap analysis → generate experiment → validate → merge → re-measure

pub mod benchmark;
pub mod cycle;
pub mod experiments;
pub mod gaps;

// Re-export public structs, enums, functions, and the core engine for perfect backwards-compatibility
pub use benchmark::{BenchmarkSnapshot, ExternalBenchmarkMetrics};
pub use cycle::{AutoImprovementEngine, HistoryEntry, ImprovementCycle};
pub use experiments::{Experiment, ExperimentStatus, generate_experiments};
pub use gaps::{Gap, GapSeverity, analyze_gaps};
