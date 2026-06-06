//! Memory Manager - Intelligent Memory Management System
//!
//! Provides autonomous memory lifecycle management:
//! - Memory Prioritization (Critical → Ephemeral)
//! - Memory Decay based on access time
//! - Memory Quality Scoring
//! - Memory Consolidation (deduplication)
//! - Intelligent Forgetting/Eviction
//!
//! Split into focused sub-modules.

pub mod actions;
pub mod config;
pub mod priority;
pub mod quality;

pub use actions::*;
pub use config::*;
pub use priority::*;
pub use quality::*;