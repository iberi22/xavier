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
                .copied()
                .or_else(|| {
                    doc.metadata
                        .get("last_accessed_at")
                        .and_then(|v| v.as_str())
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                });
            let created_at = self
                .created_times
                .lock()
                .expect("manager: created_times lock poisoned")
                .get(doc_id)
                .copied()
                .or_else(|| {
                    doc.metadata
                        .get("created_at")
                        .and_then(|v| v.as_str())
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                });

            let reference_time = last_access.or(created_at).unwrap_or_else(Utc::now);
            let days_since =
                ((Utc::now() - reference_time).num_seconds() as f32 / 86400.0).max(0.0);

            let decay_base = priority.decay_base();
            let old_relevance = match relevance_map.get(doc_id) {
                Some(&score) => score,
                None => {
                    let seeded = doc
                        .metadata
                        .get("memory_importance")
                        .or_else(|| doc.metadata.get("importance"))
                        .and_then(|v| {
                            v.as_f64()
                                .map(|f| f as f32)
                                .or_else(|| v.as_str().and_then(|s| s.parse::<f32>().ok()))
                        })
                        .unwrap_or(1.0)
                        .clamp(0.0, 1.0);
                    relevance_map.insert(doc_id.clone(), seeded);
                    seeded
                }
            };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::qmd_memory::{MemoryDocument, QmdMemory};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn decay_lowers_quality_of_untouched_record() {
        let doc_id = "untouched_doc_1".to_string();
        let past_time = Utc::now() - chrono::Duration::days(10);
        let doc = MemoryDocument {
            id: Some(doc_id.clone()),
            path: "untouched.md".to_string(),
            content: "Untouched memory content".to_string(),
            metadata: serde_json::json!({
                "kind": "fact",
                "memory_priority": "medium",
                "created_at": past_time.to_rfc3339(),
                "last_accessed_at": past_time.to_rfc3339(),
            }),
            ..Default::default()
        };

        let docs = Arc::new(RwLock::new(vec![doc]));
        let qmd = Arc::new(QmdMemory::new(docs));
        let manager = MemoryManager::new(qmd, None);

        let before_memories = manager
            .get_all_memories()
            .await
            .expect("get_all_memories before decay");
        let before_record = before_memories
            .iter()
            .find(|m| m.doc.id.as_deref() == Some(&doc_id))
            .expect("record found before decay");
        let quality_before = before_record.quality.overall;

        let result = manager.decay_memories().await.expect("decay_memories");
        assert!(
            result.documents_affected >= 1,
            "decay should affect untouched document"
        );

        let after_memories = manager
            .get_all_memories()
            .await
            .expect("get_all_memories after decay");
        let after_record = after_memories
            .iter()
            .find(|m| m.doc.id.as_deref() == Some(&doc_id))
            .expect("record found after decay");
        let quality_after = after_record.quality.overall;

        assert!(
            quality_after < quality_before,
            "decay must lower quality of untouched record (before: {}, after: {})",
            quality_before,
            quality_after
        );
    }

    #[tokio::test]
    async fn decay_seed_reads_persisted_importance() {
        let doc_id = "persisted_importance_doc".to_string();
        let persisted_importance = 0.42_f32;
        let doc = MemoryDocument {
            id: Some(doc_id.clone()),
            path: "important.md".to_string(),
            content: "Important memory content".to_string(),
            metadata: serde_json::json!({
                "kind": "fact",
                "memory_priority": "critical",
                "memory_importance": persisted_importance,
            }),
            ..Default::default()
        };

        let docs = Arc::new(RwLock::new(vec![doc]));
        let qmd = Arc::new(QmdMemory::new(docs));
        let manager = MemoryManager::new(qmd, None);

        assert!(manager.relevance_scores.lock().expect("lock").is_empty());

        let _ = manager.decay_memories().await.expect("decay_memories");

        let scores = manager.relevance_scores.lock().expect("lock");
        let score = scores
            .get(&doc_id)
            .copied()
            .expect("score seeded for doc_id");
        assert!(
            (score - persisted_importance).abs() < 0.01,
            "expected seeded score close to persisted importance {}, got {}",
            persisted_importance,
            score
        );
    }
}
