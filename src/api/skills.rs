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

/// Dispatch skill.
pub async fn dispatch_skill(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(request): Json<DispatchRequest>,
) -> impl IntoResponse {
    use crate::context::skill_dispatcher::SkillDispatchRequest;
    use crate::context::skill_registry::SkillRegistry;
    use crate::context::SkillDispatcher;

    let workspace_root = std::path::PathBuf::from(&workspace.workspace_id);
    let registry = SkillRegistry::build_skill_registry(&workspace_root).await;
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

/// Memory health.
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

/// List skills.
pub async fn list_skills(Extension(workspace): Extension<WorkspaceContext>) -> impl IntoResponse {
    use crate::context::skill_registry::SkillRegistry;

    let workspace_root = std::path::PathBuf::from(&workspace.workspace_id);
    let registry = SkillRegistry::build_skill_registry(&workspace_root).await;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::RuntimeConfig;
    use crate::memory::store::MemoryBackend;
    use crate::settings::types::DedupSettings;
    use crate::workspace::{
        EmbeddingProviderMode, PlanTier, SyncPolicy, WorkspaceConfig, WorkspaceState,
    };
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::{get, post},
        Router,
    };
    use std::sync::Arc;
    use tower::ServiceExt;

    /// Saves the previous `HOME` and `XAVIER_SKILL_STORE` values and restores both
    /// on drop, so a failing assertion unwinds without leaking either override into
    /// sibling tests of this binary.
    struct EnvGuard {
        home: Option<String>,
        skill_store: Option<String>,
    }

    impl EnvGuard {
        /// Point `HOME` at the test tempdir and `XAVIER_SKILL_STORE` at an empty
        /// store, so the registry never scans the developer's real
        /// `~/.hermes/skills` nor reads their `~/.hermes/config.yaml`.
        fn isolate(temp_dir: &std::path::Path) -> Self {
            let guard = Self {
                home: std::env::var("HOME").ok(),
                skill_store: std::env::var("XAVIER_SKILL_STORE").ok(),
            };
            let empty_store = temp_dir.join("empty_store");
            if !empty_store.exists() {
                std::fs::create_dir_all(&empty_store)
                    .expect("empty skill store should be creatable");
            }
            std::env::set_var("HOME", temp_dir);
            std::env::set_var("XAVIER_SKILL_STORE", &empty_store);
            guard
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, previous) in [
                ("HOME", self.home.take()),
                ("XAVIER_SKILL_STORE", self.skill_store.take()),
            ] {
                match previous {
                    Some(val) => std::env::set_var(key, val),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    /// Build a fully explicit workspace config with a literal synthetic token:
    /// tests must never read `XAVIER_TOKEN` from the environment, which CI does not set.
    fn test_workspace_config() -> WorkspaceConfig {
        WorkspaceConfig {
            id: "test-skills-api".to_string(),
            token: "test-token".to_string(),
            plan: PlanTier::Personal,
            memory_backend: MemoryBackend::Memory,
            storage_limit_bytes: Some(10 * 1024 * 1024),
            request_limit: Some(1000),
            request_unit_limit: Some(1000),
            embedding_provider_mode: EmbeddingProviderMode::BringYourOwn,
            managed_google_embeddings: false,
            sync_policy: SyncPolicy::LocalOnly,
            protocol: Default::default(),
            dedup: DedupSettings::default(),
        }
    }

    async fn setup_test_workspace(temp_dir: &std::path::Path) -> WorkspaceContext {
        let skills_dir = temp_dir.join("skills").join("test-skill");
        tokio::fs::create_dir_all(&skills_dir).await.unwrap();
        let skill_file = skills_dir.join("SKILL.md");
        let content = r#"---
name: test-skill
description: A helpful test skill for memory testing
---
# Test Skill Instructions
Follow these steps to analyze memory and debug issues.
"#;
        tokio::fs::write(&skill_file, content).await.unwrap();

        let config = test_workspace_config();
        let runtime = RuntimeConfig::from_env();
        let state = Arc::new(
            WorkspaceState::new(config, runtime, temp_dir.to_path_buf())
                .await
                .unwrap(),
        );

        WorkspaceContext {
            workspace_id: temp_dir.to_string_lossy().to_string(),
            workspace: state,
        }
    }

    #[tokio::test]
    async fn test_list_skills_retains_json_fields() {
        // Same lock as the registry tests: both mutate XAVIER_SKILL_STORE in one test binary.
        let _lock = crate::context::skill_registry::env_lock().lock().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let _env_guard = EnvGuard::isolate(temp_dir.path());

        let workspace_ctx = setup_test_workspace(temp_dir.path()).await;
        let app = Router::new()
            .route("/api/skill/list", get(list_skills))
            .layer(Extension(workspace_ctx));

        let req = Request::builder()
            .uri("/api/skill/list")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(json["ok"], true);
        assert_eq!(json["count"], 1);
        let skills = json["skills"].as_array().unwrap();
        assert_eq!(skills.len(), 1);
        let skill = &skills[0];
        assert_eq!(skill["name"], "test-skill");
        assert_eq!(
            skill["description"],
            "A helpful test skill for memory testing"
        );
        assert!(skill["domains"].is_array());
        assert!(skill["token_cost"].is_number());
    }

    #[tokio::test]
    async fn test_dispatch_skill_matched_retains_json_fields() {
        let _lock = crate::context::skill_registry::env_lock().lock().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let _env_guard = EnvGuard::isolate(temp_dir.path());

        let workspace_ctx = setup_test_workspace(temp_dir.path()).await;
        let app = Router::new()
            .route("/api/skill/dispatch", post(dispatch_skill))
            .layer(Extension(workspace_ctx));

        let payload = serde_json::json!({
            "task": "A helpful test skill for memory testing",
            "max_tokens": 4000
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/skill/dispatch")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(json["ok"], true);
        let data = &json["data"];
        assert_eq!(data["skill_name"], "test-skill");
        assert_eq!(
            data["skill_description"],
            "A helpful test skill for memory testing"
        );
        assert!(data["confidence"].as_f64().unwrap() >= 0.40);
        assert!(data["estimated_savings_pct"].is_number());

        let context_pack = &data["context_pack"];
        assert!(context_pack["system_instructions"].is_string());
        assert!(context_pack["relevant_memories"].is_array());
        assert!(context_pack["prior_decisions"].is_array());
        assert!(context_pack["total_tokens"].is_number());
    }

    #[tokio::test]
    async fn test_dispatch_skill_unmatched_retains_json_fields() {
        let _lock = crate::context::skill_registry::env_lock().lock().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let _env_guard = EnvGuard::isolate(temp_dir.path());

        let workspace_ctx = setup_test_workspace(temp_dir.path()).await;
        let app = Router::new()
            .route("/api/skill/dispatch", post(dispatch_skill))
            .layer(Extension(workspace_ctx));

        let payload = serde_json::json!({
            "task": "completely unrelated cooking recipe for pasta",
            "max_tokens": 4000
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/skill/dispatch")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(json["ok"], true);
        let data = &json["data"];
        assert_eq!(data["skill_name"], "_none");
        assert_eq!(data["skill_description"], "No matching skill found");
        assert_eq!(data["confidence"], 0.0);
        assert_eq!(data["estimated_savings_pct"], 0.0);

        let context_pack = &data["context_pack"];
        assert_eq!(context_pack["system_instructions"], "");
        assert!(context_pack["relevant_memories"].is_array());
        assert!(context_pack["prior_decisions"].is_array());
        assert_eq!(context_pack["total_tokens"], 0);
    }
}
