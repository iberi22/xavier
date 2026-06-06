//! Core MemoryManager struct and constructors
//!
//! The [`MemoryManager`] provides autonomous memory lifecycle management:
//! memory prioritization, decay, quality scoring, consolidation, and eviction.

use std::collections::HashMap;
use std::sync::Arc;

use crate::memory::qmd_memory::QmdMemory;

use super::types::MemoryManagerConfig;

/// Intelligent Memory Manager - manages memory lifecycle autonomously
pub struct MemoryManager {
    pub(crate) memory: Arc<QmdMemory>,
    pub(crate) _belief_graph: Option<crate::memory::belief_graph::SharedBeliefGraph>,
    pub(crate) config: MemoryManagerConfig,
    /// Track access counts per document
    pub(crate) access_counts: std::sync::Mutex<HashMap<String, usize>>,
    /// Track last access times
    pub(crate) last_access_times: std::sync::Mutex<HashMap<String, chrono::DateTime<chrono::Utc>>>,
    /// Track created times
    pub(crate) created_times: std::sync::Mutex<HashMap<String, chrono::DateTime<chrono::Utc>>>,
    /// Relevance scores (can be decayed over time)
    pub(crate) relevance_scores: std::sync::Mutex<HashMap<String, f32>>,
}

impl MemoryManager {
    pub fn new(
        memory: Arc<QmdMemory>,
        belief_graph: Option<crate::memory::belief_graph::SharedBeliefGraph>,
    ) -> Self {
        Self {
            memory,
            _belief_graph: belief_graph,
            config: MemoryManagerConfig::default(),
            access_counts: std::sync::Mutex::new(HashMap::new()),
            last_access_times: std::sync::Mutex::new(HashMap::new()),
            created_times: std::sync::Mutex::new(HashMap::new()),
            relevance_scores: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub fn with_config(
        memory: Arc<QmdMemory>,
        belief_graph: Option<crate::memory::belief_graph::SharedBeliefGraph>,
        config: MemoryManagerConfig,
    ) -> Self {
        Self {
            memory,
            _belief_graph: belief_graph,
            config,
            access_counts: std::sync::Mutex::new(HashMap::new()),
            last_access_times: std::sync::Mutex::new(HashMap::new()),
            created_times: std::sync::Mutex::new(HashMap::new()),
            relevance_scores: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Get configuration
    pub fn config(&self) -> &MemoryManagerConfig {
        &self.config
    }

    /// Expose the shared memory store so consolidation/reflection can mutate it.
    pub fn memory(&self) -> Arc<QmdMemory> {
        Arc::clone(&self.memory)
    }

    /// Update configuration
    pub fn set_config(&mut self, config: MemoryManagerConfig) {
        self.config = config;
    }
}
