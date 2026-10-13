//! Memory eviction and auto-management
//!
//! Logic for evicting low-quality memories, priority-based eviction,
//! and the full auto-management cycle (decay → consolidate → evict → storage check).

use anyhow::Result;
use tracing::info;

use super::core::MemoryManager;
use super::types::{ManagementResult, MemoryManagementAction, MemoryPriority};
use crate::memory::access;
use crate::memory::store::MemoryRecord;
use chrono::Utc;

impl MemoryManager {
    /// Evict memories based on quality threshold and priority
    pub async fn evict_low_quality(&self) -> Result<ManagementResult> {
        let threshold = self.config.quality_threshold;
        let low_quality = self.get_low_quality_memories(threshold).await?;

        let store_opt = self.memory.store().await;
        let observation_started_at = if let Some(store) = &store_opt {
            access::observation_started_at(store.as_ref())
                .await
                .unwrap_or(None)
        } else {
            None
        };
        let gate = access::prune_block_reason(observation_started_at, Utc::now());

        let mut actions = Vec::new();
        let mut bytes_freed: u64 = 0;
        let mut evicted_count = 0;
        let mut kept_no_evidence = 0;

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
                let record =
                    MemoryRecord::from_document(&self.memory.workspace_id, &memory.doc, true, None);
                let is_observable = access::record_is_observable(&record);
                let has_evidence_of_disuse = is_observable
                    && gate == access::PruneBlockReason::Eligible
                    && memory.access_count == 0;

                if !has_evidence_of_disuse {
                    kept_no_evidence += 1;
                    continue;
                }

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
            "Eviction complete: {} evicted, {} bytes freed (kept_no_evidence={})",
            evicted_count, bytes_freed, kept_no_evidence
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::access;
    use crate::memory::manager::types::MemoryManagerConfig;
    use crate::memory::qmd_memory::{MemoryDocument, QmdMemory};
    use crate::memory::schema::MemoryLevel;
    use crate::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
    use crate::memory::store::{InMemoryMemoryStore, MemoryStore};
    use std::sync::Arc;
    use tokio::sync::RwLock as AsyncRwLock;

    fn seed_aged_clock(path: &std::path::Path, days: i64) {
        let started = chrono::Utc::now() - chrono::Duration::days(days);
        let conn = rusqlite::Connection::open(path).expect("open new db");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS maintenance_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )
        .expect("create maintenance_meta");
        conn.execute(
            "INSERT OR IGNORE INTO maintenance_meta (key, value, updated_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![
                access::OBSERVATION_STARTED_AT_KEY,
                started.to_rfc3339(),
                started.to_rfc3339()
            ],
        )
        .expect("seed clock");
        conn.close().expect("close");
    }

    async fn setup_manager_with_store(store: Arc<dyn MemoryStore>) -> MemoryManager {
        let memory = Arc::new(QmdMemory::new_with_workspace(
            Arc::new(AsyncRwLock::new(Vec::new())),
            "test_ws",
        ));
        memory.set_store(store).await;
        MemoryManager::with_config(
            Arc::clone(&memory),
            None,
            MemoryManagerConfig {
                quality_threshold: 0.5,
                ..Default::default()
            },
        )
    }

    #[tokio::test]
    async fn eviction_keeps_low_priority_record_without_access_evidence() {
        crate::isolate_test_process!();
        let store = Arc::new(InMemoryMemoryStore::new());
        let manager = setup_manager_with_store(store).await;

        let doc = MemoryDocument {
            id: Some("doc_1".to_string()),
            path: "test/doc1".to_string(),
            content: "test".to_string(),
            embedding: vec![0.1, 0.2],
            level: MemoryLevel::Raw,
            metadata: serde_json::json!({
                "memory_priority": "low",
                "priority": "low"
            }),
            ..Default::default()
        };
        manager.memory.docs.write().await.push(doc);
        manager
            .access_counts
            .lock()
            .unwrap()
            .insert("doc_1".to_string(), 0);

        let result = manager.evict_low_quality().await.unwrap();
        assert_eq!(
            result.documents_affected, 0,
            "No access evidence, should be kept"
        );
    }

    #[tokio::test]
    async fn eviction_removes_low_priority_record_with_evidence_of_disuse() {
        crate::isolate_test_process!();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("eviction_disuse.sqlite3");
        seed_aged_clock(&path, 40);
        let store = Arc::new(
            VecSqliteMemoryStore::new(VecSqliteStoreConfig {
                path,
                embedding_dimensions: 8,
            })
            .await
            .expect("store construction"),
        );
        let manager = setup_manager_with_store(store).await;

        let doc = MemoryDocument {
            id: Some("doc_1".to_string()),
            path: "test/doc1".to_string(),
            content: "test".to_string(),
            embedding: vec![0.1, 0.2],
            level: MemoryLevel::Raw,
            metadata: serde_json::json!({
                "memory_priority": "low",
                "priority": "low"
            }),
            ..Default::default()
        };
        manager.memory.docs.write().await.push(doc);
        manager
            .access_counts
            .lock()
            .unwrap()
            .insert("doc_1".to_string(), 0);

        let result = manager.evict_low_quality().await.unwrap();
        assert_eq!(
            result.documents_affected, 1,
            "Evidence of disuse, should be evicted"
        );
    }
}
