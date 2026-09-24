//! Context regeneration API handlers.

use axum::{extract::Json, http::StatusCode, response::IntoResponse, Extension};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::codebase::conversations_db::Message;
use crate::context::{
    ContextBudgetConfig, ContextBuilder, ContextBuilderConfig, ContextDocument, ContextLevel,
    Orchestrator,
};
use crate::observability::token_accounting::TRACKER;
use crate::workspace::WorkspaceContext;

pub const SKILL_CONFIDENCE_THRESHOLD: f32 = 0.5;

/// Predicate to check if a prompt is trivial (acknowledgments, greetings, very short confirmations).
/// Such prompts skip skill dispatch entirely (0 ms path).
pub fn is_trivial_prompt(prompt: &str) -> bool {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return true;
    }

    let words: Vec<&str> = trimmed.split_whitespace().collect();
    if words.len() > 5 {
        return false;
    }

    let normalized: String = trimmed
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();

    let clean = normalized.trim();
    matches!(
        clean,
        "ok" | "ok thanks"
            | "ok thank you"
            | "thanks"
            | "thank you"
            | "thx"
            | "yes"
            | "yeah"
            | "yep"
            | "no"
            | "nope"
            | "sure"
            | "got it"
            | "understood"
            | "cool"
            | "great"
            | "hello"
            | "hi"
            | "hey"
            | "good morning"
            | "good afternoon"
            | "good evening"
            | "bye"
            | "goodbye"
            | "done"
            | "proceed"
            | "continue"
            | "k"
    )
}

#[derive(Debug, Deserialize)]
pub struct RegenerateRequest {
    pub session_id: String,
    pub depth: String, // "shallow", "medium", "deep"
}

#[derive(Debug, Serialize)]
pub struct RegenerateResponse {
    pub status: String,
    pub context: String,
    pub token_usage: TokenUsage,
    pub savings: TokenSavings,
}

#[derive(Debug, Serialize)]
pub struct TokenUsage {
    pub depth: String,
    pub token_count: usize,
}

#[derive(Debug, Serialize)]
pub struct TokenSavings {
    pub original_tokens: usize,
    pub optimized_tokens: usize,
    pub savings_percentage: f32,
}

#[derive(Debug, Deserialize)]
pub struct DeepenRequest {
    pub session_id: String,
    pub focus: Vec<String>,
}

/// V1 context regenerate.
pub async fn v1_context_regenerate(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(payload): Json<RegenerateRequest>,
) -> impl IntoResponse {
    let depth = payload.depth.to_lowercase();
    let (level, token_budget) = match depth.as_str() {
        "shallow" => (ContextLevel::Minimal, 50),
        "medium" => (ContextLevel::Medium, 200),
        "deep" => (ContextLevel::Maximum, 1000),
        _ => (ContextLevel::Medium, 200),
    };

    // 1. Fetch session history from ConversationsDb
    let messages: Vec<Message> = match workspace
        .workspace
        .conversations_db
        .get_thread_messages(&payload.session_id)
        .await
    {
        Ok(msgs) => msgs,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to fetch messages: {}", e) })),
            )
                .into_response()
        }
    };

    let original_token_count: usize = messages
        .iter()
        .map(|m| m.tokens.unwrap_or(0) as usize)
        .sum();

    // Convert messages to ContextDocuments
    let context_docs: Vec<ContextDocument> = messages
        .into_iter()
        .map(|m| {
            ContextDocument::new(m.id, &payload.session_id, m.role, m.content)
                .with_token_count(m.tokens.unwrap_or(0) as usize)
                .with_created_at(m.created_at)
        })
        .collect();

    // 2. Use Orchestrator to select relevant documents
    let mut budget_config = ContextBudgetConfig::default();
    // Override budgets for this request
    match level {
        ContextLevel::Minimal => {
            budget_config.session_start_min_tokens = token_budget;
            budget_config.session_start_min_docs = 2;
        }
        ContextLevel::Medium => {
            budget_config.session_start_med_tokens = token_budget;
            budget_config.session_start_med_docs = 5;
        }
        ContextLevel::Maximum => {
            budget_config.session_start_max_tokens = token_budget;
            budget_config.session_start_max_docs = 10;
        }
    }

    let orchestrator = Orchestrator::with_budgets(budget_config).with_memory(
        Arc::clone(&workspace.workspace.memory),
        Some(Arc::clone(&workspace.workspace.belief_graph)),
    );

    let plan = orchestrator
        .session_start(&payload.session_id, "regenerate context", &context_docs)
        .await;
    let selected_docs = orchestrator
        .execute(&plan, &context_docs, &payload.session_id)
        .await;

    // 3. Skill Dispatch on Maximum Level (Session Fusion)
    let mut injected_skills: Vec<String> = Vec::new();
    let mut injected_memories: Vec<ContextDocument> = Vec::new();

    if level == ContextLevel::Maximum {
        // Extract latest prompt from context documents
        let latest_prompt = context_docs
            .iter()
            .rev()
            .find(|d| d.role == "user")
            .map(|d| d.content.as_str())
            .or_else(|| context_docs.last().map(|d| d.content.as_str()))
            .unwrap_or("");

        if !is_trivial_prompt(latest_prompt) {
            use crate::context::skill_dispatcher::SkillDispatchRequest;
            use crate::context::skill_registry::SkillRegistry;
            use crate::context::SkillDispatcher;

            let workspace_root = std::path::PathBuf::from(&workspace.workspace_id);
            let mut registry = SkillRegistry::with_defaults(&workspace_root);
            let _ = registry.reindex().await;
            let memory = workspace.workspace.memory.clone();
            let dispatcher = SkillDispatcher::new(registry, Some(memory));

            let dispatch_request = SkillDispatchRequest {
                task: latest_prompt.to_string(),
                model_hint: None,
                max_tokens: Some(token_budget),
                project: None,
            };

            if let Ok(result) = dispatcher.dispatch(&dispatch_request).await {
                if result.confidence >= SKILL_CONFIDENCE_THRESHOLD {
                    let selected_tokens: usize = selected_docs.iter().map(|d| d.token_count).sum();
                    let remaining_budget = token_budget.saturating_sub(selected_tokens);

                    if !result.context_pack.system_instructions.is_empty() {
                        let instructions = result.context_pack.system_instructions;
                        let compacted = if remaining_budget > 0 {
                            let words: Vec<&str> = instructions.split_whitespace().collect();
                            if words.len() > remaining_budget {
                                words[..remaining_budget].join(" ")
                            } else {
                                instructions
                            }
                        } else {
                            instructions
                        };
                        injected_skills.push(compacted);
                    }

                    for mem_ref in result.context_pack.relevant_memories {
                        let doc = ContextDocument::new(
                            mem_ref.id.clone(),
                            &payload.session_id,
                            "system",
                            mem_ref.summary,
                        )
                        .with_metadata(serde_json::json!({
                            "path": mem_ref.path,
                            "is_external": true,
                        }));
                        injected_memories.push(doc);
                    }
                }
            }
        }
    }

    // 4. Build optimized context
    let builder_config = ContextBuilderConfig::default();
    let builder = ContextBuilder::new(builder_config);

    let context_string = builder.build(level, &selected_docs, &injected_memories, &injected_skills);
    let optimized_token_count = context_string.split_whitespace().count();

    let savings_percentage = if original_token_count > 0 {
        (original_token_count as f32 - optimized_token_count as f32) / original_token_count as f32
            * 100.0
    } else {
        0.0
    };

    // Track savings (assume 0.01 USD per 1k tokens as default for now)
    TRACKER
        .track(
            payload.session_id.clone(),
            original_token_count,
            optimized_token_count,
            0.01,
        )
        .await;

    Json(RegenerateResponse {
        status: "ok".to_string(),
        context: context_string,
        token_usage: TokenUsage {
            depth: depth.clone(),
            token_count: optimized_token_count,
        },
        savings: TokenSavings {
            original_tokens: original_token_count,
            optimized_tokens: optimized_token_count,
            savings_percentage,
        },
    })
    .into_response()
}

/// V1 context deepen.
pub async fn v1_context_deepen(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(payload): Json<DeepenRequest>,
) -> impl IntoResponse {
    // Use deep level regeneration, filtering by focus areas
    let (level, token_budget) = (ContextLevel::Maximum, 1000);

    // 1. Fetch session history from ConversationsDb
    let messages: Vec<Message> = match workspace
        .workspace
        .conversations_db
        .get_thread_messages(&payload.session_id)
        .await
    {
        Ok(msgs) => msgs,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to fetch messages: {}", e) })),
            )
                .into_response()
        }
    };

    let original_token_count: usize = messages
        .iter()
        .map(|m| m.tokens.unwrap_or(0) as usize)
        .sum();

    // Convert messages to ContextDocuments
    let mut context_docs: Vec<ContextDocument> = messages
        .into_iter()
        .map(|m| {
            ContextDocument::new(m.id, &payload.session_id, m.role, m.content)
                .with_token_count(m.tokens.unwrap_or(0) as usize)
                .with_created_at(m.created_at)
        })
        .collect();

    // Filter by focus areas: keep only docs whose content contains at least one focus keyword
    if !payload.focus.is_empty() {
        let focus_lower: Vec<String> = payload.focus.iter().map(|f| f.to_lowercase()).collect();
        context_docs.retain(|doc| {
            let content_lower = doc.content.to_lowercase();
            focus_lower
                .iter()
                .any(|keyword| content_lower.contains(keyword))
        });
    }

    if context_docs.is_empty() && !payload.focus.is_empty() {
        return Json(serde_json::json!({
            "status": "ok",
            "message": "No messages matching focus areas found",
            "focus": payload.focus,
            "session_id": payload.session_id
        }))
        .into_response();
    }

    // 2. Use Orchestrator to select relevant documents (deep budget)
    let budget_config = ContextBudgetConfig {
        session_start_max_tokens: token_budget,
        session_start_max_docs: 10,
        ..Default::default()
    };

    let orchestrator = Orchestrator::with_budgets(budget_config).with_memory(
        Arc::clone(&workspace.workspace.memory),
        Some(Arc::clone(&workspace.workspace.belief_graph)),
    );

    let plan = orchestrator
        .session_start(&payload.session_id, "deepen context", &context_docs)
        .await;
    let selected_docs = orchestrator
        .execute(&plan, &context_docs, &payload.session_id)
        .await;

    // 3. Build optimized context
    let builder_config = ContextBuilderConfig::default();
    let builder = ContextBuilder::new(builder_config);
    let context_string = builder.build(level, &selected_docs, &[], &[]);
    let optimized_token_count = context_string.split_whitespace().count();

    let savings_percentage = if original_token_count > 0 {
        (original_token_count as f32 - optimized_token_count as f32) / original_token_count as f32
            * 100.0
    } else {
        0.0
    };

    // Track savings
    TRACKER
        .track(
            payload.session_id.clone(),
            original_token_count,
            optimized_token_count,
            0.01,
        )
        .await;

    Json(serde_json::json!({
        "status": "ok",
        "context": context_string,
        "token_usage": {
            "depth": "deep",
            "token_count": optimized_token_count
        },
        "savings": {
            "original_tokens": original_token_count,
            "optimized_tokens": optimized_token_count,
            "savings_percentage": savings_percentage
        },
        "focus": payload.focus
    }))
    .into_response()
}

/// V1 context stats.
pub async fn v1_context_stats() -> impl IntoResponse {
    let stats = TRACKER.get_stats().await;
    Json(stats).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::builder::{ContextBuilder, ContextBuilderConfig};
    use crate::context::classifier::ContextLevel;
    use crate::context::skill_registry::IndexedSkill;

    #[test]
    fn test_is_trivial_prompt() {
        assert!(is_trivial_prompt("ok"));
        assert!(is_trivial_prompt("ok thanks"));
        assert!(is_trivial_prompt("thanks"));
        assert!(is_trivial_prompt("thank you"));
        assert!(is_trivial_prompt("Got it"));
        assert!(is_trivial_prompt(""));
        assert!(is_trivial_prompt("   "));

        assert!(!is_trivial_prompt(
            "Implement SQLite vector store migration"
        ));
        assert!(!is_trivial_prompt(
            "Refactor the auth middleware to support bearer tokens"
        ));
        assert!(!is_trivial_prompt(
            "Design the distributed mesh consensus protocol"
        ));
    }

    #[test]
    fn test_fusion_injects_skill_on_maximum() {
        let builder = ContextBuilder::new(ContextBuilderConfig::default());
        let skills = vec!["# Specialized Skill: Rust Microservices\nAlways use Axum.".to_string()];
        let context = builder.build(ContextLevel::Maximum, &[], &[], &skills);

        assert!(context.contains("# Available Skills"));
        assert!(context.contains("Specialized Skill: Rust Microservices"));
    }

    #[test]
    fn test_fusion_skips_below_confidence() {
        let confidence = 0.35f32;
        let mut injected_skills = Vec::new();

        if confidence >= SKILL_CONFIDENCE_THRESHOLD {
            injected_skills.push("Should not be added".to_string());
        }

        let builder = ContextBuilder::new(ContextBuilderConfig::default());
        let context = builder.build(ContextLevel::Maximum, &[], &[], &injected_skills);

        assert!(!context.contains("# Available Skills"));
    }

    #[test]
    fn test_fusion_skips_trivial_prompt() {
        let prompt = "ok thanks";
        let should_dispatch = !is_trivial_prompt(prompt);
        assert!(!should_dispatch);

        let mut injected_skills = Vec::new();
        if should_dispatch {
            injected_skills.push("Skill for ok thanks".to_string());
        }

        let builder = ContextBuilder::new(ContextBuilderConfig::default());
        let context = builder.build(ContextLevel::Maximum, &[], &[], &injected_skills);

        assert!(!context.contains("# Available Skills"));
    }

    #[test]
    fn test_fusion_respects_token_budget() {
        let skill = IndexedSkill {
            name: "test-skill".to_string(),
            description: "A test skill for testing budget constraints".to_string(),
            domains: vec!["test".to_string()],
            content_hash: "hash-test-skill".to_string(),
            token_cost: 500,
            content: "word ".repeat(500),
            source_path: "/tmp/test".to_string(),
            embedding: None,
        };

        let remaining_budget = 50usize;
        let compacted = skill.compacted_content(remaining_budget);
        let token_count = compacted.split_whitespace().count();

        // Compaction keeps the first `remaining_budget` content words plus a
        // fixed 5-word truncation marker.
        assert!(token_count <= remaining_budget + 5);

        let builder = ContextBuilder::new(ContextBuilderConfig::default());
        let context = builder.build(ContextLevel::Maximum, &[], &[], &[compacted]);
        assert!(context.contains("# Available Skills"));
    }

    /// Build an isolated workspace: temp HOME (no real skill store), one
    /// fixture skill, in-memory memory + store, minimal state.
    async fn fusion_test_workspace(
        skill_name: &str,
        skill_description: &str,
    ) -> (
        crate::workspace::WorkspaceContext,
        tempfile::TempDir,
        tempfile::TempDir,
        Option<std::ffi::OsString>,
    ) {
        let home = tempfile::tempdir().expect("temp HOME");
        let prev_home = std::env::var_os("HOME");
        std::env::set_var("HOME", home.path());

        let root = tempfile::tempdir().expect("temp workspace root");
        let skill_dir = root.path().join("skills").join(skill_name);
        std::fs::create_dir_all(&skill_dir).expect("fixture skill dir");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            format!(
                "---\nname: {skill_name}\ndescription: \"{skill_description}\"\n---\n\n# {skill_name}\n\nFixture body.\n"
            ),
        )
        .expect("fixture SKILL.md");

        let memory = std::sync::Arc::new(crate::memory::qmd_memory::QmdMemory::new(
            std::sync::Arc::new(tokio::sync::RwLock::new(Vec::new())),
        ));
        let store: std::sync::Arc<dyn crate::memory::store::MemoryStore> =
            std::sync::Arc::new(crate::memory::store::InMemoryMemoryStore::default());
        let state = crate::workspace::WorkspaceState::new_minimal(
            "fusion-e2e".to_string(),
            root.path().to_path_buf(),
            memory,
            store,
        )
        .await;
        let workspace = crate::workspace::WorkspaceContext {
            workspace_id: root.path().to_string_lossy().into_owned(),
            workspace: std::sync::Arc::new(state),
        };
        (workspace, home, root, prev_home)
    }

    fn restore_home(prev_home: Option<std::ffi::OsString>) {
        match prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }

    async fn regenerate_context(
        workspace: crate::workspace::WorkspaceContext,
        session_id: &str,
        depth: &str,
    ) -> String {
        let response = v1_context_regenerate(
            Extension(workspace),
            Json(RegenerateRequest {
                session_id: session_id.to_string(),
                depth: depth.to_string(),
            }),
        )
        .await
        .into_response();
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("response body");
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON body");
        assert_eq!(body["status"], "ok");
        body["context"].as_str().unwrap_or_default().to_string()
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_fusion_e2e_seeded_thread_injects_skill() {
        let (workspace, _home, _root, prev_home) = fusion_test_workspace(
            "mesh-architect",
            "Design distributed mesh consensus protocols and architecture",
        )
        .await;
        let thread = workspace
            .workspace
            .conversations_db
            .create_thread(Some("fusion e2e"), None, Some("test"))
            .await
            .expect("thread");
        workspace
            .workspace
            .conversations_db
            .store_message(
                &thread.id,
                "user",
                "Design the distributed mesh consensus protocol for the cluster",
                None,
                None,
                None,
                None,
                Some(40),
            )
            .await
            .expect("seed message");

        let context = regenerate_context(workspace, &thread.id, "deep").await;
        restore_home(prev_home);
        assert!(
            context.contains("# Available Skills"),
            "seeded architecture prompt must inject skills on Maximum"
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_fusion_e2e_ack_prompt_injects_nothing() {
        let (workspace, _home, _root, prev_home) = fusion_test_workspace(
            "mesh-architect",
            "Design distributed mesh consensus protocols and architecture",
        )
        .await;
        let thread = workspace
            .workspace
            .conversations_db
            .create_thread(Some("fusion e2e ack"), None, Some("test"))
            .await
            .expect("thread");
        workspace
            .workspace
            .conversations_db
            .store_message(
                &thread.id,
                "user",
                "ok thanks",
                None,
                None,
                None,
                None,
                Some(5),
            )
            .await
            .expect("seed message");

        let context = regenerate_context(workspace, &thread.id, "deep").await;
        restore_home(prev_home);
        assert!(
            !context.contains("# Available Skills"),
            "ack prompt must stay skill-free even on Maximum"
        );
    }
}
