// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Skills API endpoints
//!
//! Provides HTTP endpoints for skill dispatch and memory health monitoring.
//! These endpoints allow external agents (IDEs, CLI tools, bots) to leverage
//! Xavier's skill orchestration and cognitive maintenance capabilities.

use axum::{extract::Extension, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

use crate::workspace::WorkspaceContext;

// ---------------------------------------------------------------------------
// POST /api/skill/dispatch
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct DispatchRequest {
    /// The task description
    pub task: String,
    /// Optional model hint for budget estimation (e.g. "claude-opus-4")
    #[serde(default)]
    pub model_hint: Option<String>,
    /// Maximum tokens the agent can afford
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
    /// Optional project filter for memory retrieval
    #[serde(default)]
    pub project: Option<String>,
}

fn default_max_tokens() -> usize {
    4000
}

#[derive(Debug, Serialize)]
pub struct DispatchResponse {
    pub skill_name: String,
    pub skill_description: String,
    pub confidence: f32,
    pub context_pack: ContextPackResponse,
    pub estimated_savings_pct: f32,
}

#[derive(Debug, Serialize)]
pub struct ContextPackResponse {
    pub system_instructions: String,
    pub relevant_memories: Vec<MemoryRefResponse>,
    pub prior_decisions: Vec<String>,
    pub total_tokens: usize,
}

#[derive(Debug, Serialize)]
pub struct MemoryRefResponse {
    pub id: String,
    pub path: String,
    pub summary: String,
    pub keywords: Vec<String>,
}

pub async fn dispatch_skill(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(request): Json<DispatchRequest>,
) -> impl IntoResponse {
    use crate::context::skill_dispatcher::SkillDispatchRequest;
    use crate::context::skill_registry::SkillRegistry;
    use crate::context::SkillDispatcher;

    let workspace_root = std::path::PathBuf::from(&workspace.workspace_id);
    let mut registry = SkillRegistry::with_defaults(&workspace_root);
    let _ = registry.reindex().await;
    let memory = workspace.workspace.memory.clone();
    let dispatcher = SkillDispatcher::new(registry, Some(memory));

    let dispatch_request = SkillDispatchRequest {
        task: request.task,
        model_hint: request.model_hint,
        max_tokens: Some(request.max_tokens),
        project: request.project,
    };

    match dispatcher.dispatch(&dispatch_request).await {
        Ok(result) => {
            let response = DispatchResponse {
                skill_name: result.skill_name,
                skill_description: result.skill_description,
                confidence: result.confidence,
                context_pack: ContextPackResponse {
                    system_instructions: result.context_pack.system_instructions,
                    relevant_memories: result
                        .context_pack
                        .relevant_memories
                        .into_iter()
                        .map(|m| MemoryRefResponse {
                            id: m.id,
                            path: m.path,
                            summary: m.summary,
                            keywords: m.keywords,
                        })
                        .collect(),
                    prior_decisions: result.context_pack.prior_decisions,
                    total_tokens: result.context_pack.total_tokens,
                },
                estimated_savings_pct: result.estimated_savings_pct,
            };
            Json(serde_json::json!({ "ok": true, "data": response }))
        }
        Err(e) => Json(serde_json::json!({
            "ok": false,
            "error": format!("{}", e)
        })),
    }
}

// ---------------------------------------------------------------------------
// GET /api/memory/health
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct MemoryHealthResponse {
    pub total_documents: usize,
    pub total_size_bytes: u64,
    pub by_priority: std::collections::HashMap<String, usize>,
    pub low_quality_count: usize,
    pub ephemeral_count: usize,
    pub decayed_count: usize,
}

pub async fn memory_health(Extension(workspace): Extension<WorkspaceContext>) -> impl IntoResponse {
    let memory = &workspace.workspace.memory;
    let docs = memory.all_documents().await;
    let total_documents = docs.len();
    let total_size_bytes: u64 = docs.iter().map(|d| d.estimated_bytes()).sum();

    use crate::memory::manager::MemoryPriority;
    let mut by_priority = std::collections::HashMap::new();
    let mut low_quality_count = 0;
    let mut ephemeral_count = 0;

    for doc in &docs {
        let priority = MemoryPriority::from_metadata(&doc.metadata);
        *by_priority
            .entry(priority.as_str().to_string())
            .or_insert(0) += 1;

        if priority == MemoryPriority::Ephemeral {
            ephemeral_count += 1;
        }
        if doc.content.trim().len() < 10 {
            low_quality_count += 1;
        }
    }

    let response = MemoryHealthResponse {
        total_documents,
        total_size_bytes,
        by_priority,
        low_quality_count,
        ephemeral_count,
        decayed_count: 0, // Would need MemoryManager access for accurate count
    };

    Json(serde_json::json!({ "ok": true, "data": response }))
}

// ---------------------------------------------------------------------------
// GET /api/skill/list
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct SkillListEntry {
    pub name: String,
    pub description: String,
    pub domains: Vec<String>,
    pub token_cost: usize,
}

pub async fn list_skills(Extension(workspace): Extension<WorkspaceContext>) -> impl IntoResponse {
    use crate::context::skill_registry::SkillRegistry;

    let workspace_root = std::path::PathBuf::from(&workspace.workspace_id);
    let mut registry = SkillRegistry::with_defaults(&workspace_root);
    let _ = registry.reindex().await;

    let skills: Vec<SkillListEntry> = registry
        .list()
        .into_iter()
        .filter_map(|name| {
            registry.get(name).map(|s| SkillListEntry {
                name: s.name.clone(),
                description: s.description.clone(),
                domains: s.domains.clone(),
                token_cost: s.token_cost,
            })
        })
        .collect();

    Json(serde_json::json!({
        "ok": true,
        "count": skills.len(),
        "skills": skills
    }))
}
