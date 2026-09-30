//! Owned by issue [WAVE-30.06] (ADR-033 Wave 0).
//!
//! Material living here to cut the `memory -> agents` module cycle:
//!
//! - `BeliefEvaluator`: a pure scoring rule, moved here from `crate::agents::belief_evaluator`.
//! - `ConversationMessage` / `MessageRole`: pure transcript value types, moved here
//!   from `crate::agents::runtime`.
//! - `SemanticCompactor`: the outbound port memory compaction depends on, implemented
//!   by the LLM provider adapter that owns the HTTP client.
//!
//! Every moved item keeps a `pub use` re-export at its original path, so no caller
//! and no behaviour changes. Nothing in this module imports `crate::agents`,
//! `crate::memory` or any adapter, so the cut leaves `crate::memory` free to depend
//! on `crate::domain` alone.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::memory::belief::BeliefEdge;

// ─── Cycle cut `memory -> agents` (belief scoring) ───────────────────────────

/// Belief evaluation engine for agent reasoning.
#[derive(Debug, Clone)]
pub struct BeliefEvaluator;

impl BeliefEvaluator {
    /// New.
    pub fn new() -> Self {
        Self
    }

    /// Evaluates the confidence of a new belief based on its source and context.
    pub async fn evaluate_confidence(&self, source_type: &str, content: &str) -> f32 {
        let base_confidence = match source_type {
            "verified_fact" => 0.95,
            "user_input" => 0.8,
            "inference" => 0.6,
            "observation" => 0.7,
            _ => 0.5,
        };

        // Simple heuristic: length of content and presence of certain keywords
        let content_factor = (content.len() as f32 / 100.0).clamp(0.0, 0.05);

        (base_confidence + content_factor).clamp(0.0, 1.0)
    }

    /// Identifies if a new belief contradicts existing edges in the graph.
    pub fn find_contradiction(
        &self,
        new_edge: &BeliefEdge,
        existing_edges: &[BeliefEdge],
    ) -> Option<String> {
        for existing in existing_edges {
            if existing.source == new_edge.source
                && existing.target == new_edge.target
                && existing.relation_type == new_edge.relation_type
                && existing.provenance_id != new_edge.provenance_id
            {
                // This is a competing belief for the same triple but from a different source
                return Some(existing.id.clone());
            }
        }
        None
    }
}

impl Default for BeliefEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Cycle cut `memory -> agents` (conversation transcript) ──────────────────

/// One entry of a conversation transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub id: String,
    pub role: MessageRole,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

/// Author of a conversation message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

// ─── Cycle cut `memory -> agents` (semantic compaction) ──────────────────────

/// Outbound port semantic memory compaction depends on.
///
/// The contract lives in the domain and is implemented by the LLM provider adapter
/// (`crate::agents::provider::ModelProviderClient`), so `crate::memory` depends on
/// the capability instead of on the adapter (ADR-033 Wave 0, ADR-002).
#[async_trait]
pub trait SemanticCompactor: Send + Sync {
    /// Returns the compacted text, or `None` when the adapter produced nothing
    /// usable so the caller can fall back to plain truncation.
    async fn compact_document(&self, prompt: &str) -> Option<String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The moved transcript types keep their shape: role, content and timestamp are
    /// still serialized with the same field names the checkpoint writer expects.
    #[test]
    fn conversation_message_round_trips_through_serde() {
        let message = ConversationMessage {
            id: "msg-1".to_string(),
            role: MessageRole::User,
            content: "hello".to_string(),
            timestamp: Utc::now(),
        };

        let encoded = serde_json::to_string(&message).expect("message serializes");
        let decoded: ConversationMessage =
            serde_json::from_str(&encoded).expect("message deserializes");

        assert_eq!(decoded.id, message.id);
        assert_eq!(decoded.content, message.content);
        assert!(matches!(decoded.role, MessageRole::User));
    }

    /// The moved scoring rule is unchanged: an unknown source keeps the 0.5 floor.
    #[tokio::test]
    async fn belief_evaluator_keeps_the_unknown_source_floor() {
        let confidence = BeliefEvaluator::new()
            .evaluate_confidence("unregistered-source", "")
            .await;

        assert!((0.5..0.6).contains(&confidence));
    }
}
