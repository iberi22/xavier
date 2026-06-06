//! Memory manager — autonomous memory lifecycle management
//!
//! Provides intelligent memory management:
//! - Memory prioritization (Critical → Ephemeral)
//! - Time-based memory decay
//! - Memory quality scoring
//! - Memory consolidation (deduplication)
//! - Intelligent forgetting/eviction
//!
//! Split into focused sub-modules:
//!
//! | Module | Responsibility |
//! |--------|----------------|
//! | [`core`] | MemoryManager struct, constructors, and config access |
//! | [`types`] | MemoryPriority, MemoryQuality, MemoryStats, actions, config |
//! | [`tracking`] | Recording accesses, tracking new documents |
//! | [`decay`] | Time-based relevance decay |
//! | [`management`] | Queries, stats, promotion/demotion, legacy actions |
//! | [`consolidation`] | Deduplication via content signatures |
//! | [`eviction`] | Low-quality and priority-based eviction, auto-management |
//! | [`compression`] | Truncating oversized documents |
//! | [`tests`] | Unit tests for types and calculations |

pub mod core;
pub mod types;
mod tracking;
mod decay;
mod management;
mod consolidation;
mod eviction;
mod compression;
#[cfg(test)]
mod tests;

// Re-export primary types and struct
pub use core::MemoryManager;
pub use types::*;

/// Backwards compatibility alias.
pub use MemoryManager as XavierMemoryManager;
