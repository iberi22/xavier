//! Phase 4 consolidation layer.
//!
//! This module provides consolidation, decay, importance scoring, reflection,
//! and TGD (Textual Gradient Descent) integration on top of the existing memory store.

pub mod merger;
pub mod reflection;

use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::{
    tgd::{TgdEngine, consolidation::ProgressReport},
    memory::{
        manager::ManagedMemory,
        qmd_memory::MemoryDocument,
        schema::{EvidenceKind, MemoryKind, TypedMemoryPayload},
    },
    workspace::WorkspaceContext,
};

#[derive(Debug, Clone)]
pub struct ConsolidationTask {
    pub batch_size: usize,
    pub similarity_threshold: f32,
    pub decay_rate: f32,
    pub min_importance_for_decay: f32,
    pub reflection_batch_size: usize,
    pub reflection_age_days: i64,
    pub cleanup_similarity_threshold: f32,
    /// Whether to run TGD during consolidation (default: false)
    pub enable_tgd_in_consolidation: bool,
    /// Minimum number of new conversation turns since last TGD to trigger
    pub tgd_min_new_history: usize,
    /// Number of iterations for TGD refinement
    pub tgd_iterations: usize,
    /// Learning rate for TGD refinement
    pub tgd_learning_rate: f32,
    /// Quality threshold for TGD refinement
    pub tgd_refinement_threshold: f32,
    /// Batch size for TGD refinement (default: 5)
    pub tgd_refinement_batch_size: usize,
}

impl Default for ConsolidationTask {
    fn default() -> Self {
        Self {
            batch_size: 32,
            similarity_threshold: 0.88,
            decay_rate: 0.94,
            min_importance_for_decay: 0.30,
            reflection_batch_size: 8,
            reflection_age_days: 30,
            cleanup_similarity_threshold: 0.91,
            enable_tgd_in_consolidation: false,
            tgd_min_new_history: 20,
            tgd_iterations: 3,
            tgd_learning_rate: 0.1,
            tgd_refinement_threshold: 0.6,
            tgd_refinement_batch_size: 5,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ConsolidationStats {
    pub selected: usize,
    pub grouped: usize,
    pub merged_documents: usize,
    pub decayed_documents: usize,
    pub deleted_redundant_documents: usize,
    pub importance_updates: usize,
    pub memories_refined: usize,
    pub avg_score_improvement: f32,
    pub errors: usize,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ReflectionStats {
    pub selected: usize,
    pub summarized_documents: usize,
    pub summary_documents_created: usize,
    pub redundant_documents_removed: usize,
    pub llm_used: bool,
    pub errors: usize,
    pub duration_ms: u64,
}

impl ConsolidationTask {
    pub async fn consolidate(
        &self,
        workspace: &WorkspaceContext,
        progress: Option<Arc<RwLock<ProgressReport>>>,
    ) -> Result<ConsolidationStats> {
        let start = std::time::Instant::now();
        let mut stats = ConsolidationStats::default();
        let memories = workspace
            .workspace
            .memory_manager
            .get_all_memories()
            .await?;
        let mut selected = memories;
        selected.sort_by(|left, right| {
            right
                .quality
                .overall
                .partial_cmp(&left.quality.overall)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| right.access_count.cmp(&left.access_count))
                .then_with(|| left.doc.path.cmp(&right.doc.path))
        });
        let selected: Vec<ManagedMemory> = selected.into_iter().take(self.batch_size).collect();
        stats.selected = selected.len();

        if let Some(ref p) = progress {
            let mut p = p.write().await;
            p.total = stats.selected;
            p.processed = 0;
        }

        let memory = workspace.workspace.memory_manager.memory();
        let clusters = merger::cluster_similar_memories(&selected, self.similarity_threshold);
        stats.grouped = clusters.iter().filter(|cluster| cluster.len() > 1).count();

        let mut seen_ids = HashSet::new();
        let mut removed_ids = HashSet::new();
        let total_clusters = clusters.len();
        for (i, cluster) in clusters.into_iter().enumerate() {
            if let Some(ref p) = progress {
                let mut p = p.write().await;
                p.processed = (i * stats.selected) / total_clusters;
            }

            if cluster.len() < 2 {
                continue;
            }

            let mut docs = Vec::new();
            for managed in cluster {
                let maybe_id = managed.doc.id.clone();
                if let Some(id) = maybe_id {
                    if seen_ids.insert(id) {
                        docs.push(managed);
                    }
                }
            }

            if docs.len() < 2 {
                continue;
            }

            let merge = merger::merge_documents(&docs)?;
            if let Err(error) = memory.update(merge.canonical.clone()).await {
                warn!(%error, "failed to persist merged memory");
                stats.errors += 1;
                continue;
            }

            for redundant in merge.redundant_ids {
                removed_ids.insert(redundant.clone());
                if memory.delete(&redundant).await.is_ok() {
                    stats.deleted_redundant_documents += 1;
                } else {
                    stats.errors += 1;
                }
            }

            stats.merged_documents += docs.len().saturating_sub(1);
            stats.importance_updates += 1;
        }

        let mut decay_updates = 0usize;
        for managed in selected {
            let Some(doc_id) = managed.doc.id.clone() else {
                continue;
            };
            if removed_ids.contains(&doc_id) {
                continue;
            }

            let importance = merger::importance_score(
                managed.access_count,
                managed.last_access,
                managed.created_at,
                &managed.doc.metadata,
            );
            let decayed = merger::decay_importance(
                importance,
                managed.last_access,
                managed.created_at,
                self.decay_rate,
            );

            if decayed < self.min_importance_for_decay {
                if memory.delete(&doc_id).await.is_ok() {
                    stats.deleted_redundant_documents += 1;
                    decay_updates += 1;
                } else {
                    stats.errors += 1;
                }
                continue;
            }

            if (decayed - importance).abs() >= 0.01 {
                let mut updated = managed.doc.clone();
                if let Some(meta) = updated.metadata.as_object_mut() {
                    meta.insert("memory_importance".to_string(), serde_json::json!(decayed));
                    meta.insert("memory_decay_rate".to_string(), serde_json::json!(self.decay_rate));
                    meta.insert("memory_last_consolidated_at".to_string(),
                        serde_json::json!(Utc::now().to_rfc3339()));
                }
                if memory.update(updated).await.is_ok() {
                    decay_updates += 1;
                    stats.importance_updates += 1;
                } else {
                    stats.errors += 1;
                }
            }
        }
        stats.decayed_documents = decay_updates;

        stats.duration_ms = start.elapsed().as_millis() as u64;
        info!(
            processed = stats.selected,
            merged = stats.merged_documents,
            decayed = stats.decayed_documents,
            deleted = stats.deleted_redundant_documents,
            "memory consolidation complete"
        );
        Ok(stats)
    }

    /// If enabled and a TGD engine is provided, run TGD rule generation
    /// on any available conversation history. Does not block consolidation on failure.
    pub async fn run_tgd_if_enabled(
        &self,
        workspace: &WorkspaceContext,
        tgd_engine: Option<&TgdEngine>,
    ) -> Result<()> {
        if !self.enable_tgd_in_consolidation {
            return Ok(());
        }
        let Some(engine) = tgd_engine else {
            info!("⏭️ TGD in consolidation: skipped (no engine provided)");
            return Ok(());
        };

        let memories = workspace
            .workspace
            .memory_manager
            .get_all_memories()
            .await?;
        let recent_count = memories
            .iter()
            .filter(|m| {
                let age = merger::age_days(m.last_access, m.created_at);
                age < 1.0 // less than 1 day old
            })
            .count();

        if recent_count < self.tgd_min_new_history {
            info!(
                "⏭️ TGD in consolidation: skipped ({} recent memories < {} min)",
                recent_count, self.tgd_min_new_history
            );
            return Ok(());
        }

        info!(
            "🧠 TGD in consolidation: starting with {} recent memories",
            recent_count
        );

        // Build empty history/context to signal TGD to analyze recent memory deltas
        match engine.generate_rules(&[], &[]).await {
            Ok(rules) => {
                if rules.is_empty() {
                    info!("🧠 TGD in consolidation: no new rules generated");
                } else {
                    info!("🧠 TGD in consolidation: generated rules:\n{}", rules);
                }
            }
            Err(e) => {
                warn!("⚠️ TGD in consolidation: non-blocking error: {:#}", e);
            }
        }

        Ok(())
    }

    /// Performs iterative refinement of selected memories using TGD
    pub async fn run_tgd_memory_refinement(
        &self,
        workspace: &WorkspaceContext,
        tgd_engine: Option<&TgdEngine>,
    ) -> Result<ConsolidationStats> {
        let mut stats = ConsolidationStats::default();
        if !self.enable_tgd_in_consolidation {
            return Ok(stats);
        }

        let Some(engine) = tgd_engine else {
            return Ok(stats);
        };

        let memories = workspace
            .workspace
            .memory_manager
            .get_all_memories()
            .await?;

        // Selection: memories with low quality but high access, or simply low overall quality
        let mut candidates: Vec<ManagedMemory> = memories
            .into_iter()
            .filter(|m| {
                m.quality.overall < self.tgd_refinement_threshold && m.quality.overall > 0.1 // Not too low (trash) but needs improvement
            })
            .collect();

        candidates.sort_by(|a, b| {
            b.access_count.cmp(&a.access_count)
                .then_with(|| a.quality.overall.partial_cmp(&b.quality.overall).unwrap_or(std::cmp::Ordering::Equal))
        });

        // Limit to small batch per night to avoid high LLM costs
        let batch = candidates.into_iter().take(self.tgd_refinement_batch_size).collect::<Vec<_>>();
        stats.selected = batch.len();

        let mut total_improvement = 0.0;
        for managed in batch {
            let (refined_content, avg_score) = engine.refine_memory_content(&managed.doc.content, Some(self.tgd_iterations)).await?;

            if avg_score > managed.quality.overall {
                let mut updated = managed.doc.clone();
                updated.content = refined_content;
                if let Some(meta) = updated.metadata.as_object_mut() {
                    meta.insert("tgd_refined".to_string(), serde_json::json!(true));
                    meta.insert("tgd_refinement_score".to_string(), serde_json::json!(avg_score));
                    meta.insert("tgd_refined_at".to_string(), serde_json::json!(Utc::now().to_rfc3339()));
                }

                if workspace.workspace.memory_manager.memory().update(updated).await.is_ok() {
                    stats.memories_refined += 1;
                    total_improvement += avg_score - managed.quality.overall;
                    info!("🧠 TGD: Refined memory {}", managed.doc.id.as_deref().unwrap_or("unknown"));
                }
            }
        }

        if stats.memories_refined > 0 {
            stats.avg_score_improvement = total_improvement / stats.memories_refined as f32;
        }

        Ok(stats)
    }

    pub async fn reflect(&self, workspace: &WorkspaceContext) -> Result<ReflectionStats> {
        let start = std::time::Instant::now();
        let mut stats = ReflectionStats::default();
        let memories = workspace
            .workspace
            .memory_manager
            .get_all_memories()
            .await?;
        let mut candidates: Vec<ManagedMemory> = memories
            .into_iter()
            .filter(|memory| {
                let importance = merger::importance_score(
                    memory.access_count,
                    memory.last_access,
                    memory.created_at,
                    &memory.doc.metadata,
                );
                let age_days = merger::age_days(memory.last_access, memory.created_at);
                importance < 0.65 || age_days >= self.reflection_age_days as f32
            })
            .collect();

        candidates.sort_by(|left, right| {
            left.quality
                .overall
                .partial_cmp(&right.quality.overall)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| right.access_count.cmp(&left.access_count))
        });
        candidates.truncate(self.reflection_batch_size);
        stats.selected = candidates.len();
        if candidates.is_empty() {
            stats.duration_ms = start.elapsed().as_millis() as u64;
            return Ok(stats);
        }

        let docs: Vec<MemoryDocument> =
            candidates.iter().map(|memory| memory.doc.clone()).collect();
        let reflection = reflection::reflect_memories(&docs).await?;
        stats.llm_used = reflection.llm_used;
        stats.summarized_documents = docs.len();

        let summary_path = format!(
            "reflections/{}/{}",
            workspace.workspace_id,
            Utc::now().format("%Y%m%dT%H%M%SZ")
        );
        workspace
            .workspace
            .memory
            .add_document_typed(
                summary_path,
                reflection.summary.clone(),
                serde_json::json!({
                    "memory_priority": "high",
                    "memory_importance": 0.86,
                    "memory_reflection": true,
                    "reflection_sources": docs.iter().filter_map(|doc| doc.id.clone()).collect::<Vec<_>>(),
                    "reflection_themes": reflection.themes,
                    "reflection_notes": reflection.notes,
                    "reflection_generated_at": Utc::now().to_rfc3339(),
                }),
                Some(TypedMemoryPayload {
                    kind: Some(MemoryKind::Document),
                    evidence_kind: Some(EvidenceKind::SummaryFact),
                    namespace: None,
                    provenance: None,
                    ..Default::default()
                }),
            )
            .await?;
        stats.summary_documents_created = 1;

        for candidate in candidates {
            let Some(candidate_id) = candidate.doc.id.as_ref() else {
                continue;
            };
            let should_remove = reflection
                .cleanup_targets
                .iter()
                .any(|target| target == candidate_id)
                || merger::similarity_to_summary(&candidate.doc.content, &reflection.summary)
                    >= self.cleanup_similarity_threshold;
            if should_remove
                && workspace
                    .workspace
                    .memory
                    .delete(candidate_id)
                    .await
                    .is_ok()
            {
                stats.redundant_documents_removed += 1;
            }
        }

        stats.duration_ms = start.elapsed().as_millis() as u64;
        info!(
            selected = stats.selected,
            removed = stats.redundant_documents_removed,
            "memory reflection complete"
        );
        Ok(stats)
    }
}
