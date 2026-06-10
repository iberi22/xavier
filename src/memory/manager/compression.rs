//! Memory compression — truncate large documents to save storage.

use anyhow::Result;
use tracing::info;

use super::core::MemoryManager;
use super::types::{ManagementResult, MemoryManagementAction};

impl MemoryManager {
    /// Compress large memories semantically using an LLM to generate a dense summary.
    pub async fn compact_semantically(&self) -> Result<ManagementResult> {
        use crate::agents::provider::ModelProviderClient;

        let docs = self.memory.all_documents().await;
        let threshold = self.config.compression_threshold_bytes;
        let mut actions = Vec::new();
        let mut bytes_freed: u64 = 0;

        // Initialize the LLM client once, prioritizing compaction_model if set
        let settings = crate::settings::XavierSettings::current();
        let compaction_model = settings.models.compaction_model.clone();
        let llm_client = ModelProviderClient::from_model_override(compaction_model);

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

                let compacted_content = match llm_client.generate_response(&prompt, &[]).await {
                    Ok(res) if !res.text.trim().is_empty() => res.text.trim().to_string(),
                    _ => {
                        // Fallback to basic truncation if LLM fails
                        format!(
                            "{}...[truncated from {} chars]",
                            &doc.content[..threshold.saturating_sub(20)],
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
