//! Memory compression — truncate large documents to save storage.

use anyhow::Result;
use tracing::info;

use super::core::MemoryManager;
use super::types::{ManagementResult, MemoryManagementAction};
use crate::domain::cycle_breaks::w30_06::SemanticCompactor;

impl MemoryManager {
    /// Compress large memories semantically using an LLM to generate a dense summary.
    ///
    /// The LLM is reached through the [`SemanticCompactor`] port declared in
    /// `crate::domain::cycle_breaks::w30_06`, so this module no longer imports the
    /// provider adapter (ADR-033 Wave 0). The caller owns the adapter and with it the
    /// compaction-model selection; a compactor that returns `None` falls back to
    /// plain truncation, exactly as a failed LLM call always has.
    pub async fn compact_semantically(
        &self,
        compactor: &dyn SemanticCompactor,
    ) -> Result<ManagementResult> {
        let docs = self.memory.all_documents().await;
        let threshold = self.config.compression_threshold_bytes;
        let mut actions = Vec::new();
        let mut bytes_freed: u64 = 0;

        for doc in docs {
            let size = doc.content.len();
            if size > threshold {
                let Some(doc_id) = &doc.id else {
                    continue;
                };

                // Skip critical priority
                if super::types::MemoryPriority::from_metadata(&doc.metadata)
                    == super::types::MemoryPriority::Critical
                {
                    continue;
                }

                info!(
                    "Semantically compacting memory {} (size: {} bytes)",
                    doc_id, size
                );

                let prompt = format!(
                    "You are an expert cognitive archivist. The following memory document is too large and needs to be semantically compacted.\n\
                     Retain ALL key facts, entities, decisions, and technical details, but remove verbosity, repetition, and filler text.\n\
                     Keep the format clear and dense.\n\n\
                     CONTENT:\n{}",
                    doc.content
                );

                let compacted_content = match compactor.compact_document(&prompt).await {
                    Some(compacted) => compacted,
                    None => {
                        // Fallback to basic truncation if LLM fails
                        format!(
                            "{}...[truncated from {} chars]",
                            crate::memory::snippet::clip_chars(
                                &doc.content,
                                threshold.saturating_sub(20)
                            ),
                            size
                        )
                    }
                };

                let old_size = size as u64;
                let new_size = compacted_content.len() as u64;
                let freed = old_size.saturating_sub(new_size);

                if freed > 0 {
                    let mut updated_doc = doc.clone();
                    updated_doc.content = compacted_content;
                    updated_doc.metadata["semantically_compacted"] = serde_json::json!(true);
                    updated_doc.metadata["original_size"] = serde_json::json!(old_size);

                    if self.memory.update(updated_doc).await.is_ok() {
                        bytes_freed += freed;
                        actions.push(MemoryManagementAction::Compressed {
                            doc_id: doc_id.clone(),
                            old_size,
                            new_size,
                        });
                    }
                }
            }
        }

        info!(
            "Semantic compaction complete: {} compacted, {} bytes freed",
            actions.len(),
            bytes_freed
        );

        Ok(ManagementResult {
            documents_affected: actions.len(),
            actions,
            bytes_freed,
        })
    }
}
