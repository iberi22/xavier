//! Memory eviction and auto-management
//!
//! Logic for evicting low-quality memories, priority-based eviction,
//! and the full auto-management cycle (decay → consolidate → evict → storage check).

use anyhow::Result;
use tracing::info;

use super::core::MemoryManager;
use super::types::{ManagementResult, MemoryManagementAction, MemoryPriority};

impl MemoryManager {
    /// Prune Gestalt bus execution events older than 7 days (or XAVIER_GESTALT_BUS_RETENTION_DAYS).
    pub async fn prune_expired_bus_events(&self) -> Result<usize> {
        let retention_days = std::env::var("XAVIER_GESTALT_BUS_RETENTION_DAYS")
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(7);

        let docs = self.memory.all_documents().await;
        let now = chrono::Utc::now();
        let threshold = now - chrono::Duration::days(retention_days);
        let mut pruned = 0;

        for doc in docs {
            if crate::memory::qmd::writer::is_gestalt_bus_execution(&doc.path, &doc.metadata) {
                let doc_time = doc
                    .metadata
                    .get("created_at")
                    .or_else(|| doc.metadata.get("provenance").and_then(|p| p.get("recorded_at")))
                    .or_else(|| doc.metadata.get("provenance").and_then(|p| p.get("observed_at")))
                    .or_else(|| doc.metadata.get("updated_at"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or(now);

                if doc_time < threshold {
                    let id_or_path = doc.id.clone().unwrap_or_else(|| doc.path.clone());
                    if self.memory.delete(&id_or_path).await?.is_some() {
                        pruned += 1;
                    }
                }
            }
        }

        if pruned > 0 {
            info!(
                "Pruned {} expired Gestalt bus execution documents older than {} days",
                pruned, retention_days
            );
        }

        Ok(pruned)
    }

    /// Evict memories based on quality threshold and priority
    pub async fn evict_low_quality(&self) -> Result<ManagementResult> {
        let threshold = self.config.quality_threshold;
        let low_quality = self.get_low_quality_memories(threshold).await?;

        let mut actions = Vec::new();
        let mut bytes_freed: u64 = 0;
        let mut evicted_count = 0;

        for memory in low_quality {
            let Some(doc_id) = &memory.doc.id else {
                continue;
            };
            let priority = memory.priority;

            if priority == MemoryPriority::Critical {
                continue;
            }

            let should_evict = match priority {
                MemoryPriority::Critical => false,
                MemoryPriority::High => memory.quality.overall < 0.1,
                MemoryPriority::Medium => memory.quality.overall < threshold,
                MemoryPriority::Low => true,
                MemoryPriority::Ephemeral => true,
            };

            if should_evict {
                let size = memory.doc.estimated_bytes();
                if self.memory.delete(doc_id).await?.is_some() {
                    bytes_freed += size;
                    evicted_count += 1;
                    actions.push(MemoryManagementAction::Evicted {
                        doc_id: doc_id.clone(),
                        reason: format!(
                            "Quality {} below threshold {}",
                            memory.quality.overall, threshold
                        ),
                        priority: priority.as_str().to_string(),
                    });
                    info!(
                        "Evicted memory {} (priority={}, quality={:.2})",
                        doc_id,
                        priority.as_str(),
                        memory.quality.overall
                    );
                }
            }
        }

        info!(
            "Eviction complete: {} evicted, {} bytes freed",
            evicted_count, bytes_freed
        );

        Ok(ManagementResult {
            documents_affected: evicted_count,
            actions,
            bytes_freed,
        })
    }

    /// Evict memories by specific priority level
    pub async fn evict_by_priority(&self, priority: MemoryPriority) -> Result<ManagementResult> {
        let memories = self.get_memories_by_priority(priority).await?;

        let mut actions = Vec::new();
        let mut bytes_freed: u64 = 0;
        let mut evicted_count = 0;

        for memory in memories {
            let Some(doc_id) = &memory.doc.id else {
                continue;
            };

            if priority == MemoryPriority::Critical {
                info!("Skipping eviction of critical memory {}", doc_id);
                continue;
            }

            let size = memory.doc.estimated_bytes();
            if self.memory.delete(doc_id).await?.is_some() {
                bytes_freed += size;
                evicted_count += 1;
                actions.push(MemoryManagementAction::Evicted {
                    doc_id: doc_id.clone(),
                    reason: format!("Manual eviction by priority={}", priority.as_str()),
                    priority: priority.as_str().to_string(),
                });
            }
        }

        info!(
            "Priority eviction ({}): {} evicted, {} bytes freed",
            priority.as_str(),
            evicted_count,
            bytes_freed
        );

        Ok(ManagementResult {
            documents_affected: evicted_count,
            actions,
            bytes_freed,
        })
    }

    /// Full auto-management cycle: decay → consolidate → evict → storage check
    pub async fn auto_manage(&self) -> Result<usize> {
        let mut total_actions = 0;

        // First, prune expired Gestalt bus execution events
        if let Ok(bus_pruned) = self.prune_expired_bus_events().await {
            total_actions += bus_pruned;
        }

        if self.config.auto_decay_enabled {
            let decay_result = self.decay_memories().await?;
            total_actions += decay_result.documents_affected;

            // Reorganize after decay
            let _ = self.flatten_reorganize().await;
        }

        if self.config.auto_consolidate_enabled {
            let consolidate_result = self.consolidate_memories().await?;
            total_actions += consolidate_result.documents_affected;
        }

        if self.config.auto_evict_enabled {
            let evict_result = self.evict_low_quality().await?;
            total_actions += evict_result.documents_affected;
        }

        let stats = self.get_stats().await?;
        if stats.total_size_bytes > self.config.max_storage_bytes {
            info!(
                "Storage limit exceeded ({} > {}), triggering aggressive eviction",
                stats.total_size_bytes, self.config.max_storage_bytes
            );
            let ratio = stats.total_size_bytes as f64 / self.config.max_storage_bytes as f64;
            let extra_threshold = self.config.quality_threshold * (ratio as f32);
            let low_quality = self.get_low_quality_memories(extra_threshold).await?;
            for memory in low_quality {
                if memory.priority != MemoryPriority::Critical {
                    if let Some(doc_id) = &memory.doc.id {
                        let _ = self.memory.delete(doc_id).await;
                        total_actions += 1;
                    }
                }
            }
        }

        info!(
            "Auto-manage cycle complete: {} total actions",
            total_actions
        );
        Ok(total_actions)
    }
}
