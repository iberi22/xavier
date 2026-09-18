//! Context builder for agent prompts
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::context::{ContextDocument, ContextLevel};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

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
    /// New.
    pub fn new(config: ContextBuilderConfig) -> Self {
        Self { config }
    }

    /// Build.
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
                let _ = write!(context, "- {}\n", rule);
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

        for &line in lines.iter().take(lines.len().min(10)) {
            compressed_lines.push(line);
        }

        for &line in lines.iter().skip(10) {
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
            let _ = write!(preview, "{}: {}\n", msg.role, msg.content);
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
            let _ = write!(context, "- [REF:{}] {}\n", mem.id, path);
        }
        if memories.len() > limit {
            let _ = write!(
                context,
                "- ... and {} more references\n",
                memories.len() - limit
            );
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
                let _ = write!(context, "- [{}:{}] {}\n", prefix, mem.id, mem.content);
            } else {
                let path = mem.metadata["path"].as_str().unwrap_or(&mem.id);
                let _ = write!(
                    context,
                    "- [{}:{}] {} (Body virtualized)\n",
                    prefix, mem.id, path
                );
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
            let _ = write!(context, "- [{}:{}] {}\n", prefix, mem.id, mem.content);
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
            let _ = write!(context, "[REF:msg_{}] {}: {}\n", i, msg.role, msg.content);
        }
        context.push('\n');
    }

    fn append_skills(&self, context: &mut String, skills: &[String]) {
        if skills.is_empty() {
            return;
        }

        context.push_str("# Available Skills\n");
        for skill in skills {
            let _ = write!(context, "- {}\n", skill);
        }
        context.push('\n');
    }

    /// Filter documents matching the given project_id with hierarchical path fallback.
    pub fn get_project_context(
        &self,
        documents: &[ContextDocument],
        project_id: &str,
    ) -> Vec<ContextDocument> {
        get_project_context(documents, project_id)
    }
}

/// Helper to determine if a document matches the project_id.
/// Checks `metadata.namespace.project == project_id` first.
/// If missing, falls back to hierarchical path matching:
/// - `doc.path.starts_with(&format!("gitcore/{}/", project_id))`
/// - `doc.path.starts_with(&format!("workspaces/{}/", project_id))`
/// - `doc.path.contains(&format!("/{}/", project_id))`
pub fn matches_project_context(doc: &ContextDocument, project_id: &str) -> bool {
    let meta_project = doc
        .metadata
        .get("namespace")
        .and_then(|ns| ns.get("project"))
        .and_then(|p| p.as_str())
        .or_else(|| doc.metadata.get("project").and_then(|p| p.as_str()));

    if let Some(proj) = meta_project {
        return proj == project_id;
    }

    let path = if !doc.path.is_empty() {
        doc.path.as_str()
    } else {
        doc.metadata
            .get("path")
            .and_then(|p| p.as_str())
            .unwrap_or("")
    };

    path.starts_with(&format!("gitcore/{}/", project_id))
        || path.starts_with(&format!("workspaces/{}/", project_id))
        || path.contains(&format!("/{}/", project_id))
}

/// Filter documents matching the given project_id, taking advantage of
/// hierarchical path fallback if metadata is not explicitly populated.
pub fn get_project_context(
    documents: &[ContextDocument],
    project_id: &str,
) -> Vec<ContextDocument> {
    documents
        .iter()
        .filter(|doc| matches_project_context(doc, project_id))
        .cloned()
        .collect()
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

    #[test]
    fn test_hierarchical_project_context_matching() {
        let mut doc1 = ContextDocument::new("1", "s1", "doc", "content");
        doc1.path = "gitcore/xavier/README.md".to_string();
        assert!(matches_project_context(&doc1, "xavier"));

        let mut doc2 = ContextDocument::new("2", "s1", "doc", "content");
        doc2.path = "workspaces/xavier/src/main.rs".to_string();
        assert!(matches_project_context(&doc2, "xavier"));

        let mut doc3 = ContextDocument::new("3", "s1", "doc", "content");
        doc3.path = "other/sub/xavier/file.txt".to_string();
        assert!(matches_project_context(&doc3, "xavier"));

        let mut doc4 = ContextDocument::new("4", "s1", "doc", "content");
        doc4.path = "gitcore/other/README.md".to_string();
        assert!(!matches_project_context(&doc4, "xavier"));

        // Explicit metadata matches
        let mut doc_meta = ContextDocument::new("5", "s1", "doc", "content");
        doc_meta.metadata = serde_json::json!({
            "namespace": { "project": "xavier" }
        });
        doc_meta.path = "some/unrelated/path.md".to_string();
        assert!(matches_project_context(&doc_meta, "xavier"));

        // Explicit metadata mismatch overrides path
        let mut doc_meta_diff = ContextDocument::new("6", "s1", "doc", "content");
        doc_meta_diff.metadata = serde_json::json!({
            "namespace": { "project": "other" }
        });
        doc_meta_diff.path = "gitcore/xavier/README.md".to_string();
        assert!(!matches_project_context(&doc_meta_diff, "xavier"));
    }

    #[test]
    fn test_get_project_context() {
        let mut doc1 = ContextDocument::new("1", "s1", "doc", "c1");
        doc1.path = "gitcore/xavier/README.md".to_string();
        let mut doc2 = ContextDocument::new("2", "s1", "doc", "c2");
        doc2.path = "gitcore/other/README.md".to_string();

        let filtered = get_project_context(&[doc1, doc2], "xavier");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "1");
    }
}
