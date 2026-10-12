//! Memory compression — semantically compress large documents to save storage.

use anyhow::Result;
use tracing::{info, warn};

use super::core::MemoryManager;
use super::types::{ManagementResult, MemoryManagementAction};
use crate::domain::cycle_breaks::w30_06::SemanticCompactor;

impl MemoryManager {
    /// Compress large memories semantically using an LLM to generate a dense summary.
    ///
    /// The LLM is reached through the [`SemanticCompactor`] port declared in
    /// `crate::domain::cycle_breaks::w30_06`, so this module no longer imports the
    /// provider adapter (ADR-033 Wave 0). The caller owns the adapter and with it the
    /// compaction-model selection; if the compactor returns `None` or an empty/whitespace
    /// string, the document is left untouched without falling back to truncation.
    pub async fn compact_semantically(
        &self,
        compactor: &dyn SemanticCompactor,
    ) -> Result<ManagementResult> {
        let docs = self.memory.all_documents().await;
        let threshold = self.config.compression_threshold_bytes;
        let mut actions = Vec::new();
        let mut bytes_freed: u64 = 0;
        let mut skipped_count: usize = 0;

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
                    Some(compacted) if !compacted.trim().is_empty() => compacted.trim().to_string(),
                    _ => {
                        skipped_count += 1;
                        continue;
                    }
                };

                let old_size = size as u64;
                let new_size = compacted_content.len() as u64;
                if new_size >= old_size {
                    skipped_count += 1;
                    continue;
                }
                let freed = old_size - new_size;

                let mut updated_doc = doc.clone();
                updated_doc.content = compacted_content;
                updated_doc.metadata["semantically_compacted"] = serde_json::json!(true);
                updated_doc.metadata["original_size"] = serde_json::json!(old_size);

                match self.memory.update(updated_doc).await {
                    Ok(_) => {
                        bytes_freed += freed;
                        actions.push(MemoryManagementAction::Compressed {
                            doc_id: doc_id.clone(),
                            old_size,
                            new_size,
                        });
                    }
                    Err(e) => warn!("Semantic compaction update failed for {}: {}", doc_id, e),
                }
            }
        }

        if skipped_count > 0 {
            warn!(
                "Semantic compaction skipped {} document(s) due to compactor returning no or invalid content",
                skipped_count
            );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::cycle_breaks::w30_06::SemanticCompactor;
    use crate::memory::manager::types::MemoryManagerConfig;
    use crate::memory::qmd_memory::{MemoryDocument, QmdMemory};
    use async_trait::async_trait;
    use std::sync::Arc;

    struct FakeCompactor {
        response: Option<String>,
    }

    #[async_trait]
    impl SemanticCompactor for FakeCompactor {
        async fn compact_document(&self, _prompt: &str) -> Option<String> {
            self.response.clone()
        }
    }

    async fn setup_manager_with_doc(content: &str) -> (MemoryManager, String) {
        let docs = Arc::new(tokio::sync::RwLock::new(Vec::new()));
        let qmd = Arc::new(QmdMemory::new(docs));
        let config = MemoryManagerConfig {
            compression_threshold_bytes: 100,
            ..Default::default()
        };
        let manager = MemoryManager::with_config(qmd, None, config);

        let doc_id = "doc-test-1".to_string();
        let doc = MemoryDocument {
            id: Some(doc_id.clone()),
            path: "test/large.md".to_string(),
            content: content.to_string(),
            metadata: serde_json::json!({"kind": "note"}),
            ..Default::default()
        };
        manager.memory.docs.write().await.push(doc);

        (manager, doc_id)
    }

    #[tokio::test]
    async fn test_compact_semantically_none_leaves_content_untouched() {
        let original_content = "A".repeat(200);
        let (manager, doc_id) = setup_manager_with_doc(&original_content).await;
        let compactor = FakeCompactor { response: None };

        let result = manager
            .compact_semantically(&compactor)
            .await
            .expect("compact_semantically succeeds");

        assert_eq!(result.actions.len(), 0, "No actions should be produced");
        let docs = manager.memory.all_documents().await;
        let doc = docs
            .iter()
            .find(|d| d.id.as_deref() == Some(&doc_id))
            .unwrap();
        assert_eq!(
            doc.content, original_content,
            "Stored content must be byte-identical when compactor returns None"
        );
    }

    #[tokio::test]
    async fn test_compact_semantically_empty_leaves_content_untouched() {
        let original_content = "B".repeat(200);
        let (manager, doc_id) = setup_manager_with_doc(&original_content).await;
        let compactor = FakeCompactor {
            response: Some("   \n\t".to_string()),
        };

        let result = manager
            .compact_semantically(&compactor)
            .await
            .expect("compact_semantically succeeds");

        assert_eq!(result.actions.len(), 0, "No actions should be produced");
        let docs = manager.memory.all_documents().await;
        let doc = docs
            .iter()
            .find(|d| d.id.as_deref() == Some(&doc_id))
            .unwrap();
        assert_eq!(
            doc.content, original_content,
            "Stored content must be byte-identical when compactor returns whitespace/empty"
        );
    }

    #[tokio::test]
    async fn test_compact_semantically_short_summary_replaces_content() {
        let original_content = "C".repeat(200);
        let (manager, doc_id) = setup_manager_with_doc(&original_content).await;
        let summary = "Dense concise summary".to_string();
        let compactor = FakeCompactor {
            response: Some(summary.clone()),
        };

        let result = manager
            .compact_semantically(&compactor)
            .await
            .expect("compact_semantically succeeds");

        assert_eq!(
            result.actions.len(),
            1,
            "One compressed action should be produced"
        );
        assert_eq!(result.documents_affected, 1);
        let docs = manager.memory.all_documents().await;
        let doc = docs
            .iter()
            .find(|d| d.id.as_deref() == Some(&doc_id))
            .unwrap();
        assert_eq!(
            doc.content, summary,
            "Stored content must be replaced with the summary"
        );
    }
}
