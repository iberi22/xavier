//! Unified token estimation utility.
//!
//! The heuristic itself is owned by the domain layer (`domain::cycle_breaks::w30_08`, ADR-033
//! Wave 0) so that `memory` can estimate token counts without importing `context`. This module
//! re-exports it, keeping every existing `context::estimate_tokens` caller compiling unchanged.

pub use crate::domain::cycle_breaks::w30_08::estimate_tokens;
