//! Context builder for agent prompts
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::context::{ContextDocument, ContextLevel};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBuilderConfig {
    pub persona: String,
    pub rules: Vec<String>,
    pub goals: Vec<String>,
    pub constraints: Vec<String>,
    pub recent_messages_limit: usize,
    pub enable_compression: bool,
}

impl Default for ContextBuilderConfig {
    fn default() -> Self {
        Self {
            persona: "You are Xavier, a cognitive memory runtime for AI agents.".to_string(),
            rules: vec![],
            goals: vec![],
            constraints: vec![],
            recent_messages_limit: 5,
            enable_compression: true,
        }
    }
}

pub struct ContextBuilder {
    config: ContextBuilderConfig,
}

impl ContextBuilder {
    pub fn new(config: ContextBuilderConfig) -> Self {
        Self { config }
    }

    pub fn build(
        &self,
        level: ContextLevel,
        recent_messages: &[ContextDocument],
        memories: &[ContextDocument],
        skills: &[String],
    ) -> String {
        let mut context = String::new();

        // 1. System Prompt & Persona
        context.push_str("# System Prompt\n");
        context.push_str(&self.config.persona);
        context.push_str("\n\n");

        // 2. Rules
        if !self.config.rules.is_empty() {
            context.push_str("## Rules\n");
            for rule in &self.config.rules {
                context.push_str(&format!("- {}\n", rule));
            }
            context.push('\n');
        }

        match level {
            ContextLevel::Minimal => {
                // Shallow: core slots + episodic summary (last_preview) + virtual refs
                self.append_core_slots(&mut context);
                self.append_episodic_summary(&mut context, recent_messages, 200);
                self.append_virtual_refs(&mut context, memories, 5);
            }
            ContextLevel::Medium => {
                // Medium: core slots + episodic + recent messages + top 3 memories full
                self.append_core_slots(&mut context);
                self.append_episodic_summary(&mut context, recent_messages, 400);
                self.append_recent_messages(&mut context, recent_messages);
                self.append_memories_tiered(&mut context, memories, 3);
            }
            ContextLevel::Maximum => {
                // Deep: full retrieval + skills + full history
                self.append_memories_full(&mut context, memories);
                self.append_skills(&mut context, skills);
                self.append_recent_messages(&mut context, recent_messages);
            }
        }

        if self.config.enable_compression {
            self.compress_and_cross_reference(&mut context);
        }

        context
    }

    fn compress_and_cross_reference(&self, context: &mut String) {
        // Skip compression for small contexts
        if context.len() < 1000 {
            return;
        }
        // Simple "compression" by removing excessive whitespace and adding cross-refs
        // NOTE: This is a shallow compression (whitespace only).
        // Real savings come from progressive disclosure and budget-aware selection in Orchestrator.

        let keywords = [
            "error",
            "fix",
            "critical",
            "decision",
            "architecture",
            "goal",
            "ref",
            "summary",
        ];
        let lines: Vec<&str> = context.lines().collect();
        let mut compressed_lines = Vec::new();

        for i in 0..lines.len().min(10) {
            compressed_lines.push(lines[i]);
        }

        for i in 10..lines.len() {
            let line = lines[i];
            let lower = line.to_lowercase();
            if keywords.iter().any(|&k| lower.contains(k))
                || line.starts_with("#")
                || line.starts_with("[REF:")
                || line.starts_with("- ")
            {
                compressed_lines.push(line);
            }
        }

        let mut compressed = compressed_lines.join("\n");
        compressed = compressed.replace("  ", " ").replace("\n\n\n", "\n\n");

        if compressed.len() > 1500 {
            compressed.insert_str(0, "## CONTEXT_CHUNK_START:v1:extractive-compressed\n");
            compressed.push_str("\n## CONTEXT_CHUNK_END\n");
        }

        *context = compressed;
    }

    fn append_core_slots(&self, context: &mut String) {
        context.push_str("## Core Slots\n");
        context.push_str("- System Status: Active\n");
        context.push_str("- Context Mode: Progressive Disclosure\n");
        context.push('\n');
    }

    fn append_episodic_summary(
        &self,
        context: &mut String,
        messages: &[ContextDocument],
        max_chars: usize,
    ) {
        if messages.is_empty() {
            return;
        }
        context.push_str("## Episodic Summary (Last Preview)\n");

        let count = messages.len().min(2);
        let start = messages.len() - count;

        let mut preview = String::new();
        for msg in &messages[start..] {
            preview.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }

        let truncated: String = preview.chars().take(max_chars).collect();
        context.push_str(&truncated);
        if preview.len() > max_chars {
            context.push_str("...");
        }
        context.push_str("\n\n");
    }

    fn append_virtual_refs(
        &self,
        context: &mut String,
        memories: &[ContextDocument],
        limit: usize,
    ) {
        if memories.is_empty() {
            return;
        }
        context.push_str("## Virtual References (Available for page-in)\n");
        for mem in memories.iter().take(limit) {
            let path = mem.metadata["path"].as_str().unwrap_or(&mem.id);
            context.push_str(&format!("- [REF:{}] {}\n", mem.id, path));
        }
        if memories.len() > limit {
            context.push_str(&format!(
                "- ... and {} more references\n",
                memories.len() - limit
            ));
        }
        context.push('\n');
    }

    fn append_memories_tiered(
        &self,
        context: &mut String,
        memories: &[ContextDocument],
        full_limit: usize,
    ) {
        if memories.is_empty() {
            return;
        }

        context.push_str("# Relevant Memories & CodeGraph\n");
        for (i, mem) in memories.iter().enumerate() {
            let prefix = if mem.metadata["source"] == "code_graph" {
                "CODE"
            } else if mem.metadata["is_external"] == true {
                "MEM"
            } else {
                "DOC"
            };

            if i < full_limit {
                context.push_str(&format!("- [{}:{}] {}\n", prefix, mem.id, mem.content));
            } else {
                let path = mem.metadata["path"].as_str().unwrap_or(&mem.id);
                context.push_str(&format!(
                    "- [{}:{}] {} (Body virtualized)\n",
                    prefix, mem.id, path
                ));
            }
        }
        context.push('\n');
    }

    fn append_memories_full(&self, context: &mut String, memories: &[ContextDocument]) {
        if memories.is_empty() {
            return;
        }

        context.push_str("# Relevant Memories & CodeGraph\n");
        for mem in memories {
            let prefix = if mem.metadata["source"] == "code_graph" {
                "CODE"
            } else if mem.metadata["is_external"] == true {
                "MEM"
            } else {
                "DOC"
            };
            context.push_str(&format!("- [{}:{}] {}\n", prefix, mem.id, mem.content));
        }
        context.push('\n');
    }

    fn append_recent_messages(&self, context: &mut String, messages: &[ContextDocument]) {
        if messages.is_empty() {
            return;
        }

        context.push_str("# Recent Messages\n");
        let limit = self.config.recent_messages_limit.min(messages.len());
        let start = messages.len() - limit;

        for (i, msg) in messages[start..].iter().enumerate() {
            context.push_str(&format!("[REF:msg_{}] {}: {}\n", i, msg.role, msg.content));
        }
        context.push('\n');
    }

    fn append_skills(&self, context: &mut String, skills: &[String]) {
        if skills.is_empty() {
            return;
        }

        context.push_str("# Available Skills\n");
        for skill in skills {
            context.push_str(&format!("- {}\n", skill));
        }
        context.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_minimal_context() {
        let config = ContextBuilderConfig::default();
        let builder = ContextBuilder::new(config);
        let ctx = builder.build(ContextLevel::Minimal, &[], &[], &[]);

        assert!(ctx.contains("# System Prompt"));
        assert!(ctx.contains("## Core Slots"));
        assert!(ctx.contains("Context Mode: Progressive Disclosure"));
    }

    #[test]
    fn test_extractive_compression() {
        let config = ContextBuilderConfig::default();
        let builder = ContextBuilder::new(config);
        let mut ctx = "# System Prompt\nPersona info here.\n## Rules\nRule 1\nRule 2\nRule 3\nRule 4\nRule 5\nRule 6\nRule 7\nRule 8\nRule 9\nRule 10\n".to_string();
        ctx.push_str("Some boring content that should be removed during compression because it lacks keywords.\n");
        ctx.push_str("Decision: We should use extractive compression.\n");
        ctx.push_str("Critical Error: The system is too chatty.\n");

        while ctx.len() < 2000 {
            ctx.push_str("More filler text with Decision keyword to ensure it exceeds the 1500 char threshold for header.\n");
        }

        let mut test_ctx = ctx.clone();
        builder.compress_and_cross_reference(&mut test_ctx);

        assert!(test_ctx.contains("Decision:"));
        assert!(test_ctx.contains("Critical Error:"));
        assert!(!test_ctx.contains("Some boring content"));
        assert!(test_ctx.contains("CONTEXT_CHUNK_START:v1:extractive-compressed"));
    }
}
