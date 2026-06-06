//! Memory compression — truncate large documents to save storage.

use anyhow::Result;
use tracing::info;

use super::core::MemoryManager;
use super::types::{MemoryManagementAction, ManagementResult};

impl MemoryManager {
    /// Compress large memories by truncating oversized content.
    pub async fn compress_large_memories(&self) -> Result<ManagementResult> {
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

                let compressed_content = if doc.content.len() > threshold {
                    format!(
                        "{}...[compressed from {} chars]",
                        &doc.content[..threshold.saturating_sub(20)],
                        doc.content.len()
                    )
                } else {
                    doc.content.clone()
                };

                let old_size = doc.content.len() as u64;
                let new_size = compressed_content.len() as u64;
                let freed = old_size.saturating_sub(new_size);

                if freed > 0 {
                    let mut updated_doc = doc.clone();
                    updated_doc.content = compressed_content;
                    updated_doc.metadata["compressed"] = serde_json::json!(true);
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
            "Compression complete: {} compressed, {} bytes freed",
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
