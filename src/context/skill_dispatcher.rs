//! Skill Dispatcher — Routes tasks to skills and builds pre-digested context packs
//!
//! This is the core of Xavier's "masticación" capability. Instead of IDEs or CLI
//! agents deciding which skill to use and what context to inject, the dispatcher:
//! 1. Classifies the incoming task
//! 2. Matches it to the best skill from the registry
//! 3. Gathers relevant memories using the existing retrieval pipeline
//! 4. Compacts everything into a minimal-token ContextPack
//!
//! The result is a payload ready for direct LLM consumption with minimal waste.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

use super::skill_registry::SkillRegistry;
use crate::memory::qmd_memory::QmdMemory;
use crate::memory::virtual_memory::{MemoryReference, VirtualMemory};

/// Request from an agent/IDE to dispatch a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDispatchRequest {
    /// The task description (e.g. "Analyze the failed traces from bot X")
    pub task: String,
    /// Optional model hint for budget estimation (e.g. "claude-opus-4")
    pub model_hint: Option<String>,
    /// Maximum tokens the agent can afford for this context injection
    pub max_tokens: Option<usize>,
    /// Optional project filter for memory retrieval
    pub project: Option<String>,
}

/// The pre-digested result ready for LLM consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDispatchResult {
    /// The matched skill
    pub skill_name: String,
    /// Skill description for the agent
    pub skill_description: String,
    /// Match confidence (0.0 - 1.0)
    pub confidence: f32,
    /// The pre-digested context pack
    pub context_pack: ContextPack,
    /// How many tokens were saved vs. sending everything raw
    pub estimated_savings_pct: f32,
}

/// A minimal-token payload containing everything the LLM needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPack {
    /// The skill's instructions (potentially compacted to fit budget)
    pub system_instructions: String,
    /// Relevant memory references (summaries, not full content)
    pub relevant_memories: Vec<MemoryReference>,
    /// Prior decisions from Xavier's decision memory
    pub prior_decisions: Vec<String>,
    /// Total estimated token count of this pack
    pub total_tokens: usize,
}

/// The dispatcher that ties the registry, memory, and retrieval together.
pub struct SkillDispatcher {
    registry: SkillRegistry,
    memory: Option<Arc<QmdMemory>>,
}

impl SkillDispatcher {
    /// Create a new dispatcher with a skill registry and optional memory backend.
    pub fn new(registry: SkillRegistry, memory: Option<Arc<QmdMemory>>) -> Self {
        Self { registry, memory }
    }

    /// Dispatch a task: find the best skill, gather context, build a pack.
    pub async fn dispatch(&self, request: &SkillDispatchRequest) -> Result<SkillDispatchResult> {
        let max_tokens = request.max_tokens.unwrap_or(4000);

        // 1. Find the best matching skill
        let matches = self.registry.search(&request.task, 3);

        let (confidence, skill) = if let Some((score, skill)) = matches.first() {
            (*score, (*skill).clone())
        } else {
            // No skill matched — return a generic "no-skill" result
            return Ok(SkillDispatchResult {
                skill_name: "_none".to_string(),
                skill_description: "No matching skill found".to_string(),
                confidence: 0.0,
                context_pack: ContextPack {
                    system_instructions: String::new(),
                    relevant_memories: Vec::new(),
                    prior_decisions: Vec::new(),
                    total_tokens: 0,
                },
                estimated_savings_pct: 0.0,
            });
        };

        // 2. Budget allocation: skill gets 40%, memories get 50%, decisions get 10%
        let skill_budget = (max_tokens as f32 * 0.40) as usize;
        let memory_budget = (max_tokens as f32 * 0.50) as usize;
        let _decision_budget = max_tokens.saturating_sub(skill_budget + memory_budget);

        // 3. Compact the skill content to fit budget
        let system_instructions = skill.compacted_content(skill_budget);

        // 4. Gather relevant memories
        let (relevant_memories, prior_decisions) = if let Some(memory) = &self.memory {
            self.gather_context(
                memory,
                &request.task,
                memory_budget,
                request.project.as_deref(),
            )
            .await
        } else {
            (Vec::new(), Vec::new())
        };

        // 5. Calculate total tokens
        let instructions_tokens = system_instructions.split_whitespace().count();
        let memory_tokens: usize = relevant_memories
            .iter()
            .map(|m| m.summary.split_whitespace().count() + m.keywords.len())
            .sum();
        let decision_tokens: usize = prior_decisions
            .iter()
            .map(|d| d.split_whitespace().count())
            .sum();
        let total_tokens = instructions_tokens + memory_tokens + decision_tokens;

        // 6. Estimate savings
        let raw_tokens = skill.token_cost
            + relevant_memories.len() * 500 // Avg full doc ~500 tokens
            + prior_decisions.len() * 100;
        let estimated_savings_pct = if raw_tokens > 0 {
            ((raw_tokens.saturating_sub(total_tokens) as f32) / raw_tokens as f32) * 100.0
        } else {
            0.0
        };

        info!(
            skill = skill.name,
            confidence,
            total_tokens,
            savings_pct = estimated_savings_pct,
            "Skill dispatched"
        );

        Ok(SkillDispatchResult {
            skill_name: skill.name,
            skill_description: skill.description,
            confidence,
            context_pack: ContextPack {
                system_instructions,
                relevant_memories,
                prior_decisions,
                total_tokens,
            },
            estimated_savings_pct,
        })
    }

    /// Gather relevant memories and prior decisions for the task.
    async fn gather_context(
        &self,
        memory: &Arc<QmdMemory>,
        query: &str,
        max_memory_tokens: usize,
        project: Option<&str>,
    ) -> (Vec<MemoryReference>, Vec<String>) {
        let vm = VirtualMemory::new(Arc::clone(memory), None);
        let max_entries = (max_memory_tokens / 80).clamp(3, 15); // ~80 tokens per reference

        // Build filters if project is specified
        let filters = project.map(|p| crate::memory::schema::MemoryQueryFilters {
            project: Some(p.to_string()),
            ..Default::default()
        });

        let entries = vm
            .page_in_filtered(query, max_entries, filters.as_ref())
            .await
            .unwrap_or_default();

        let mut references: Vec<MemoryReference> =
            entries.iter().map(|e| e.to_reference()).collect();

        // Trim to token budget
        let mut total_tokens = 0;
        references.retain(|r| {
            let ref_tokens = r.summary.split_whitespace().count() + r.keywords.len();
            if total_tokens + ref_tokens <= max_memory_tokens {
                total_tokens += ref_tokens;
                true
            } else {
                false
            }
        });

        // Extract prior decisions from decision-kind memories
        let decision_filters = crate::memory::schema::MemoryQueryFilters {
            kinds: Some(vec![crate::memory::schema::MemoryKind::Decision]),
            ..Default::default()
        };
        let decisions = memory
            .search_filtered(query, 5, Some(&decision_filters))
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|doc| {
                // Use just the first line or 100 chars as a compact decision reference
                doc.content
                    .lines()
                    .next()
                    .unwrap_or(&doc.content)
                    .chars()
                    .take(150)
                    .collect::<String>()
            })
            .collect();

        (references, decisions)
    }

    /// Get a reference to the skill registry.
    pub fn registry(&self) -> &SkillRegistry {
        &self.registry
    }

    /// Get a mutable reference to the skill registry (for reindexing).
    pub fn registry_mut(&mut self) -> &mut SkillRegistry {
        &mut self.registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_pack_serializes() {
        let pack = ContextPack {
            system_instructions: "Do the thing".to_string(),
            relevant_memories: vec![MemoryReference {
                id: "m1".to_string(),
                path: "test/path".to_string(),
                summary: "A test memory".to_string(),
                keywords: vec!["test".to_string()],
            }],
            prior_decisions: vec!["Use clustering".to_string()],
            total_tokens: 42,
        };

        let json = serde_json::to_string(&pack).unwrap();
        assert!(json.contains("Do the thing"));
        assert!(json.contains("test memory"));
    }

    #[test]
    fn dispatch_result_serializes() {
        let result = SkillDispatchResult {
            skill_name: "test-skill".to_string(),
            skill_description: "A test".to_string(),
            confidence: 0.85,
            context_pack: ContextPack {
                system_instructions: "Test".to_string(),
                relevant_memories: Vec::new(),
                prior_decisions: Vec::new(),
                total_tokens: 1,
            },
            estimated_savings_pct: 73.2,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test-skill"));
        assert!(json.contains("73.2"));
    }
}
