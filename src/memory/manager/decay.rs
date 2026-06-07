//! Memory decay logic
//!
//! Applies time-based relevance decay to all tracked memories
//! using priority-specific decay factors.

use anyhow::Result;
use chrono::Utc;
use tracing::info;

use super::core::MemoryManager;
use super::types::{ManagementResult, MemoryManagementAction, MemoryPriority};

impl MemoryManager {
    /// Apply decay to all memories based on time since last access
    pub async fn decay_memories(&self) -> Result<ManagementResult> {
        let docs = self.memory.all_documents().await;
        let mut actions = Vec::new();
        let mut decayed_count = 0;
        let mut relevance_map = self
            .relevance_scores
            .lock()
            .expect("manager: relevance_scores lock poisoned");

        for doc in docs {
            let Some(doc_id) = &doc.id else {
                continue;
            };
            let priority = MemoryPriority::from_metadata(&doc.metadata);
            let last_access = self
                .last_access_times
                .lock()
                .expect("manager: last_access_times lock poisoned")
                .get(doc_id)
                .copied();
            let created_at = self
                .created_times
                .lock()
                .expect("manager: created_times lock poisoned")
                .get(doc_id)
                .copied();

            let reference_time = last_access.or(created_at).unwrap_or_else(Utc::now);
            let days_since = (Utc::now() - reference_time).num_days() as f32;

            let decay_base = priority.decay_base();
            let old_relevance = *relevance_map.get(doc_id).unwrap_or(&1.0);
            let new_relevance = old_relevance * decay_base.powf(days_since);

            if (old_relevance - new_relevance).abs() > 0.001 {
                relevance_map.insert(doc_id.clone(), new_relevance);
                actions.push(MemoryManagementAction::Decayed {
                    doc_id: doc_id.clone(),
                    old_relevance,
                    new_relevance,
                });
                decayed_count += 1;
            }
        }

        info!(
            "Decay applied to {} memories (threshold: {})",
            decayed_count, self.config.quality_threshold
        );

        Ok(ManagementResult {
            documents_affected: decayed_count,
            actions,
            bytes_freed: 0,
        })
    }
}
