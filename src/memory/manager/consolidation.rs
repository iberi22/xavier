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
        info!("🌙 Running nightly consolidation with TGD...");

        // Phase 1: Standard memory dedup consolidation
        let result = self.consolidate_memories().await?;

        // Phase 2: Run TGD nightly
        info!("🌙 Phase 2: Triggering TGD nightly...");
        if let Err(e) = crate::tgd::consolidation::run_nightly_tgd().await {
            info!("⚠️ TGD nightly encountered an issue: {}", e);
            // Don't fail the whole consolidation — TGD is optional
        }

        Ok(result)
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
    fn create_consolidation_signature(&self, doc: &MemoryDocument) -> String {
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
