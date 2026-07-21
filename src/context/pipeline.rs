//! Complete Context Regeneration Pipeline
//!
//! Ties together `Orchestrator`, `RegenerationLoop`, `ContextIndexer`,
//! `WorkingMemory`, and `ContextBuilder` to form a cohesive context
//! lifecycle and optimization engine.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::context::{
    ContextBuilder, ContextBuilderConfig, ContextDocument, ContextIndexer,
    Orchestrator, RegenDecision, RegenerationConfig, RegenerationLoop,
    ContextBudgetConfig,
};
use crate::memory::working::{MemoryItem, WorkingMemory};

/// Metrics representing context retrieval quality
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct RecallMetrics {
    /// Average Recall@K across all evaluation queries (0.0 to 1.0)
    pub recall_at_k: f64,
    /// Mean Reciprocal Rank (MRR) across all evaluation queries
    pub mrr: f64,
    /// Total number of queries evaluated
    pub total_queries: usize,
    /// Number of queries with at least one correct retrieval
    pub successful_retrievals: usize,
}

/// The main context regeneration pipeline.
pub struct ContextRegenerationPipeline {
    /// Orchestrator for planning and executing retrievals
    pub orchestrator: Arc<Orchestrator>,
    /// Loop for checking staleness and growth triggers
    pub regen_loop: RegenerationLoop,
    /// Shared document indexer
    pub indexer: Arc<RwLock<ContextIndexer>>,
    /// Shared working memory (hot cache)
    pub working_memory: Arc<RwLock<WorkingMemory>>,
    /// Context builder for formatting tiered context strings
    pub builder: ContextBuilder,
    /// Configuration for context budgets (mutable for tuning)
    pub budgets: Arc<RwLock<ContextBudgetConfig>>,
    /// Minimum allowed recall threshold (triggers alerts if violated)
    pub min_recall_threshold: f64,
}

impl ContextRegenerationPipeline {
    /// Create a new context regeneration pipeline
    pub fn new(
        orchestrator: Arc<Orchestrator>,
        working_memory_capacity: usize,
        min_recall_threshold: f64,
    ) -> Self {
        let budgets = ContextBudgetConfig::default();
        let regen_config = RegenerationConfig {
            stale_after_secs: 5,         // Short stale time for reactive pipeline tests
            growth_ratio_threshold: 0.1, // 10% growth triggers rebuild
            min_growth_tokens: 10,       // Sensitive growth limit for tests
            cooldown_secs: 1,            // Fast cooldown
            max_rebuilds_per_window: 100,
        };

        let wm_config = crate::memory::working::WorkingMemoryConfig {
            capacity: working_memory_capacity,
            lru_exempt_access_threshold: 2,
            bm25_k1: 1.5,
            bm25_b: 0.75,
        };

        Self {
            regen_loop: RegenerationLoop::with_config(regen_config).with_orchestrator(Arc::clone(&orchestrator)),
            orchestrator,
            indexer: Arc::new(RwLock::new(ContextIndexer::new())),
            working_memory: Arc::new(RwLock::new(WorkingMemory::with_config(wm_config))),
            builder: ContextBuilder::new(ContextBuilderConfig::default()),
            budgets: Arc::new(RwLock::new(budgets)),
            min_recall_threshold,
        }
    }

    /// Process a new message: index it, add to working memory, check triggers, and regenerate if needed
    pub async fn process_message(
        &self,
        session_id: &str,
        doc: ContextDocument,
    ) -> Result<RegenDecision, String> {
        let doc_tokens = doc.token_count;

        // 1. Index the document in-memory
        {
            let mut idx = self.indexer.write().await;
            idx.index_document(doc.clone());
        }

        // 2. Insert into volatile WorkingMemory
        {
            let mut wm = self.working_memory.write().await;
            let mut item = MemoryItem::new(doc.id.clone(), doc.content.clone());
            item.created_at = doc.created_at;
            item.metadata = Some(serde_json::json!({
                "session_id": session_id,
                "role": doc.role,
                "token_count": doc.token_count,
                "metadata": doc.metadata,
            }));
            wm.push(item);
        }

        // 3. Evaluate if we need regeneration based on session growth / staleness
        let decision = self.regen_loop.check(session_id, doc_tokens).await;

        match decision {
            RegenDecision::Stale { .. } | RegenDecision::Growth { .. } => {
                info!(
                    session_id = %session_id,
                    decision = ?decision,
                    "Triggering automated context regeneration"
                );
                self.regenerate_context(session_id).await?;
            }
            _ => {}
        }

        Ok(decision)
    }

    /// Forcefully rebuild/regenerate context for the given session ID
    pub async fn regenerate_context(&self, session_id: &str) -> Result<String, String> {
        let docs = {
            let idx = self.indexer.read().await;
            idx.all_documents()
        };

        // Filter session documents
        let session_docs: Vec<ContextDocument> = docs
            .into_iter()
            .filter(|d| d.session_id == session_id)
            .collect();

        // Create a prompt to trigger precompact execution plan
        let prompt = "regenerate context for active session";

        // Build the retrieval plan using current active budgets
        let active_budgets = *self.budgets.read().await;
        let temp_orchestrator = Orchestrator::with_budgets(active_budgets);
        let plan = temp_orchestrator.precompact(session_id, prompt, &session_docs).await;

        // Execute the plan
        let selected_docs = temp_orchestrator.execute(&plan, &session_docs, session_id).await;

        // Extract skills list
        let skills = Vec::new();

        // Build tiered and compressed context using ContextBuilder
        let final_context = self.builder.build(
            plan.level,
            &session_docs,
            &selected_docs,
            &skills,
        );

        // Update regeneration statistics in RegenerationLoop
        let _ = self.regen_loop.trigger_rebuild(session_id, &session_docs).await?;

        Ok(final_context)
    }

    /// Evaluate Recall@K and MRR metrics against a query-to-document ground truth mapping.
    /// It simulates context planning under current budgets and evaluates retrieval quality.
    pub async fn evaluate_recall(
        &self,
        session_id: &str,
        queries: &[String],
        ground_truth: &HashMap<String, Vec<String>>,
        k: usize,
    ) -> RecallMetrics {
        if queries.is_empty() {
            return RecallMetrics {
                recall_at_k: 0.0,
                mrr: 0.0,
                total_queries: 0,
                successful_retrievals: 0,
            };
        }

        let mut total_recall = 0.0;
        let mut total_mrr = 0.0;
        let mut successful_retrievals = 0;

        let indexer = self.indexer.read().await;
        let docs = indexer.all_documents();

        let active_budgets = *self.budgets.read().await;
        let temp_orchestrator = Orchestrator::with_budgets(active_budgets);

        for query in queries {
            let expected_ids = match ground_truth.get(query) {
                Some(ids) if !ids.is_empty() => ids,
                _ => continue,
            };

            // Retrieve selected document IDs under precompact plan simulating the current budget
            let plan = temp_orchestrator.precompact(session_id, query, &docs).await;
            let mut retrieved_ids = plan.selected_document_ids;

            // Bounded by evaluation parameter k
            retrieved_ids.truncate(k);

            // Calculate Recall@K
            let matched_count = expected_ids
                .iter()
                .filter(|id| retrieved_ids.contains(id))
                .count();
            let recall = matched_count as f64 / expected_ids.len() as f64;
            total_recall += recall;

            if matched_count > 0 {
                successful_retrievals += 1;
            }

            // Calculate Reciprocal Rank (MRR)
            let mut first_rank = 0.0;
            for (idx, id) in retrieved_ids.iter().enumerate() {
                if expected_ids.contains(id) {
                    first_rank = 1.0 / (idx + 1) as f64;
                    break;
                }
            }
            total_mrr += first_rank;
        }

        let total_queries = queries.len();
        let avg_recall = total_recall / total_queries as f64;
        let avg_mrr = total_mrr / total_queries as f64;

        let metrics = RecallMetrics {
            recall_at_k: avg_recall,
            mrr: avg_mrr,
            total_queries,
            successful_retrievals,
        };

        if avg_recall < self.min_recall_threshold {
            warn!(
                avg_recall = %avg_recall,
                threshold = %self.min_recall_threshold,
                "Context recall has fallen below the safety threshold!"
            );
        }

        metrics
    }

    /// Auto-tuning loop: adjusts budgets sequentially until the target recall is satisfied
    pub async fn auto_tune(
        &self,
        session_id: &str,
        queries: &[String],
        ground_truth: &HashMap<String, Vec<String>>,
        target_recall: f64,
    ) -> Result<bool, String> {
        let mut metrics = self.evaluate_recall(session_id, queries, ground_truth, 5).await;
        if metrics.recall_at_k >= target_recall {
            info!(
                recall = %metrics.recall_at_k,
                target = %target_recall,
                "Target recall already met. Tuning skipped."
            );
            return Ok(true);
        }

        let max_iterations = 5;
        let mut iteration = 0;

        while metrics.recall_at_k < target_recall && iteration < max_iterations {
            iteration += 1;
            info!(
                iteration = iteration,
                current_recall = %metrics.recall_at_k,
                target = %target_recall,
                "Auto-tuning context retrieval budgets..."
            );

            // Increment precompact and session_start document/token limits
            {
                let mut b = self.budgets.write().await;
                b.precompact_min_docs = b.precompact_min_docs.saturating_add(2);
                b.precompact_med_docs = b.precompact_med_docs.saturating_add(2);
                b.precompact_max_docs = b.precompact_max_docs.saturating_add(2);
                b.precompact_max_tokens = b.precompact_max_tokens.saturating_add(500);
                b.session_start_min_docs = b.session_start_min_docs.saturating_add(2);
                b.session_start_med_docs = b.session_start_med_docs.saturating_add(2);
                b.session_start_max_docs = b.session_start_max_docs.saturating_add(2);
                b.session_start_max_tokens = b.session_start_max_tokens.saturating_add(500);
            }

            // Re-evaluate
            metrics = self.evaluate_recall(session_id, queries, ground_truth, 5).await;
        }

        let success = metrics.recall_at_k >= target_recall;
        if success {
            info!(
                final_recall = %metrics.recall_at_k,
                "Auto-tuning succeeded in meeting the target recall."
            );
        } else {
            warn!(
                final_recall = %metrics.recall_at_k,
                "Auto-tuning finished without meeting target recall."
            );
        }

        Ok(success)
    }

    /// Extractive Episodic Dialog Summarization (ctx-episodic-real)
    /// Summarizes the dialogue logs to provide an compact summary of the interactions
    pub fn summarize_episodic(&self, documents: &[ContextDocument]) -> String {
        let mut summary_lines = Vec::new();
        summary_lines.push("=== Extractive Dialogue Episodic Summary ===".to_string());

        let mut sorted_docs = documents.to_vec();
        sorted_docs.sort_by_key(|d| d.created_at);

        let mut critical_decisions = Vec::new();
        let mut key_questions = Vec::new();

        for doc in &sorted_docs {
            let role_label = match doc.role.as_str() {
                "user" => "User",
                "assistant" => "Assistant",
                _ => "System",
            };

            for line in doc.content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let lower = trimmed.to_lowercase();
                if lower.contains("decision:") || lower.contains("decide") || lower.contains("architecture:") {
                    critical_decisions.push(format!("- [{}] Decision: {}", role_label, trimmed));
                } else if lower.contains("error:") || lower.contains("critical") || lower.contains("panic") {
                    critical_decisions.push(format!("- [{}] Incident: {}", role_label, trimmed));
                } else if trimmed.ends_with('?') || lower.contains("how to") || lower.contains("why") {
                    key_questions.push(format!("- [{}] Query: {}", role_label, trimmed));
                }
            }
        }

        if !critical_decisions.is_empty() {
            summary_lines.push("\n[Critical Decisions & Architectural Incidents]".to_string());
            summary_lines.extend(critical_decisions.into_iter().take(5));
        }

        if !key_questions.is_empty() {
            summary_lines.push("\n[Key Queries]".to_string());
            summary_lines.extend(key_questions.into_iter().take(5));
        }

        if sorted_docs.len() >= 2 {
            summary_lines.push("\n[Conversational Flow Preview]".to_string());
            let preview_docs = &sorted_docs[sorted_docs.len() - 2..];
            for doc in preview_docs {
                let preview: String = doc.content.chars().take(100).collect();
                summary_lines.push(format!("- {}: {}{}", doc.role, preview, if doc.content.len() > 100 { "..." } else { "" }));
            }
        }

        summary_lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pipeline_basic_flow() {
        let orchestrator = Arc::new(Orchestrator::default());
        let pipeline = ContextRegenerationPipeline::new(orchestrator, 10, 0.7);

        let doc1 = ContextDocument::new("1", "session-1", "user", "Hello pipeline");
        let expected_tokens = doc1.token_count;
        let decision = pipeline.process_message("session-1", doc1).await.unwrap();

        assert_eq!(decision, RegenDecision::Skip);

        let stats = pipeline.regen_loop.get_stats("session-1").await.unwrap();
        assert_eq!(stats.total_tokens_seen, expected_tokens);
    }

    #[tokio::test]
    async fn test_episodic_summarization_heuristics() {
        let orchestrator = Arc::new(Orchestrator::default());
        let pipeline = ContextRegenerationPipeline::new(orchestrator, 10, 0.7);

        let docs = vec![
            ContextDocument::new("1", "s-1", "user", "How to fix the memory leak?"),
            ContextDocument::new("2", "s-1", "assistant", "Decision: We will use a bounded FIFO cache to avoid memory leak."),
        ];

        let summary = pipeline.summarize_episodic(&docs);
        assert!(summary.contains("Extractive Dialogue Episodic Summary"));
        assert!(summary.contains("Decision:"));
        assert!(summary.contains("How to fix"));
    }
}
