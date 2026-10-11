//! Skills API endpoints
//!
//! Provides HTTP endpoints for skill dispatch and memory health monitoring.
//! These endpoints allow external agents (IDEs, CLI tools, bots) to leverage
//! Xavier's skill orchestration and cognitive maintenance capabilities.

use axum::{extract::Extension, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

use crate::workspace::WorkspaceContext;

use crate::context::skill_controller::audit::{
    audit_packages, AuditReport, AuditedPackage, Severity,
};
use crate::context::{SkillFsPolicy, SkillManifest, SkillResolvedRoots};

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

// ---------------------------------------------------------------------------
// GET /api/skill/audit
// ---------------------------------------------------------------------------

pub fn audit_report_json(report: &AuditReport) -> serde_json::Value {
    serde_json::json!({
        "packages_audited": report.packages_audited,
        "bytes_scanned": report.bytes_scanned,
        "truncated": report.truncated,
        "findings": report.findings.iter().map(|f| {
            serde_json::json!({
                "rule": f.rule.id,
                "severity": match f.severity {
                    Severity::Info => "info",
                    Severity::Warning => "warning",
                    Severity::Error => "error",
                },
                "package": f.package,
                "location": f.location,
                "detail": f.detail,
            })
        }).collect::<Vec<_>>(),
        "proposals": report.proposals.iter().map(|p| {
            serde_json::json!({
                "rule": p.rule.id,
                "package": p.package,
                "path": p.path,
                "diff": p.diff,
            })
        }).collect::<Vec<_>>(),
    })
}

pub fn audit_manifest_blocking(
    manifest_path: &std::path::Path,
) -> Result<serde_json::Value, String> {
    let content = std::fs::read_to_string(manifest_path).map_err(|e| e.to_string())?;
    let manifest = SkillManifest::from_toml(&content).map_err(|e| e.to_string())?;

    let sources: Vec<std::path::PathBuf> = manifest
        .sources
        .values()
        .map(std::path::PathBuf::from)
        .collect();
    let targets: Vec<std::path::PathBuf> = manifest
        .targets
        .iter()
        .map(|t| std::path::PathBuf::from(&t.root))
        .collect();
    let policy = SkillFsPolicy::new(&sources, &targets).map_err(|e| e.to_string())?;
    let roots = SkillResolvedRoots {
        sources: manifest
            .sources
            .iter()
            .map(|(id, dir)| (id.clone(), std::path::PathBuf::from(dir)))
            .collect(),
        targets: manifest
            .targets
            .iter()
            .map(|t| (t.id.clone(), std::path::PathBuf::from(&t.root)))
            .collect(),
    };

    let packages: Vec<AuditedPackage> = manifest
        .placements
        .iter()
        .map(|p| AuditedPackage {
            placement: p.clone(),
            recorded_hash: None,
        })
        .collect();

    let report = audit_packages(&policy, &roots, &packages).map_err(|e| e.to_string())?;

    Ok(audit_report_json(&report))
}

pub async fn audit_skills() -> axum::response::Response {
    let path_str = std::env::var("XAVIER_SKILL_MANIFEST").unwrap_or_default();
    let path_str = path_str.trim();
    if path_str.is_empty() {
        return (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "no skill manifest configured"})),
        )
            .into_response();
    }
    let path = std::path::PathBuf::from(path_str);

    match tokio::task::spawn_blocking(move || audit_manifest_blocking(&path)).await {
        Ok(Ok(json)) => (axum::http::StatusCode::OK, Json(json)).into_response(),
        Ok(Err(msg)) => (
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": msg})),
        )
            .into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "audit task failed"})),
        )
            .into_response(),
    }
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
        skill_manifest: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        /// Point `HOME` at the test tempdir and `XAVIER_SKILL_STORE` at an empty
        /// store, so the registry never scans the developer's real
        /// `~/.hermes/skills` nor reads their `~/.hermes/config.yaml`.
        fn isolate(temp_dir: &std::path::Path) -> Self {
            let guard = Self {
                home: std::env::var("HOME").ok(),
                skill_store: std::env::var("XAVIER_SKILL_STORE").ok(),
                skill_manifest: std::env::var_os("XAVIER_SKILL_MANIFEST"),
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
            match self.skill_manifest.take() {
                Some(value) => std::env::set_var("XAVIER_SKILL_MANIFEST", value),
                None => std::env::remove_var("XAVIER_SKILL_MANIFEST"),
            }
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

    #[tokio::test]
    async fn test_audit_manifest_blocking_and_endpoint() {
        let _lock = crate::context::skill_registry::env_lock().lock().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let _env_guard = EnvGuard::isolate(temp_dir.path());

        let sources_dir = temp_dir.path().join("sources");
        let targets_dir = temp_dir.path().join("targets");
        let package_dir = sources_dir.join("demo");
        std::fs::create_dir_all(&package_dir).unwrap();
        std::fs::create_dir_all(&targets_dir).unwrap();

        let skill_md_path = package_dir.join("SKILL.md");
        let content = "---\nname: demo\n---\nbody";
        let before_content = content.as_bytes().to_vec();
        std::fs::write(&skill_md_path, content).unwrap();

        let manifest_path = temp_dir.path().join("manifest.toml");
        let manifest_content = format!(
            r#"version = 1

[sources]
test_source = "{}"

[[targets]]
id = "test_target"
root = "{}"
tools = ["codex"]
mode = "symlink"

[[placements]]
source = "test_source"
targets = ["test_target"]
path = "demo"
name = "demo"
"#,
            sources_dir.display(),
            targets_dir.display()
        );
        std::fs::write(&manifest_path, &manifest_content).unwrap();
        let before_files = vec![
            (skill_md_path.clone(), before_content),
            (manifest_path.clone(), manifest_content.into_bytes()),
        ];

        let result = audit_manifest_blocking(&manifest_path).unwrap();
        let findings = result.get("findings").unwrap().as_array().unwrap();
        assert!(
            !findings.is_empty(),
            "Expected findings due to missing description"
        );

        std::env::set_var(
            "XAVIER_SKILL_MANIFEST",
            format!("  {}  ", manifest_path.display()),
        );
        let app = Router::new().route("/api/skill/audit", get(audit_skills));
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/skill/audit")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let (parts, body) = resp.into_parts();
        assert_eq!(parts.status, StatusCode::OK);

        let body_bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        let endpoint_findings = json.get("findings").unwrap().as_array().unwrap();
        assert_eq!(findings, endpoint_findings);

        for (path, bytes) in before_files {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
        assert_eq!(std::fs::read_dir(&targets_dir).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn test_audit_skills_unset_manifest_returns_404() {
        let _lock = crate::context::skill_registry::env_lock().lock().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let _env_guard = EnvGuard::isolate(temp_dir.path());
        std::env::remove_var("XAVIER_SKILL_MANIFEST");
        let resp_not_found = audit_skills().await;
        let (parts_not_found, _) = resp_not_found.into_parts();
        assert_eq!(parts_not_found.status, StatusCode::NOT_FOUND);
    }
}
