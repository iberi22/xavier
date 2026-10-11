//! Context-related MCP tools
//!
//! Provides tools for saving, restoring, and searching optimized context,
//! as well as reporting token savings and packaging GitHub issues into PreciseChange context packages.

use super::types::*;
use crate::codebase::issue_context::assemble_package;
use crate::codebase::snapshot::SnapshotManager;
use crate::context::skill_registry::build_skill_registry;
use crate::context::{
    ContextBudgetConfig, ContextBuilder, ContextBuilderConfig, ContextDocument, ContextLevel,
    Orchestrator,
};
use crate::memory::schema::MemoryQueryFilters;
use crate::observability::token_accounting::TRACKER;
use crate::workspace::WorkspaceContext;
use crate::AppState;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

/// Get xavier context tools.
pub fn get_xavier_context_tools() -> Vec<MCPTool> {
    vec![
        MCPTool {
            name: "xavier_context_save".to_string(),
            description: "Save current session context as a checkpoint in Xavier".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session identifier" },
                    "name": { "type": "string", "description": "Optional name for the checkpoint" }
                },
                "required": ["session_id"]
            }),
        },
        MCPTool {
            name: "xavier_context_restore".to_string(),
            description: "Retrieve an optimized context block for a session (wraps regenerate)"
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session identifier" },
                    "depth": { "type": "string", "enum": ["shallow", "medium", "deep"], "default": "medium" }
                },
                "required": ["session_id"]
            }),
        },
        MCPTool {
            name: "xavier_context_search".to_string(),
            description: "Search within saved session contexts".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "session_id": { "type": "string", "description": "Optional session filter" }
                },
                "required": ["query"]
            }),
        },
        MCPTool {
            name: "xavier_token_savings".to_string(),
            description: "Report token savings achieved through Xavier context regeneration"
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        MCPTool {
            name: "xavier_issue_context_package".to_string(),
            description: "Analyze a GitHub issue (title, body, repo) and produce a PreciseChange context package"
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "issue_id": { "type": "string", "description": "GitHub issue ID or number" },
                    "title": { "type": "string", "description": "Issue title" },
                    "body": { "type": "string", "description": "Issue body / description text" },
                    "repo": { "type": "string", "description": "Repository identifier (e.g. owner/repo)" },
                    "repo_path": { "type": "string", "description": "Path to local repository root (defaults to current dir)" }
                },
                "required": ["issue_id", "title", "body"]
            }),
        },
        MCPTool {
            name: "xavier_run_command".to_string(),
            description: "Execute a shell command via Xavier RTK Kernel Proxy with output condensation and token savings tracking".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Command line to execute" },
                    "cwd": { "type": "string", "description": "Optional working directory" },
                    "session_id": { "type": "string", "description": "Optional session ID for token accounting" }
                },
                "required": ["command"]
            }),
        },
        MCPTool {
            name: "xavier_dispatch_skill".to_string(),
            description: "Dispatch a task to the best skill".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task": { "type": "string", "description": "Task to match a skill" },
                    "max_tokens": { "type": "integer", "description": "Token budget, default 4000" },
                    "project": { "type": "string", "description": "Optional project filter" }
                },
                "required": ["task"]
            }),
        },
        MCPTool {
            name: "xavier_skill_list".to_string(),
            description: "List indexed skills with costs".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        MCPTool {
            name: "xavier_skill_usage".to_string(),
            description: "Read skill lifecycle counts and harness acknowledgment coverage".to_string(),
            input_schema: json!({"type": "object", "properties": {}}),
        },
    ]
}

/// Handle context tool.
pub async fn handle_context_tool(
    state: AppState,
    workspace: WorkspaceContext,
    name: &str,
    arguments: Value,
) -> anyhow::Result<Value> {
    match name {
        "xavier_context_save" => {
            let session_id = arguments
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let name = arguments.get("name").and_then(|v| v.as_str());

            let checkpoint = workspace
                .workspace
                .conversations_db
                .create_checkpoint(
                    Some(&workspace.workspace_id),
                    Some(session_id),
                    None,
                    name,
                    Some("context_save"),
                )
                .await?;

            super::server::mcp_text_result(
                format!(
                    "Context saved for session {}. Checkpoint ID: {}",
                    session_id, checkpoint.id
                ),
                false,
            )
        }
        "xavier_context_restore" => {
            let session_id = arguments
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let depth = arguments
                .get("depth")
                .and_then(|v| v.as_str())
                .unwrap_or("medium");

            let (level, token_budget) = match depth {
                "shallow" => (ContextLevel::Minimal, 150),
                "deep" => (ContextLevel::Maximum, 2400),
                _ => (ContextLevel::Medium, 800),
            };

            // Fetch session history
            let messages = workspace
                .workspace
                .conversations_db
                .get_thread_messages(session_id)
                .await?;

            let original_token_count: usize = messages
                .iter()
                .map(|m| {
                    let tokens = m.tokens.unwrap_or(0) as usize;
                    if tokens > 0 {
                        tokens
                    } else {
                        crate::context::estimate_tokens(&m.content)
                    }
                })
                .sum();

            let context_docs: Vec<ContextDocument> = messages
                .into_iter()
                .map(|m| {
                    let token_count = m.tokens.unwrap_or(0) as usize;
                    let token_count = if token_count > 0 {
                        token_count
                    } else {
                        crate::context::estimate_tokens(&m.content)
                    };
                    ContextDocument::new(m.id, session_id, m.role, m.content)
                        .with_token_count(token_count)
                        .with_created_at(m.created_at)
                })
                .collect();

            // Use Orchestrator
            let mut budget_config = ContextBudgetConfig::default();
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
                .session_start(session_id, "restore context", &context_docs)
                .await;
            let selected_docs = orchestrator.execute(&plan, &context_docs, session_id).await;

            // Build optimized context
            let builder_config = ContextBuilderConfig::default();
            let builder = ContextBuilder::new(builder_config);
            let context_string = builder.build(level, &selected_docs, &[], &[]);

            // Use unified honest estimator
            let optimized_token_count = crate::context::estimate_tokens(&context_string);

            let savings_percentage = if original_token_count > 0 {
                (original_token_count as f32 - optimized_token_count as f32)
                    / original_token_count as f32
                    * 100.0
            } else {
                0.0
            };

            TRACKER
                .track(
                    session_id.to_string(),
                    original_token_count,
                    optimized_token_count,
                    0.01,
                )
                .await;

            let result = json!({
                "status": "ok",
                "session_id": session_id,
                "depth": depth,
                "context": context_string,
                "token_usage": {
                    "original": original_token_count,
                    "optimized": optimized_token_count,
                    "savings_percentage": format!("{:.1}%", savings_percentage)
                }
            });

            super::server::mcp_text_result(serde_json::to_string_pretty(&result)?, false)
        }
        "xavier_context_search" => {
            let query = arguments
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let session_id = arguments.get("session_id").and_then(|v| v.as_str());

            let filters = session_id.map(|sid| MemoryQueryFilters {
                session_id: Some(sid.to_string()),
                ..Default::default()
            });

            let results = workspace
                .workspace
                .memory
                .search_filtered(query, 10, filters.as_ref())
                .await?;

            if results.is_empty() {
                return super::server::mcp_text_result(
                    format!("No context found for query: {}", query),
                    false,
                );
            }

            let mut output = String::new();
            for (i, doc) in results.iter().enumerate() {
                let snippet = if doc.content.len() > 200 {
                    format!(
                        "{}...",
                        crate::memory::snippet::clip_chars(&doc.content, 200)
                    )
                } else {
                    doc.content.clone()
                };
                let meta_preview = match &doc.metadata {
                    serde_json::Value::Object(m) => {
                        let entries: Vec<String> = m
                            .iter()
                            .take(4)
                            .map(|(k, v)| format!("{}: {}", k, v))
                            .collect();
                        entries.join(", ")
                    }
                    _ => String::new(),
                };
                output.push_str(&format!(
                    "--- Result {} ---\nPath: {}\nMetadata: {}\nSnippet: {}\n\n",
                    i + 1,
                    doc.path,
                    meta_preview,
                    snippet
                ));
            }

            super::server::mcp_text_result(output, false)
        }
        "xavier_token_savings" => {
            let stats = TRACKER.get_stats().await;
            let search_stats = crate::observability::token_accounting::SEARCH_STATS.snapshot();
            let combined = json!({
                "execution_and_restore": stats,
                "search_progressive_disclosure": search_stats,
            });
            super::server::mcp_text_result(serde_json::to_string_pretty(&combined)?, false)
        }
        "xavier_run_command" => {
            let command = arguments
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let cwd = arguments.get("cwd").and_then(|v| v.as_str());
            let session_id = arguments.get("session_id").and_then(|v| v.as_str());

            if command.is_empty() {
                return super::server::mcp_text_result(
                    "Error: 'command' argument cannot be empty",
                    true,
                );
            }

            // Fail-closed cwd validation: a caller-controlled working
            // directory must exist and be a real directory (symlinks
            // resolved). Rejects traversal into non-directories instead of
            // silently falling back to the server cwd.
            let cwd_canonical: Option<String> = match cwd {
                Some(dir) => match std::fs::canonicalize(dir) {
                    Ok(p) if p.is_dir() => Some(p.to_string_lossy().into_owned()),
                    _ => {
                        return super::server::mcp_text_result(
                            "Error: 'cwd' must be an existing directory",
                            true,
                        );
                    }
                },
                None => None,
            };

            let result = crate::kernel::runner::execute_rtk_command(
                command,
                cwd_canonical.as_deref(),
                session_id,
            )
            .await?;
            let json_res = serde_json::to_string_pretty(&result)?;
            super::server::mcp_text_result(json_res, result.exit_code != 0)
        }
        "xavier_issue_context_package" => {
            let issue_id = arguments
                .get("issue_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let title = arguments
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let body = arguments.get("body").and_then(|v| v.as_str()).unwrap_or("");
            let repo = arguments
                .get("repo")
                .and_then(|v| v.as_str())
                .unwrap_or("owner/repo");
            let repo_path_str = arguments
                .get("repo_path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");

            let repo_root = Path::new(repo_path_str);
            let snapshot_manager = SnapshotManager::new(repo_root);

            let package = assemble_package(
                issue_id,
                title,
                repo,
                body,
                &state.code_db,
                &snapshot_manager,
                repo_root,
            )?;

            super::server::mcp_text_result(serde_json::to_string_pretty(&package)?, false)
        }
        "xavier_dispatch_skill" => {
            use crate::context::skill_dispatcher::{SkillDispatchRequest, SkillDispatcher};

            let task = arguments.get("task").and_then(|v| v.as_str()).unwrap_or("");
            if task.is_empty() {
                return super::server::mcp_text_result(
                    serde_json::to_string(&json!({
                        "ok": false,
                        "error": "'task' argument cannot be empty",
                    }))?,
                    true,
                );
            }
            let max_tokens = arguments
                .get("max_tokens")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
                .unwrap_or(4000);
            let project = arguments
                .get("project")
                .and_then(|v| v.as_str())
                .map(str::to_string);

            let registry = build_skill_registry(Path::new(&workspace.workspace_id)).await;
            let memory = workspace.workspace.memory.clone();
            let dispatcher = SkillDispatcher::new(registry, Some(memory))
                .with_clearance(super::tools_memory::caller_clearance())
                .with_telemetry_db(crate::api::skills::skill_usage_db_path());
            match dispatcher
                .dispatch(&SkillDispatchRequest {
                    task: task.to_string(),
                    model_hint: None,
                    max_tokens: Some(max_tokens),
                    project,
                })
                .await
            {
                Ok(result) => super::server::mcp_text_result(
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "skill_name": result.skill_name,
                        "skill_description": result.skill_description,
                        "confidence": result.confidence,
                        "context_pack": {
                            "system_instructions": result.context_pack.system_instructions,
                            "composition": result.context_pack.composition,
                            "relevant_memories": result.context_pack.relevant_memories,
                            "prior_decisions": result.context_pack.prior_decisions,
                            "total_tokens": result.context_pack.total_tokens,
                        },
                        "estimated_savings_pct": result.estimated_savings_pct,
                        "hidden_by_clearance": result.hidden_by_clearance,
                    }))?,
                    false,
                ),
                // Fail-open: empty-skill verdict, never a throw across MCP.
                Err(e) => super::server::mcp_text_result(
                    serde_json::to_string(&json!({
                        "ok": false,
                        "skill_name": "_none",
                        "confidence": 0.0,
                        "error": format!("{e}"),
                    }))?,
                    true,
                ),
            }
        }
        "xavier_skill_usage" => {
            let summary = tokio::task::spawn_blocking(|| {
                crate::api::skills::usage_summary_blocking(
                    &crate::api::skills::skill_usage_db_path(),
                )
            })
            .await?;
            match summary {
                Ok(summary) => {
                    super::server::mcp_text_result(serde_json::to_string_pretty(&summary)?, false)
                }
                Err(error) => super::server::mcp_text_result(
                    serde_json::to_string(&json!({"error": error}))?,
                    true,
                ),
            }
        }
        "xavier_skill_list" => {
            let registry = build_skill_registry(Path::new(&workspace.workspace_id)).await;
            let skills: Vec<Value> = registry
                .list()
                .into_iter()
                .filter_map(|name| {
                    registry.get(name).map(|s| {
                        json!({
                            "name": s.name,
                            "description": s.description,
                            "domains": s.domains,
                            "token_cost": s.token_cost,
                        })
                    })
                })
                .collect();
            super::server::mcp_text_result(
                serde_json::to_string_pretty(&json!({
                    "ok": true,
                    "count": skills.len(),
                    "skills": skills,
                }))?,
                false,
            )
        }
        _ => Err(anyhow::anyhow!("Unknown context tool: {}", name)),
    }
}

#[cfg(test)]
mod tests {
    use super::handle_context_tool;
    use crate::workspace::WorkspaceContext;
    use axum::response::IntoResponse;
    use serde_json::{json, Value};
    use std::path::Path;
    use tokio::sync::MutexGuard;

    /// Points `HOME` at a throwaway directory and clears `XAVIER_SKILL_STORE`
    /// so only the test fixture is scanned, and restores both previous values
    /// on drop so a failing assertion cannot leak overrides into sibling tests.
    struct IsolatedHome {
        _lock: MutexGuard<'static, ()>,
        _home: tempfile::TempDir,
        previous_home: Option<std::ffi::OsString>,
        previous_store: Option<std::ffi::OsString>,
    }

    impl IsolatedHome {
        async fn acquire() -> Self {
            let lock = crate::context::skill_registry::env_lock().lock().await;
            let home = tempfile::tempdir().expect("temp HOME");
            let previous_home = std::env::var_os("HOME");
            let previous_store = std::env::var_os("XAVIER_SKILL_STORE");
            std::env::set_var("HOME", home.path());
            std::env::remove_var("XAVIER_SKILL_STORE");
            Self {
                _lock: lock,
                _home: home,
                previous_home,
                previous_store,
            }
        }
    }

    impl Drop for IsolatedHome {
        fn drop(&mut self) {
            match self.previous_home.take() {
                Some(value) => std::env::set_var("HOME", value),
                None => std::env::remove_var("HOME"),
            }
            match self.previous_store.take() {
                Some(value) => std::env::set_var("XAVIER_SKILL_STORE", value),
                None => std::env::remove_var("XAVIER_SKILL_STORE"),
            }
        }
    }

    /// Write a fixture skill in real store shape: `<root>/skills/<name>/SKILL.md`.
    fn write_fixture_skill(root: &Path, name: &str, description: &str) {
        let dir = root.join("skills").join(name);
        std::fs::create_dir_all(&dir).expect("fixture skill dir");
        std::fs::write(
            dir.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: \"{description}\"\n---\n\n# {name}\n\nFixture body.\n"
            ),
        )
        .expect("fixture SKILL.md");
    }

    fn mcp_payload(out: &Value) -> Value {
        let text = out["content"][0]["text"]
            .as_str()
            .expect("mcp text content");
        serde_json::from_str(text).expect("tool payload is JSON")
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn skill_usage_cli_rest_mcp_agree() {
        let _home = IsolatedHome::acquire().await;
        let temp = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("XAVIER_DATA_DIR");
        struct Restore(Option<std::ffi::OsString>);
        impl Drop for Restore {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(value) => std::env::set_var("XAVIER_DATA_DIR", value),
                    None => std::env::remove_var("XAVIER_DATA_DIR"),
                }
            }
        }
        let _restore = Restore(previous);
        std::env::set_var("XAVIER_DATA_DIR", temp.path());
        let db = crate::api::skills::skill_usage_db_path();
        crate::api::skills::seed_usage_fixture(&db);
        let cli = crate::api::skills::usage_summary_blocking(&db).unwrap();
        let response = crate::api::skills::skill_usage().await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        let rest: Value = serde_json::from_slice(&body).unwrap();
        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        let claims = crate::security::auth::Claims::new(
            "fixture-reader".into(),
            "fixture@example.invalid".into(),
            crate::security::auth::UserRole::Readonly,
            chrono::Duration::hours(1),
        );
        let listing = super::super::session::dispatch_mcp_value(
            state.clone(),
            workspace.clone(),
            Some(&claims),
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(listing.get("error").is_none());
        let tool = listing["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "xavier_skill_usage")
            .unwrap();
        assert_eq!(
            tool["inputSchema"],
            json!({"type": "object", "properties": {}})
        );
        let response = super::super::session::dispatch_mcp_value(
            state,
            workspace,
            Some(&claims),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": "xavier_skill_usage", "arguments": {}}}),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(response.get("error").is_none(), "{response}");
        let output = &response["result"];
        assert_eq!(output["isError"], false);
        assert_eq!(cli, rest);
        assert_eq!(cli, mcp_payload(output));
    }

    /// Skill names of a listing payload, sorted.
    ///
    /// `SkillRegistry::list` walks a `HashMap`, so the per-instance order is not
    /// stable; both surfaces must agree on the same skills and the same count.
    fn sorted_skill_names(payload: &Value) -> Vec<String> {
        let mut names: Vec<String> = payload["skills"]
            .as_array()
            .expect("skills array")
            .iter()
            .filter_map(|skill| skill["name"].as_str().map(str::to_string))
            .collect();
        names.sort();
        names
    }

    async fn rest_skill_list(workspace: &WorkspaceContext) -> Value {
        let response = crate::api::skills::list_skills(axum::extract::Extension(workspace.clone()))
            .await
            .into_response();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("rest body");
        serde_json::from_slice(&body).expect("rest payload is JSON")
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_mcp_skill_list_matches_rest_on_same_fixture() {
        let _home = IsolatedHome::acquire().await;
        let work = tempfile::tempdir().expect("temp workspace");
        for (name, description) in [
            ("alpha", "First fixture skill for parity"),
            ("beta", "Second fixture skill for parity"),
            ("gamma", "Third fixture skill for parity"),
        ] {
            write_fixture_skill(work.path(), name, description);
        }

        let (state, mut workspace) = crate::server::mcp::tests::test_state().await;
        workspace.workspace_id = work.path().to_string_lossy().into_owned();

        let rest = rest_skill_list(&workspace).await;
        let out = handle_context_tool(state, workspace, "xavier_skill_list", json!({}))
            .await
            .expect("mcp skill list never throws");
        let mcp = mcp_payload(&out);

        assert_eq!(rest["ok"], true);
        assert_eq!(mcp["ok"], true);
        assert_eq!(mcp["count"], 3, "isolated fixture is the only skill source");
        assert_eq!(
            mcp["count"], rest["count"],
            "MCP and REST must report the same skill count"
        );
        // SkillRegistry::list order is unspecified, so parity is asserted on the sorted set.
        assert_eq!(
            sorted_skill_names(&mcp),
            sorted_skill_names(&rest),
            "MCP and REST must report the same skills in the same listing"
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_live_composition_hash_matches_composer_without_files() {
        use crate::context::skill_controller::composer::{
            self, CompositionInput, CompositionScope, Evidence, SkillEntry,
        };
        use crate::context::skill_registry::build_skill_registry;
        use sha2::{Digest, Sha256};
        use std::sync::Arc;

        let home = IsolatedHome::acquire().await;
        let work = tempfile::tempdir().expect("temp workspace");
        let previous_cwd = std::env::current_dir().expect("cwd");
        struct CwdGuard(std::path::PathBuf);
        impl Drop for CwdGuard {
            fn drop(&mut self) {
                std::env::set_current_dir(&self.0).expect("restore cwd");
            }
        }
        let _cwd = CwdGuard(previous_cwd);
        std::env::set_current_dir(work.path()).expect("fixture cwd");
        let task = "Design distributed mesh consensus protocols and architecture";
        write_fixture_skill(work.path(), "mesh-architect", task);
        let skill_path = work.path().join("skills/mesh-architect/SKILL.md");
        let skill_text = std::fs::read_to_string(&skill_path).expect("fixture bytes");
        std::fs::write(
            &skill_path,
            skill_text.replacen("---\n", "---\nvisibility: public\n", 1),
        )
        .expect("public fixture skill");
        let (state, mut workspace) = crate::server::mcp::tests::test_state().await;
        let document = crate::memory::qmd_memory::MemoryDocument {
            id: Some("m-live".to_string()),
            path: "sessions/m-live".to_string(),
            content: "distributed mesh consensus architecture evidence".to_string(),
            metadata: json!({"kind": "observation", "namespace": {"workspace_id": "ws-live"}}),
            ..Default::default()
        };
        let memory = Arc::new(crate::memory::qmd_memory::QmdMemory::new_with_workspace(
            Arc::new(tokio::sync::RwLock::new(vec![document])),
            "ws-live",
        ));
        let store = Arc::new(crate::memory::store::InMemoryMemoryStore::default());
        workspace.workspace = Arc::new(
            crate::workspace::WorkspaceState::new_minimal(
                "ws-live".to_string(),
                work.path().to_path_buf(),
                memory,
                store,
            )
            .await,
        );
        workspace.workspace_id = work.path().to_string_lossy().into_owned();
        let registry = build_skill_registry(work.path()).await;
        let skill = registry.get("mesh-architect").expect("fixture skill");
        let scope = CompositionScope::new("ws-live", None).expect("scope");
        let expected = composer::compose(
            &scope,
            &CompositionInput {
                task_id: composer::task_fingerprint("ws-live", task),
                task: task.to_string(),
                skills: vec![SkillEntry {
                    name: skill.name.clone(),
                    content_hash: skill.content_hash.clone(),
                }],
                evidence: vec![Evidence::new(
                    "m-live",
                    "ws-live",
                    "memory",
                    "distributed mesh consensus architecture evidence",
                )],
                memory_available: true,
                graph_revision: None,
            },
            4000,
        )
        .expect("composition");
        let output = handle_context_tool(
            state,
            workspace,
            "xavier_dispatch_skill",
            json!({"task": task}),
        )
        .await
        .expect("live dispatch");
        let payload = mcp_payload(&output);
        assert_eq!(payload["ok"], true);
        let live = payload["context_pack"]["composition"]
            .as_str()
            .expect("live composition");
        assert_eq!(
            Sha256::digest(live.as_bytes()),
            Sha256::digest(expected.markdown.as_bytes())
        );
        assert_eq!(live, expected.markdown);
        let file_content = tokio::task::spawn_blocking(move || {
            let published_root = tempfile::tempdir().expect("file delivery fixture");
            let journal_root = tempfile::tempdir().expect("journal fixture");
            let journal = crate::context::skill_controller::state::Journal::new(
                journal_root.path().to_path_buf(),
            )
            .expect("journal");
            let published = crate::context::skill_controller::lease::publish_task_skill(
                &journal,
                published_root.path(),
                "live-fixture",
                &expected.markdown,
            )
            .expect("file publication");
            std::fs::read(published.skill_path).expect("published bytes")
        })
        .await
        .expect("file fixture worker");
        assert_eq!(
            Sha256::digest(live.as_bytes()),
            Sha256::digest(&file_content)
        );
        for root in [work.path(), home._home.path()] {
            assert!(!root.join(".agents").exists());
            assert!(!root.join(".claude").exists());
        }
    }

    #[test]
    fn test_mcp_context_tools_own_no_registry_constructor() {
        let source = include_str!("tools_context.rs");
        let production = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("split always yields a head");
        assert!(
            !production.contains("fn build_skill_registry"),
            "MCP must not re-declare a registry constructor; the shared one is the single site"
        );
        assert!(
            production.contains("skill_registry::build_skill_registry"),
            "MCP must build the registry through the shared constructor"
        );
    }
}
