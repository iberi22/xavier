// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Core MemoryManager struct and constructors
//!
//! The [`MemoryManager`] provides autonomous memory lifecycle management:
//! memory prioritization, decay, quality scoring, consolidation, and eviction.
use anyhow::Result;
use tracing::info;

use std::collections::HashMap;
use std::sync::Arc;

use super::types::{ManagementResult, MemoryAction};
use crate::memory::qmd_memory::QmdMemory;

use super::types::MemoryManagerConfig;

/// Intelligent Memory Manager - manages memory lifecycle autonomously
pub struct MemoryManager {
    pub(crate) memory: Arc<QmdMemory>,
    pub(crate) _belief_graph: Option<crate::memory::belief_graph::SharedBeliefGraph>,
    pub(crate) config: MemoryManagerConfig,
    /// Track access counts per document
    pub(crate) access_counts: std::sync::Mutex<HashMap<String, usize>>,
    /// Track last access times
    pub(crate) last_access_times: std::sync::Mutex<HashMap<String, chrono::DateTime<chrono::Utc>>>,
    /// Track created times
    pub(crate) created_times: std::sync::Mutex<HashMap<String, chrono::DateTime<chrono::Utc>>>,
    /// Relevance scores (can be decayed over time)
    pub(crate) relevance_scores: std::sync::Mutex<HashMap<String, f32>>,
}

impl MemoryManager {
    pub fn new(
        memory: Arc<QmdMemory>,
        belief_graph: Option<crate::memory::belief_graph::SharedBeliefGraph>,
    ) -> Self {
        Self {
            memory,
            _belief_graph: belief_graph,
            config: MemoryManagerConfig::default(),
            access_counts: std::sync::Mutex::new(HashMap::new()),
            last_access_times: std::sync::Mutex::new(HashMap::new()),
            created_times: std::sync::Mutex::new(HashMap::new()),
            relevance_scores: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub fn with_config(
        memory: Arc<QmdMemory>,
        belief_graph: Option<crate::memory::belief_graph::SharedBeliefGraph>,
        config: MemoryManagerConfig,
    ) -> Self {
        Self {
            memory,
            _belief_graph: belief_graph,
            config,
            access_counts: std::sync::Mutex::new(HashMap::new()),
            last_access_times: std::sync::Mutex::new(HashMap::new()),
            created_times: std::sync::Mutex::new(HashMap::new()),
            relevance_scores: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Get configuration
    pub fn config(&self) -> &MemoryManagerConfig {
        &self.config
    }

    /// Expose the shared memory store so consolidation/reflection can mutate it.
    pub fn memory(&self) -> Arc<QmdMemory> {
        Arc::clone(&self.memory)
    }

    /// Update configuration
    pub fn set_config(&mut self, config: MemoryManagerConfig) {
        self.config = config;
    }

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
}
