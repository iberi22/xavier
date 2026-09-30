//! Belief evaluation engine for agent reasoning
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
//!
//! The evaluator is a pure scoring rule, so it now lives in
//! `crate::domain::cycle_breaks::w30_06` and `crate::memory` no longer has to
//! import `crate::agents` to score a belief (ADR-033 Wave 0). The re-export below
//! keeps `crate::agents::belief_evaluator::BeliefEvaluator` resolving for every
//! existing caller.
pub use crate::domain::cycle_breaks::w30_06::BeliefEvaluator;
