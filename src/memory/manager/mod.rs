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
pub mod compression;
pub mod config;
pub mod consolidation;
pub mod core;
pub mod decay;
pub mod eviction;
pub mod gc;
pub mod management;
pub mod priority;
pub mod quality;
pub mod tracking;
pub mod types;

// Re-export primary types and struct
pub use core::MemoryManager;
pub use types::*;

#[cfg(test)]
mod tests;
