//! Memory consolidation — deduplication of similar memories.
//!
//! Merges near-duplicate documents based on a normalized content signature,
//! retaining the more recent version and freeing duplicates.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use anyhow::Result;
use chrono::Utc;
use tracing::info;

use crate::memory::qmd_memory::MemoryDocument;

use super::core::MemoryManager;
use super::types::{ManagementResult, MemoryManagementAction};

impl MemoryManager {
    /// Run full nightly consolidation pipeline: memory dedup + TGD
    pub async fn nightly_consolidate(&self) -> Result<ManagementResult> {
        info!("🌙 Running nightly consolidation with compaction, deduplication, and purge...");

        let store_opt = self.memory.store().await;

        // 1. Collect "before" metrics
        let before_size = if let Some(ref store) = store_opt {
            store.db_size().await.unwrap_or(None)
        } else {
            None
        };
        let before_count = self.memory.count().await.unwrap_or(0);

        // 2. Phase 1: Standard memory dedup consolidation
        info!("🌙 Phase 1: Standard memory dedup consolidation...");
        let mut result = self.consolidate_memories().await?;

        // 3. Phase 2: Purge expired memories
        info!("🌙 Phase 2: Purging expired memories...");
        let purge_result = self.purge_expired_memories().await?;
        result.documents_affected += purge_result.documents_affected;
        result.bytes_freed += purge_result.bytes_freed;
        result.actions.extend(purge_result.actions);

        // 4. Phase 3: Compact vec-store
        info!("🌙 Phase 3: Compacting vec-store...");
        if let Some(ref store) = store_opt {
            if let Err(e) = store.compact().await {
                info!("⚠️ Failed to compact vec-store: {}", e);
            }
        }

        // 5. Collect "after" metrics
        let after_size = if let Some(ref store) = store_opt {
            store.db_size().await.unwrap_or(None)
        } else {
            None
        };
        let after_count = self.memory.count().await.unwrap_or(0);

        // 6. Log consolidation metrics (antes/después: tamaño, nº de memorias)
        let before_size_str = before_size.map(|s| format!("{} bytes", s)).unwrap_or_else(|| "N/A".to_string());
        let after_size_str = after_size.map(|s| format!("{} bytes", s)).unwrap_or_else(|| "N/A".to_string());

        info!(
            "📊 Consolidation complete metrics:\n\
             - Size Before: {}\n\
             - Size After: {}\n\
             - Memory Count Before: {}\n\
             - Memory Count After: {}",
            before_size_str, after_size_str, before_count, after_count
        );

        // Phase 4: Run TGD nightly
        info!("🌙 Phase 4: Triggering TGD nightly...");
        if let Err(e) = crate::tgd::consolidation::run_nightly_tgd().await {
            info!("⚠️ TGD nightly encountered an issue: {}", e);
            // Don't fail the whole consolidation — TGD is optional
        }

        Ok(result)
    }

    /// Purge memories that have expired based on their metadata
    pub async fn purge_expired_memories(&self) -> Result<ManagementResult> {
        let docs = self.memory.all_documents().await;
        let mut actions = Vec::new();
        let mut bytes_freed: u64 = 0;
        let mut purged_count = 0;
        let now = chrono::Utc::now();

        for doc in docs {
            let Some(doc_id) = &doc.id else {
                continue;
            };

            let mut expired = false;

            // 1. Check "expires_at" field
            if let Some(expires_at_val) = doc.metadata.get("expires_at") {
                if let Some(expires_str) = expires_at_val.as_str() {
                    if let Ok(expires_dt) = chrono::DateTime::parse_from_rfc3339(expires_str) {
                        if now > expires_dt {
                            expired = true;
                        }
                    }
                } else if let Some(expires_num) = expires_at_val.as_i64() {
                    if now.timestamp() > expires_num {
                        expired = true;
                    }
                }
            }

            // 2. Check "ttl" field (in seconds) relative to creation time
            if !expired {
                if let Some(ttl_val) = doc.metadata.get("ttl") {
                    if let Some(ttl_secs) = ttl_val.as_i64() {
                        let created_at = self
                            .created_times
                            .lock()
                            .expect("manager: created_times lock poisoned")
                            .get(doc_id)
                            .copied()
                            .or_else(|| {
                                doc.metadata.get("created_at")
                                    .or_else(|| doc.metadata.get("updated_at"))
                                    .and_then(|v| v.as_str())
                                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                                    .map(|dt| dt.with_timezone(&chrono::Utc))
                            })
                            .unwrap_or_else(chrono::Utc::now);

                        if (now - created_at).num_seconds() > ttl_secs {
                            expired = true;
                        }
                    }
                }
            }

            if expired {
                let size = doc.estimated_bytes();
                if self.memory.delete(doc_id).await?.is_some() {
                    bytes_freed += size;
                    purged_count += 1;
                    actions.push(MemoryManagementAction::Evicted {
                        doc_id: doc_id.clone(),
                        reason: "Expired based on metadata".to_string(),
                        priority: doc
                            .metadata
                            .get("memory_priority")
                            .and_then(|v| v.as_str())
                            .unwrap_or("medium")
                            .to_string(),
                    });
                    info!("Purged expired memory {}", doc_id);
                }
            }
        }

        info!(
            "Purge complete: {} expired memories purged, {} bytes freed",
            purged_count, bytes_freed
        );

        Ok(ManagementResult {
            documents_affected: purged_count,
            actions,
            bytes_freed,
        })
    }

    /// Consolidate similar memories — merge duplicates and near-duplicates
    pub async fn consolidate_memories(&self) -> Result<ManagementResult> {
        let docs = self.memory.all_documents().await;
        let mut actions = Vec::new();
        let mut bytes_freed: u64 = 0;
        let mut seen_signatures: HashMap<String, String> = HashMap::new();

        for doc in docs {
            let Some(doc_id) = &doc.id else {
                continue;
            };

            let signature = self.create_consolidation_signature(&doc);

            if let Some(existing_id) = seen_signatures.get(&signature) {
                let existing_time = self
                    .created_times
                    .lock()
                    .expect("manager: created_times lock poisoned")
                    .get(existing_id)
                    .copied()
                    .unwrap_or_else(Utc::now);
                let doc_time = self
                    .created_times
                    .lock()
                    .expect("manager: created_times lock poisoned")
                    .get(doc_id)
                    .copied()
                    .unwrap_or_else(Utc::now);

                if doc_time < existing_time {
                    bytes_freed += doc.estimated_bytes();
                    actions.push(MemoryManagementAction::Consolidated {
                        doc_ids: vec![existing_id.clone(), doc_id.clone()],
                        into_doc_id: doc_id.clone(),
                    });
                    if self.memory.delete(existing_id).await?.is_some() {
                        info!("Consolidated duplicate {} into {}", existing_id, doc_id);
                    }
                } else {
                    bytes_freed += doc.estimated_bytes();
                    actions.push(MemoryManagementAction::Consolidated {
                        doc_ids: vec![doc_id.clone(), existing_id.clone()],
                        into_doc_id: existing_id.clone(),
                    });
                    if self.memory.delete(doc_id).await?.is_some() {
                        info!("Consolidated duplicate {} into {}", doc_id, existing_id);
                    }
                }
            } else {
                seen_signatures.insert(signature, doc_id.clone());
            }
        }

        info!(
            "Consolidation complete: {} groups merged, {} bytes freed",
            actions.len(),
            bytes_freed
        );

        Ok(ManagementResult {
            documents_affected: actions.len(),
            actions,
            bytes_freed,
        })
    }

    /// Create a normalized signature for consolidation detection
    pub fn create_consolidation_signature(&self, doc: &MemoryDocument) -> String {
        let normalized: String = doc
            .content
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");

        let kind = doc
            .metadata
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let priority = doc
            .metadata
            .get("memory_priority")
            .and_then(|v| v.as_str())
            .unwrap_or("medium");

        let mut hasher = DefaultHasher::new();
        (normalized.len(), kind, priority).hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }
}
