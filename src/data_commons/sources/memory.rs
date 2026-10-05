//! Memory source: instruction/response pairs from the real memory store.
//!
//! Pairing strategy (deterministic, no LLM involved):
//! * response = the memory `content` (truncated to `max_content_chars`).
//! * instruction is synthesized from a label = `metadata.title`, else
//!   `metadata.summary` (first line, <= 160 chars), else the memory `path`:
//!   - decision      -> "What was decided about {label}, and why?"
//!   - file / symbol / repo / branch / harness -> "What does {label} do in this codebase?"
//!   - task          -> "What is the context and status of the task: {label}?"
//!   - procedural    -> "How do I {label}?"
//!   - anything else -> "What do you know about: {label}?"
//! * Skipped (counted in `excluded_other`): soft-deleted, encrypted, clearance above
//!   `max_clearance` (default Internal), no label, content shorter than `min_content_chars`.
//! * Records are sorted by memory id so the export is deterministic.

use super::{metadata_object, SourceBatch, SourceContext, TrainingSource};
use crate::memory::store::{MemoryRecord, MemoryStore};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use xavier_core_logic::ClearanceLevel;

pub struct MemorySource {
    store: Arc<dyn MemoryStore>,
    workspace_id: String,
    /// Allowed `metadata.kind` values (lowercase); empty = every kind.
    kinds: Vec<String>,
    /// Match `metadata.namespace.project|scope` (exact) when set.
    namespace: Option<String>,
    /// Match `metadata.domain` or an entry of `metadata.tags` when set.
    domain: Option<String>,
    pub min_content_chars: usize,
    pub max_content_chars: usize,
    pub max_clearance: ClearanceLevel,
}

impl MemorySource {
    pub fn new(store: Arc<dyn MemoryStore>, workspace_id: impl Into<String>) -> Self {
        Self {
            store,
            workspace_id: workspace_id.into(),
            kinds: Vec::new(),
            namespace: None,
            domain: None,
            min_content_chars: 20,
            max_content_chars: 4000,
            max_clearance: ClearanceLevel::Internal,
        }
    }

    pub fn with_kinds(mut self, kinds: Vec<String>) -> Self {
        self.kinds = kinds.into_iter().map(|k| k.to_ascii_lowercase()).collect();
        self
    }

    pub fn with_namespace(mut self, namespace: Option<String>) -> Self {
        self.namespace = namespace.filter(|n| !n.is_empty());
        self
    }

    pub fn with_domain(mut self, domain: Option<String>) -> Self {
        self.domain = domain.filter(|d| !d.is_empty());
        self
    }

    pub fn with_max_clearance(mut self, level: ClearanceLevel) -> Self {
        self.max_clearance = level;
        self
    }
}

fn meta_str<'a>(meta: &'a Value, key: &str) -> Option<&'a str> {
    meta.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn namespace_of(meta: &Value) -> Option<&str> {
    let ns = meta.get("namespace")?;
    ns.get("project")
        .and_then(Value::as_str)
        .or_else(|| ns.get("scope").and_then(Value::as_str))
        .filter(|s| !s.is_empty())
}

fn first_line_capped(text: &str, cap: usize) -> String {
    text.lines()
        .next()
        .unwrap_or("")
        .trim()
        .chars()
        .take(cap)
        .collect()
}

/// Synthesize the instruction for a memory (see module docs).
pub fn synthesize_instruction(record: &MemoryRecord) -> Option<String> {
    let meta = &record.metadata;
    let label = meta_str(meta, "title")
        .map(str::to_string)
        .or_else(|| meta_str(meta, "summary").map(|s| first_line_capped(s, 160)))
        .filter(|s| !s.is_empty())
        .or_else(|| Some(record.path.trim().to_string()).filter(|p| !p.is_empty()))?;
    let kind = meta_str(meta, "kind").unwrap_or("").to_ascii_lowercase();
    Some(match kind.as_str() {
        "decision" => format!("What was decided about {label}, and why?"),
        "file" | "symbol" | "repo" | "branch" | "harness" => {
            format!("What does {label} do in this codebase?")
        }
        "task" => format!("What is the context and status of the task: {label}?"),
        "procedural" => format!("How do I {label}?"),
        _ => format!("What do you know about: {label}?"),
    })
}

#[async_trait]
impl TrainingSource for MemorySource {
    fn name(&self) -> &'static str {
        "memory"
    }

    async fn collect(&self, _ctx: &SourceContext) -> Result<SourceBatch, String> {
        let mut all = self
            .store
            .list(&self.workspace_id)
            .await
            .map_err(|e| e.to_string())?;
        all.sort_by(|a, b| a.id.cmp(&b.id));

        let mut batch = SourceBatch {
            total_found: all.len(),
            ..SourceBatch::default()
        };

        for record in all {
            let meta = &record.metadata;
            let kind = meta_str(meta, "kind").unwrap_or("").to_ascii_lowercase();
            if !self.kinds.is_empty() && !self.kinds.contains(&kind) {
                continue; // filtered out by request, not an exclusion
            }
            if let Some(ns) = &self.namespace {
                if namespace_of(meta) != Some(ns.as_str()) {
                    continue;
                }
            }
            if let Some(domain) = &self.domain {
                let in_tags = meta
                    .get("tags")
                    .and_then(Value::as_array)
                    .map(|t| t.iter().any(|v| v.as_str() == Some(domain.as_str())))
                    .unwrap_or(false);
                if meta_str(meta, "domain") != Some(domain.as_str()) && !in_tags {
                    continue;
                }
            }
            if record.deleted_at.is_some() {
                batch.exclude("memory:deleted");
                continue;
            }
            if record.encrypted_dek.is_some() {
                batch.exclude("memory:encrypted");
                continue;
            }
            if record.clearance > self.max_clearance {
                batch.exclude("memory:clearance_above_max");
                continue;
            }
            let content = record.content.trim();
            if content.chars().count() < self.min_content_chars {
                batch.exclude("memory:content_too_short");
                continue;
            }
            let Some(instruction) = synthesize_instruction(&record) else {
                batch.exclude("memory:no_label");
                continue;
            };
            let response: String = content.chars().take(self.max_content_chars).collect();
            let mut metadata = metadata_object(&[
                ("source", Some("memory")),
                ("kind", Some(kind.as_str())),
                ("workspace", Some(record.workspace_id.as_str())),
                ("namespace", namespace_of(meta)),
                ("domain", meta_str(meta, "domain")),
            ]);
            metadata["memory_id"] = Value::String(record.id.clone());
            batch.records.push(serde_json::json!({
                "instruction": instruction,
                "response": response,
                "metadata": metadata,
            }));
        }
        Ok(batch)
    }
}
