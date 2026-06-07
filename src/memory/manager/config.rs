//! Memory manager configuration settings.
//!
//! Defines the configuration options for the MemoryManager,
//! including tier thresholds, consolidation intervals, and
//! eviction policies for each memory layer.

/// Configuration for memory manager
#[derive(Debug, Clone)]
pub struct MemoryManagerConfig {
    /// Maximum documents before eviction triggers
    pub max_documents: usize,
    /// Maximum storage bytes before eviction triggers
    pub max_storage_bytes: u64,
    /// Quality threshold below which documents are evicted
    pub quality_threshold: f32,
    /// Enable automatic decay
    pub auto_decay_enabled: bool,
    /// Enable automatic consolidation
    pub auto_consolidate_enabled: bool,
    /// Enable automatic eviction
    pub auto_evict_enabled: bool,
    /// Decay factor for all memories (can override per-priority)
    pub global_decay_factor: f32,
    /// Run auto-management every N hours
    pub auto_manage_interval_hours: u32,
    /// Compress memories larger than this size
    pub compression_threshold_bytes: usize,
}

impl Default for MemoryManagerConfig {
    fn default() -> Self {
        Self {
            max_documents: 10000,
            max_storage_bytes: 500 * 1024 * 1024, // 500MB
            quality_threshold: 0.25,
            auto_decay_enabled: true,
            auto_consolidate_enabled: true,
            auto_evict_enabled: true,
            global_decay_factor: 0.97,
            auto_manage_interval_hours: 24,
            compression_threshold_bytes: 2 * 1024, // 2KB
        }
    }
}
