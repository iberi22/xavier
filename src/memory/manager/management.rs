//! Memory manager query and statistics operations
//!
//! Methods for retrieving managed memories, computing statistics,
//! promoting/demoting priorities, and executing legacy action types.

use anyhow::Result;
use std::collections::HashMap;
use tracing::info;

use super::core::MemoryManager;
use super::types::{ManagedMemory, MemoryPriority, MemoryQuality, MemoryStats};

impl MemoryManager {
    /// Get statistics about all memories
    pub async fn get_stats(&self) -> Result<MemoryStats> {
        let docs = self.memory.all_documents().await;
        let mut stats = MemoryStats {
            total_documents: docs.len(),
            total_size_bytes: docs.iter().map(|d| d.estimated_bytes()).sum(),
            by_priority: HashMap::new(),
            by_quality_bucket: HashMap::new(),
            low_quality_count: 0,
            ephemeral_count: 0,
            decayed_count: 0,
        };

        for doc in docs {
            let priority = MemoryPriority::from_metadata(&doc.metadata);
            *stats
                .by_priority
                .entry(priority.as_str().to_string())
                .or_insert(0) += 1;

            let (access_count, last_access) = if let Some(id) = &doc.id {
                let counts = self
                    .access_counts
                    .lock()
                    .expect("manager: access_counts lock poisoned");
                let times = self
                    .last_access_times
                    .lock()
                    .expect("manager: last_access_times lock poisoned");
                (counts.get(id).copied().unwrap_or(0), times.get(id).copied())
            } else {
                (0, None)
            };

            let mut verified = false;
            if let (Some(graph_lock), Some(doc_id)) = (&self._belief_graph, &doc.id) {
                verified = graph_lock.read().await.has_supporting_beliefs(doc_id).await;
            }

            let quality =
                MemoryQuality::calculate(&doc, priority, access_count, last_access, verified);

            let bucket = if quality.overall >= 0.7 {
                "high"
            } else if quality.overall >= 0.4 {
                "medium"
            } else {
                "low"
            };
            *stats
                .by_quality_bucket
                .entry(bucket.to_string())
                .or_insert(0) += 1;

            if quality.overall < self.config.quality_threshold {
                stats.low_quality_count += 1;
            }
            if priority == MemoryPriority::Ephemeral {
                stats.ephemeral_count += 1;
            }
        }

        Ok(stats)
    }

    /// Get all managed memories with their quality scores
    pub async fn get_all_memories(&self) -> Result<Vec<ManagedMemory>> {
        let docs = self.memory.all_documents().await;

        let mut memories = Vec::new();
        for doc in docs {
            let priority = MemoryPriority::from_metadata(&doc.metadata);
            let (access_count, last_access, created_at) = if let Some(id) = &doc.id {
                let counts = self
                    .access_counts
                    .lock()
                    .expect("manager: access_counts lock poisoned");
                let times = self
                    .last_access_times
                    .lock()
                    .expect("manager: last_access_times lock poisoned");
                let created = self
                    .created_times
                    .lock()
                    .expect("manager: created_times lock poisoned");

                (
                    counts.get(id).copied().unwrap_or(0),
                    times.get(id).copied(),
                    created.get(id).copied(),
                )
            } else {
                (0, None, None)
            };

            let mut verified = false;
            if let (Some(graph_lock), Some(doc_id)) = (&self._belief_graph, &doc.id) {
                verified = graph_lock.read().await.has_supporting_beliefs(doc_id).await;
            }

            let quality =
                MemoryQuality::calculate(&doc, priority, access_count, last_access, verified);

            memories.push(ManagedMemory {
                doc,
                priority,
                quality,
                access_count,
                last_access,
                created_at,
                size_bytes: 0, // Will be computed if needed
            });
        }

        Ok(memories)
    }

    /// Get memories below quality threshold
    pub async fn get_low_quality_memories(&self, threshold: f32) -> Result<Vec<ManagedMemory>> {
        let all = self.get_all_memories().await?;
        Ok(all
            .into_iter()
            .filter(|m| m.quality.overall < threshold)
            .collect())
    }

    /// Get memories by priority
    pub async fn get_memories_by_priority(
        &self,
        priority: MemoryPriority,
    ) -> Result<Vec<ManagedMemory>> {
        let all = self.get_all_memories().await?;
        Ok(all.into_iter().filter(|m| m.priority == priority).collect())
    }

    /// Promote a memory's priority
    pub async fn promote_memory(&self, doc_id: &str, new_priority: MemoryPriority) -> Result<()> {
        if let Some(mut doc) = self.memory.get(doc_id).await? {
            let old_priority = MemoryPriority::from_metadata(&doc.metadata);
            doc.metadata["memory_priority"] = serde_json::json!(new_priority.as_str());
            self.memory.update(doc).await?;

            let mut relevance = self
                .relevance_scores
                .lock()
                .expect("manager: relevance_scores lock poisoned");
            let current = relevance.get(doc_id).copied().unwrap_or(1.0);
            // Boost relevance on promotion
            relevance.insert(doc_id.to_string(), (current * 1.2).min(1.0));

            info!(
                "Promoted memory {} from {} to {}",
                doc_id,
                old_priority.as_str(),
                new_priority.as_str()
            );
        }
        Ok(())
    }

    /// Demote a memory's priority
    pub async fn demote_memory(&self, doc_id: &str, new_priority: MemoryPriority) -> Result<()> {
        if let Some(mut doc) = self.memory.get(doc_id).await? {
            let old_priority = MemoryPriority::from_metadata(&doc.metadata);
            doc.metadata["memory_priority"] = serde_json::json!(new_priority.as_str());
            self.memory.update(doc).await?;

            let mut relevance = self
                .relevance_scores
                .lock()
                .expect("manager: relevance_scores lock poisoned");
            let current = relevance.get(doc_id).copied().unwrap_or(1.0);
            // Reduce relevance on demotion
            relevance.insert(doc_id.to_string(), current * 0.8);

            info!(
                "Demoted memory {} from {} to {}",
                doc_id,
                old_priority.as_str(),
                new_priority.as_str()
            );
        }
        Ok(())
    }
}
