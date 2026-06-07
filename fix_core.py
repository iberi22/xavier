import re
import os

filepath = 'src/memory/manager/core.rs'
with open(filepath, 'r') as f:
    content = f.read()

# Add imports
content = "use anyhow::Result;\nuse tracing::info;\n" + content
content = content.replace("use crate::memory::qmd_memory::QmdMemory;", "use crate::memory::qmd_memory::QmdMemory;\nuse super::types::{MemoryAction, ManagementResult};")

# Add execute_actions and flatten_reorganize
new_methods = """
    /// Execute legacy action types for backwards compatibility
    pub async fn execute_actions(&self, actions: Vec<MemoryAction>) -> Result<usize> {
        let mut executed = 0;

        for action in actions {
            match action {
                MemoryAction::Delete { doc_id, reason } => {
                    info!("Deleting document {}: {}", doc_id, reason);
                    if self.memory.delete(&doc_id).await?.is_some() {
                        executed += 1;
                    }
                }
                MemoryAction::Compress { doc_id, reason } => {
                    info!("Compressing document {}: {}", doc_id, reason);
                    if let Some(mut doc) = self.memory.get(&doc_id).await? {
                        doc.metadata["compressed"] = serde_json::json!(true);
                        doc.metadata["compression_reason"] = serde_json::json!(reason);
                        let _ = self.memory.update(doc).await;
                        executed += 1;
                    }
                }
                MemoryAction::Update {
                    doc_id,
                    new_content,
                } => {
                    if let Some(mut doc) = self.memory.get(&doc_id).await? {
                        doc.content = new_content;
                        if self.memory.update(doc).await.is_ok() {
                            executed += 1;
                        }
                    }
                }
                MemoryAction::Curate { doc_id } => {
                    if let Some(mut doc) = self.memory.get(&doc_id).await? {
                        if let Some(meta) = doc.metadata.as_object_mut() {
                            if !meta.contains_key("memory_priority") {
                                meta.insert(
                                    "memory_priority".to_string(),
                                    serde_json::json!("medium"),
                                );
                            }
                            if !meta.contains_key("curated") {
                                meta.insert("curated".to_string(), serde_json::json!(true));
                                meta.insert(
                                    "curated_at".to_string(),
                                    serde_json::json!(chrono::Utc::now().to_rfc3339()),
                                );
                            }
                        }
                        if self.memory.update(doc).await.is_ok() {
                            executed += 1;
                        }
                    }
                }
                MemoryAction::Consolidate { doc_ids, reason } => {
                    info!(
                        "Consolidating documents: {} - {}",
                        doc_ids.join(", "),
                        reason
                    );
                    executed += 1;
                }
                MemoryAction::Keep => {}
            }
        }

        Ok(executed)
    }

    /// Flatten and reorganize memories to optimize storage and retrieval
    pub async fn flatten_reorganize(&self) -> Result<ManagementResult> {
        info!("Flattening and reorganizing memories...");
        // Stub implementation
        Ok(ManagementResult {
            actions: Vec::new(),
            documents_affected: 0,
            bytes_freed: 0,
        })
    }
"""

# Insert before the last closing brace
content = content.rstrip()
if content.endswith("}"):
    content = content[:-1] + new_methods + "}\n"

with open(filepath, 'w') as f:
    f.write(content)
