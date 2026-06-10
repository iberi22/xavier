use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{error, info};

use crate::memory::manager::core::MemoryManager;

/// Background Daemon to run scheduled memory maintenance tasks autonomously.
pub struct MemoryDaemon {
    manager: Arc<MemoryManager>,
}

impl MemoryDaemon {
    pub fn new(manager: Arc<MemoryManager>) -> Self {
        Self { manager }
    }

    /// Spawns the autonomous Tokio loop.
    pub fn spawn(self) {
        // Run Memory Decay every 6 hours
        let manager_decay = self.manager.clone();
        tokio::spawn(async move {
            info!("MemoryDaemon: Scheduled decay loop started (6h interval)");
            loop {
                sleep(Duration::from_secs(6 * 3600)).await;
                info!("MemoryDaemon: Running scheduled decay_memories()");
                if let Err(e) = manager_decay.decay_memories().await {
                    error!("MemoryDaemon: Scheduled decay failed: {}", e);
                }
            }
        });

        // Run Semantic Compaction every 12 hours
        let manager_compact = self.manager.clone();
        tokio::spawn(async move {
            info!("MemoryDaemon: Scheduled semantic compaction loop started (12h interval)");
            loop {
                sleep(Duration::from_secs(12 * 3600)).await;
                info!("MemoryDaemon: Running scheduled compact_semantically()");
                if let Err(e) = manager_compact.compact_semantically().await {
                    error!("MemoryDaemon: Scheduled compaction failed: {}", e);
                }
            }
        });

        // Run Garbage Collection every 24 hours
        let manager_gc = self.manager.clone();
        tokio::spawn(async move {
            info!("MemoryDaemon: Scheduled GC loop started (24h interval)");
            loop {
                sleep(Duration::from_secs(24 * 3600)).await;
                info!("MemoryDaemon: Running scheduled garbage_collect()");
                match manager_gc.garbage_collect().await {
                    Ok(stats) => info!("MemoryDaemon: Scheduled GC completed. Bytes freed: {}, Orphans cleaned: {}", stats.bytes_freed, stats.orphaned_vectors_cleaned),
                    Err(e) => error!("MemoryDaemon: Scheduled GC failed: {}", e),
                }
            }
        });
    }
}
