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

pub mod core;
pub mod types;
pub mod actions;
pub mod config;
pub mod priority;
pub mod quality;
pub mod management;
pub mod eviction;
pub mod decay;
pub mod consolidation;
pub mod compression;
pub mod tracking;

// Re-export primary types and struct
pub use core::MemoryManager;
pub use types::*;