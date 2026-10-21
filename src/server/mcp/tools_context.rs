//! Context-related MCP tools
//!
//! Provides tools for saving, restoring, and searching optimized context,
//! as well as reporting token savings and packaging GitHub issues into PreciseChange context packages.

use super::types::*;
use crate::codebase::issue_context::assemble_package;
use crate::codebase::snapshot::SnapshotManager;
use crate::context::skill_controller::telemetry::{SkillStage, UsageEvent};
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
use std::time::Duration;

/// Upper bound for the session existence lookups in `xavier_context_restore`.
const SESSION_LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);
/// Default and maximum result count of `xavier_context_search`.
const CONTEXT_SEARCH_DEFAULT_LIMIT: u64 = 10;
const CONTEXT_SEARCH_MAX_LIMIT: u64 = 50;

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
                    "session_id": { "type": "string", "description": "Optional session filter" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 10, "description": "Maximum number of results" }
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
        MCPTool {
            name: "xavier_skill_ack".to_string(),
            description: "Acknowledge that a delivered skill was invoked or completed (idempotent)".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "skill_name": { "type": "string", "description": "Skill name from the registry" },
                    "stage": { "type": "string", "enum": ["invoked", "completed"] },
                    "task_id": { "type": "string", "description": "Opaque task identifier, never task text" },
                    "tool": { "type": "string", "description": "Harness identifier" },
                    "project": { "type": "string", "description": "Optional opaque project id, default 'default'" },
                    "outcome": { "type": "string", "description": "Bounded outcome label, default ok" }
                },
                "required": ["skill_name", "stage", "task_id"]
            }),
        },
    ]
}

/// Session ids: 1-128 chars of ASCII alnum . _ - : / with no `..`.
fn validate_session_id(session_id: &str) -> Result<(), String> {
    let ok = !session_id.is_empty()
        && session_id.len() <= 128
        && !session_id.contains("..")
        && session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':' | '/'));
    if ok {
        Ok(())
    } else {
        Err("session_id must be 1-128 chars of ASCII alphanumerics or . _ - : / and not contain '..'"
            .to_string())
    }
}

/// Checkpoints of one session, newest first.
async fn session_checkpoints(
    workspace: &WorkspaceContext,
    session_id: &str,
) -> anyhow::Result<Vec<crate::codebase::conversations_db::Checkpoint>> {
    let all = workspace
        .workspace
        .conversations_db
        .list_checkpoints(10_000)
        .await?;
    Ok(all
        .into_iter()
        .filter(|c| c.session_id.as_deref() == Some(session_id))
        .collect())
}

/// Tool error for a rejected argument.
fn invalid_argument(message: impl std::fmt::Display) -> anyhow::Result<Value> {
    super::server::mcp_text_result(format!("Error: invalid argument: {message}"), true)
}

/// A required, non-blank string argument.
fn required_text<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, String> {
    match arguments.get(key).and_then(Value::as_str) {
        Some(value) if !value.trim().is_empty() => Ok(value),
        _ => Err(format!("{key} must be a non-empty string")),
    }
}

/// An optional positive integer argument; absent or null yields `None`.
fn optional_positive_int(arguments: &Value, key: &str) -> Result<Option<u64>, String> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => match value.as_u64() {
            Some(n) if n > 0 => Ok(Some(n)),
            _ => Err(format!("{key} must be a positive integer")),
        },
    }
}

/// Canonical repository root: no `..` segments, must be an existing directory.
fn resolve_repo_root(repo_path: &str) -> Result<std::path::PathBuf, String> {
    if Path::new(repo_path)
        .components()
        .any(|c| c == std::path::Component::ParentDir)
    {
        return Err("repo_path must not contain '..' segments".to_string());
    }
    match std::fs::canonicalize(repo_path) {
        Ok(path) if path.is_dir() => Ok(path),
        Ok(_) => Err("repo_path must be a directory".to_string()),
        Err(e) => Err(format!("repo_path cannot be resolved: {e}")),
    }
}

/// Build an invoked/completed ack event from MCP arguments (opaque ids only).
fn build_ack_event(arguments: &Value) -> Result<UsageEvent, String> {
    use sha2::Digest;
    let text = |key: &str| {
        arguments
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    let stage = match text("stage").as_deref() {
        Some("invoked") => SkillStage::Invoked,
        Some("completed") => SkillStage::Completed,
        _ => return Err("'stage' must be 'invoked' or 'completed'".to_string()),
    };
    let (Some(skill_name), Some(task_id)) = (text("skill_name"), text("task_id")) else {
        return Err("'skill_name' and 'task_id' are required".to_string());
    };
    // D12: opaque, bounded identifiers only; no free text reaches the telemetry DB.
    let opaque = |v: &str, max: usize| {
        !v.is_empty()
            && v.len() <= max
            && v.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.:/".contains(c))
    };
    if !opaque(&skill_name, 128) || !opaque(&task_id, 128) {
        return Err(
            "'skill_name' and 'task_id' must be opaque ids ([A-Za-z0-9-_.:/], max 128)".to_string(),
        );
    }
    if let Some(outcome) = text("outcome") {
        if !(1..=32).contains(&outcome.len())
            || !outcome.chars().all(|c| c.is_ascii_lowercase() || c == '_')
        {
            return Err("'outcome' must be a short label ([a-z_], max 32)".to_string());
        }
    }
    let event_id = crate::crypto::hex_encode(sha2::Sha256::digest(
        format!("{task_id}:{skill_name}:{}", stage.as_str()).as_bytes(),
    ));
    let mut event = UsageEvent::new(
        event_id,
        text("project").unwrap_or_else(|| "default".to_string()),
        task_id,
        text("tool").unwrap_or_else(|| "mcp-harness".to_string()),
        skill_name,
        stage,
    );
    if let Some(outcome) = text("outcome") {
        event.outcome = outcome;
    }
    Ok(event)
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
            let session_id = match required_text(&arguments, "session_id") {
                Ok(session_id) => session_id,
                Err(message) => return invalid_argument(message),
            };
            if let Err(message) = validate_session_id(session_id) {
                return invalid_argument(message);
            }
            let name = arguments.get("name").and_then(|v| v.as_str());
            if name.is_some_and(|n| n.chars().count() > 200) {
                return invalid_argument("name must be at most 200 chars");
            }

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
            let session_id = match required_text(&arguments, "session_id") {
                Ok(session_id) => session_id,
                Err(message) => return invalid_argument(message),
            };
            if let Err(message) = validate_session_id(session_id) {
                return invalid_argument(message);
            }
            let depth = match arguments.get("depth") {
                None | Some(Value::Null) => "medium",
                Some(value) => match value.as_str() {
                    Some(depth @ ("shallow" | "medium" | "deep")) => depth,
                    _ => return invalid_argument("depth must be one of shallow, medium, deep"),
                },
            };

            let (level, token_budget) = match depth {
                "shallow" => (ContextLevel::Minimal, 150),
                "deep" => (ContextLevel::Maximum, 2400),
                _ => (ContextLevel::Medium, 800),
            };

            // Cheap, bounded existence check first: unknown sessions must not
            // reach the expensive fallback below.
            match tokio::time::timeout(
                SESSION_LOOKUP_TIMEOUT,
                workspace.workspace.session_has_context(session_id),
            )
            .await
            {
                Ok(Ok(true)) => {}
                Ok(Ok(false)) => {
                    return super::server::mcp_text_result(
                        format!("no saved context for session {}", session_id),
                        true,
                    );
                }
                Ok(Err(e)) => {
                    return super::server::mcp_text_result(
                        format!("Error: context store unavailable for session {session_id}: {e}"),
                        true,
                    );
                }
                Err(_) => {
                    return super::server::mcp_text_result(
                        format!(
                            "Error: session lookup for {session_id} timed out after {}s",
                            SESSION_LOOKUP_TIMEOUT.as_secs()
                        ),
                        true,
                    );
                }
            }

            // Fetch session history
            let messages = match workspace
                .workspace
                .conversations_db
                .get_thread_messages(session_id)
                .await
            {
                Ok(messages) => messages,
                Err(e) => {
                    return super::server::mcp_text_result(
                        format!("Error: context store unavailable for session {session_id}: {e}"),
                        true,
                    );
                }
            };

            let own_ids: std::collections::HashSet<String> =
                messages.iter().map(|m| m.id.clone()).collect();
            let has_messages = !messages.is_empty();
            let checkpoints = match session_checkpoints(&workspace, session_id).await {
                Ok(c) => c,
                Err(e) => {
                    return super::server::mcp_text_result(
                        format!("Error: context store unavailable for session {session_id}: {e}"),
                        true,
                    );
                }
            };

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
            let mut selected_docs = orchestrator.execute(&plan, &context_docs, session_id).await;
            // Only the session's own messages count as its history.
            selected_docs.retain(|d| own_ids.contains(&d.id));

            // Build optimized context
            let builder_config = ContextBuilderConfig::default();
            let builder = ContextBuilder::new(builder_config);
            let mut context_string = builder.build(level, &selected_docs, &[], &[]);
            if !has_messages {
                context_string
                    .push_str("# Recent Messages\n(no messages recorded for this session)\n");
            }

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
                "checkpoints": checkpoints
                    .iter()
                    .take(20)
                    .map(|c| json!({
                        "id": c.id,
                        "name": c.name,
                        "created_at": c.created_at.to_rfc3339(),
                    }))
                    .collect::<Vec<_>>(),
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
            let session_id = match arguments.get("session_id") {
                None | Some(Value::Null) => None,
                Some(Value::String(sid)) => Some(sid.as_str()),
                Some(_) => return invalid_argument("session_id must be a string"),
            };
            let limit = match optional_positive_int(&arguments, "limit") {
                Ok(limit) => limit.unwrap_or(CONTEXT_SEARCH_DEFAULT_LIMIT),
                Err(message) => return invalid_argument(message),
            };
            if limit > CONTEXT_SEARCH_MAX_LIMIT {
                return invalid_argument(format!(
                    "limit must be between 1 and {CONTEXT_SEARCH_MAX_LIMIT}"
                ));
            }

            let filters = session_id.map(|sid| MemoryQueryFilters {
                session_id: Some(sid.to_string()),
                ..Default::default()
            });

            let results = workspace
                .workspace
                .memory
                .search_filtered(query, limit as usize, filters.as_ref())
                .await?;

            let needle = query.trim().to_lowercase();
            let checkpoint_hits: Vec<_> = if needle.is_empty() {
                Vec::new()
            } else {
                workspace
                    .workspace
                    .conversations_db
                    .list_checkpoints(10_000)
                    .await?
                    .into_iter()
                    .filter(|c| session_id.is_none_or(|sid| c.session_id.as_deref() == Some(sid)))
                    .filter(|c| {
                        c.id.to_lowercase().contains(&needle)
                            || c.name
                                .as_deref()
                                .is_some_and(|n| n.to_lowercase().contains(&needle))
                    })
                    .take(limit as usize)
                    .collect()
            };

            if results.is_empty() && checkpoint_hits.is_empty() {
                return super::server::mcp_text_result(
                    format!("No context found for query: {}", query),
                    false,
                );
            }

            let mut output = String::new();
            for c in &checkpoint_hits {
                output.push_str(&format!(
                    "--- Checkpoint ---\nID: {}\nName: {}\nSession: {}\nCreated: {}\n\n",
                    c.id,
                    c.name.as_deref().unwrap_or(""),
                    c.session_id.as_deref().unwrap_or(""),
                    c.created_at.to_rfc3339()
                ));
            }
            for (i, doc) in results.iter().enumerate() {
                let snippet = if doc.content.len() > 200 {
                    format!(
                        "{}...",
                        crate::memory::snippet::clip_chars(&doc.content, 200)
                    )
                } else {
                    doc.content.clone()
                };
                let meta_preview = match super::metadata_redact::public_metadata(&doc.metadata) {
                    Value::Object(m) => {
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

            let result = crate::kernel::runner::execute_rtk_command_bounded(
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
            let title = match required_text(&arguments, "title") {
                Ok(title) => title,
                Err(message) => return invalid_argument(message),
            };
            let Some(body) = arguments.get("body").and_then(Value::as_str) else {
                return invalid_argument("body must be a string");
            };
            let repo = arguments
                .get("repo")
                .and_then(|v| v.as_str())
                .unwrap_or("owner/repo");
            let repo_path_str = arguments
                .get("repo_path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");

            let repo_root = match resolve_repo_root(repo_path_str) {
                Ok(root) => root,
                Err(message) => return invalid_argument(message),
            };
            let repo_root = repo_root.as_path();
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
            let max_tokens = match optional_positive_int(&arguments, "max_tokens") {
                Ok(max_tokens) => max_tokens.map_or(4000, |v| v as usize),
                Err(message) => return invalid_argument(message),
            };
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
        "xavier_skill_ack" => {
            let event = match build_ack_event(&arguments) {
                Ok(event) => event,
                Err(error) => {
                    return super::server::mcp_text_result(
                        serde_json::to_string(&json!({"ok": false, "error": error}))?,
                        true,
                    )
                }
            };
            let outcome = tokio::task::spawn_blocking(move || {
                if let Some(parent) = crate::api::skills::skill_usage_db_path().parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                crate::context::skill_dispatcher::record_ack(
                    &crate::api::skills::skill_usage_db_path(),
                    &event,
                )
            })
            .await?;
            match outcome {
                Ok(outcome) => super::server::mcp_text_result(
                    serde_json::to_string(
                        &json!({"ok": true, "recorded": outcome.applied, "affected": outcome.affected}),
                    )?,
                    false,
                ),
                Err(error) => super::server::mcp_text_result(
                    serde_json::to_string(&json!({"ok": false, "error": error.to_string()}))?,
                    true,
                ),
            }
        }
        "xavier_skill_list" => {
            let registry = build_skill_registry(Path::new(&workspace.workspace_id)).await;
            let (visible, _) = crate::security::clearance::split_by_clearance(
                super::tools_memory::caller_clearance(),
                registry.list(),
                |name| registry.clearance_of(name),
            );
            let skills: Vec<Value> = visible
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
                "---\nname: {name}\ndescription: \"{description}\"\nvisibility: public\n---\n\n# {name}\n\nFixture body.\n"
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
        crate::isolate_test_process!();
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
        let response =
            crate::api::skills::list_skills(axum::extract::Extension(workspace.clone()), None)
                .await
                .into_response();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("rest body");
        serde_json::from_slice(&body).expect("rest payload is JSON")
    }

    const CONFIDENTIAL_SKILL: &str = "confidential-mesh-design";
    const CONFIDENTIAL_DESCRIPTION: &str = "Confidential distributed mesh consensus architecture";
    const CONFIDENTIAL_TASK: &str = "distributed mesh consensus architecture";

    fn write_confidential_skill(root: &Path) {
        write_fixture_skill(root, CONFIDENTIAL_SKILL, CONFIDENTIAL_DESCRIPTION);
        let path = root
            .join("skills")
            .join(CONFIDENTIAL_SKILL)
            .join("SKILL.md");
        let bytes = std::fs::read_to_string(&path).expect("fixture bytes");
        std::fs::write(
            path,
            bytes.replace("visibility: public", "clearance: confidential"),
        )
        .expect("confidential fixture");
    }

    async fn skill_session_call(
        state: &crate::AppState,
        workspace: &WorkspaceContext,
        role: crate::security::auth::UserRole,
        name: &str,
        arguments: Value,
    ) -> Value {
        let claims = crate::security::auth::Claims::new(
            "skill-fixture".into(),
            "skill-fixture@example.invalid".into(),
            role,
            chrono::Duration::minutes(5),
        );
        // HTTP establishes this identity before entering the JSON-RPC dispatcher.
        let output = super::super::tools_memory::with_mcp_caller(
            super::super::tools_memory::McpCaller {
                space: None,
                clearance: crate::security::clearance::role_clearance(role),
            },
            super::super::session::dispatch_mcp_value(
                state.clone(),
                workspace.clone(),
                Some(&claims),
                json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                    "params": {"name": name, "arguments": arguments}}),
            ),
        )
        .await
        .expect("session dispatch")
        .expect("session response");
        assert!(output.get("error").is_none(), "{output}");
        assert_eq!(output["result"]["isError"], false, "{output}");
        mcp_payload(&output["result"])
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn skill_clearance_mcp_session_list_filters_low_role() {
        crate::isolate_test_process!();
        use crate::security::auth::UserRole;

        let _home = IsolatedHome::acquire().await;
        let work = tempfile::tempdir().expect("workspace");
        write_confidential_skill(work.path());
        write_fixture_skill(work.path(), "public-reference", "Public fixture reference");
        let (state, mut workspace) = crate::server::mcp::tests::test_state().await;
        workspace.workspace_id = work.path().to_string_lossy().into_owned();

        let low = skill_session_call(
            &state,
            &workspace,
            UserRole::Readonly,
            "xavier_skill_list",
            json!({}),
        )
        .await;
        assert_eq!(sorted_skill_names(&low), vec!["public-reference"]);
        assert_eq!(low["count"], 1);
        assert!(!low.to_string().contains(CONFIDENTIAL_SKILL));
        assert!(!low.to_string().contains(CONFIDENTIAL_DESCRIPTION));
        for role in [UserRole::User, UserRole::Admin] {
            let allowed =
                skill_session_call(&state, &workspace, role, "xavier_skill_list", json!({})).await;
            assert_eq!(allowed["count"], 2);
            assert!(sorted_skill_names(&allowed).contains(&CONFIDENTIAL_SKILL.to_string()));
            assert!(allowed.to_string().contains(CONFIDENTIAL_DESCRIPTION));
        }
        let local = crate::server::mcp_stdio::dispatch_stdio_value(
            state,
            workspace,
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": "xavier_skill_list", "arguments": {}}}),
        )
        .await
        .expect("local dispatch")
        .expect("local response");
        assert!(local.get("error").is_none(), "{local}");
        assert_eq!(mcp_payload(&local["result"])["count"], 2);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn skill_clearance_mcp_session_dispatch_filters_low_role() {
        crate::isolate_test_process!();
        use crate::security::auth::UserRole;

        let _home = IsolatedHome::acquire().await;
        let work = tempfile::tempdir().expect("workspace");
        write_confidential_skill(work.path());
        let (state, mut workspace) = crate::server::mcp::tests::test_state().await;
        workspace.workspace_id = work.path().to_string_lossy().into_owned();
        let arguments = json!({"task": CONFIDENTIAL_TASK});
        let low = skill_session_call(
            &state,
            &workspace,
            UserRole::Readonly,
            "xavier_dispatch_skill",
            arguments.clone(),
        )
        .await;
        assert_eq!(low["skill_name"], "_none");
        assert_eq!(low["hidden_by_clearance"], 1);
        assert_eq!(low["context_pack"]["system_instructions"], "");
        assert_eq!(low["context_pack"]["composition"], "");
        assert!(!low.to_string().contains(CONFIDENTIAL_DESCRIPTION));
        for role in [UserRole::User, UserRole::Admin] {
            let allowed = skill_session_call(
                &state,
                &workspace,
                role,
                "xavier_dispatch_skill",
                arguments.clone(),
            )
            .await;
            assert_eq!(allowed["skill_name"], CONFIDENTIAL_SKILL);
            assert_eq!(allowed["hidden_by_clearance"], 0);
            assert!(!allowed["context_pack"]["system_instructions"]
                .as_str()
                .unwrap()
                .is_empty());
        }
        let local = crate::server::mcp_stdio::dispatch_stdio_value(
            state,
            workspace,
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": "xavier_dispatch_skill", "arguments": arguments}}),
        )
        .await
        .expect("local dispatch")
        .expect("local response");
        assert!(local.get("error").is_none(), "{local}");
        assert_eq!(
            mcp_payload(&local["result"])["skill_name"],
            CONFIDENTIAL_SKILL
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn skill_clearance_mcp_missing_identity_defaults_unclassified() {
        crate::isolate_test_process!();

        let _home = IsolatedHome::acquire().await;
        let work = tempfile::tempdir().expect("workspace");
        write_confidential_skill(work.path());
        let (state, mut workspace) = crate::server::mcp::tests::test_state().await;
        workspace.workspace_id = work.path().to_string_lossy().into_owned();
        let listing = handle_context_tool(
            state.clone(),
            workspace.clone(),
            "xavier_skill_list",
            json!({}),
        )
        .await
        .expect("anonymous list");
        assert_eq!(mcp_payload(&listing)["count"], 0);
        let dispatch = handle_context_tool(
            state.clone(),
            workspace.clone(),
            "xavier_dispatch_skill",
            json!({"task": CONFIDENTIAL_TASK}),
        )
        .await
        .expect("anonymous dispatch");
        assert_eq!(mcp_payload(&dispatch)["skill_name"], "_none");
        assert_eq!(mcp_payload(&dispatch)["hidden_by_clearance"], 1);
        let anonymous = super::super::session::dispatch_mcp_value(
            state,
            workspace,
            None,
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": {"name": "xavier_skill_list", "arguments": {}}}),
        )
        .await
        .expect("anonymous session")
        .expect("anonymous response");
        assert!(anonymous.get("error").is_some(), "{anonymous}");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_mcp_skill_list_matches_rest_on_same_fixture() {
        crate::isolate_test_process!();
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
        crate::isolate_test_process!();
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
        crate::isolate_test_process!();
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

    fn tool_text(out: &Value) -> &str {
        out["content"][0]["text"]
            .as_str()
            .expect("mcp text content")
    }

    fn assert_tool_error(out: &Value, needle: &str) {
        assert_eq!(out["isError"], true, "{out}");
        assert!(tool_text(out).contains(needle), "{out}");
    }

    /// A workspace whose memory holds exactly `documents`.
    async fn workspace_with_documents(
        documents: Vec<crate::memory::qmd_memory::MemoryDocument>,
    ) -> (crate::AppState, WorkspaceContext, tempfile::TempDir) {
        use std::sync::Arc;
        let work = tempfile::tempdir().expect("workspace dir");
        let (state, mut workspace) = crate::server::mcp::tests::test_state().await;
        let memory = Arc::new(crate::memory::qmd_memory::QmdMemory::new_with_workspace(
            Arc::new(tokio::sync::RwLock::new(documents)),
            "ws-context",
        ));
        let store = Arc::new(crate::memory::store::InMemoryMemoryStore::default());
        workspace.workspace = Arc::new(
            crate::workspace::WorkspaceState::new_minimal(
                "ws-context".to_string(),
                work.path().to_path_buf(),
                memory,
                store,
            )
            .await,
        );
        (state, workspace, work)
    }

    fn session_document(
        id: &str,
        session: &str,
        metadata: Value,
    ) -> crate::memory::qmd_memory::MemoryDocument {
        crate::memory::qmd_memory::MemoryDocument {
            id: Some(id.to_string()),
            path: format!("sessions/{session}/{id}"),
            content: format!("context window checkpoint notes {id}"),
            metadata,
            ..Default::default()
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn restore_unknown_session_fast_error() {
        crate::isolate_test_process!();
        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        let started = std::time::Instant::now();
        let out = handle_context_tool(
            state.clone(),
            workspace,
            "xavier_context_restore",
            json!({"session_id": "never-saved-session"}),
        )
        .await
        .expect("unknown session is a tool error");
        assert_tool_error(&out, "no saved context for session never-saved-session");
        assert!(started.elapsed() < std::time::Duration::from_secs(10));

        // A conversation store that cannot serve queries is reported as a tool
        // error, not a protocol error.
        let (_, mut workspace, work) = workspace_with_documents(Vec::new()).await;
        let mut inner = crate::workspace::WorkspaceState::new_minimal(
            "ws-unavailable".to_string(),
            work.path().to_path_buf(),
            std::sync::Arc::clone(&workspace.workspace.memory),
            std::sync::Arc::new(crate::memory::store::InMemoryMemoryStore::default()),
        )
        .await;
        inner.conversations_db = std::sync::Arc::new(
            crate::codebase::conversations_db::ConversationsDb::open_disabled("unavailable")
                .expect("disabled handle"),
        );
        workspace.workspace = std::sync::Arc::new(inner);
        let out = handle_context_tool(
            state,
            workspace,
            "xavier_context_restore",
            json!({"session_id": "never-saved-session"}),
        )
        .await
        .expect("store failure is a tool error");
        assert_tool_error(&out, "context store unavailable");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn restore_rejects_unknown_depth() {
        crate::isolate_test_process!();
        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        for depth in [json!("extreme"), json!(3), json!("")] {
            let out = handle_context_tool(
                state.clone(),
                workspace.clone(),
                "xavier_context_restore",
                json!({"session_id": "any-session", "depth": depth}),
            )
            .await
            .expect("bad depth is a tool error");
            assert_tool_error(&out, "depth must be one of shallow, medium, deep");
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn save_rejects_empty_session_id() {
        crate::isolate_test_process!();
        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        for arguments in [
            json!({}),
            json!({"session_id": ""}),
            json!({"session_id": "   "}),
            json!({"session_id": 42}),
        ] {
            let out = handle_context_tool(
                state.clone(),
                workspace.clone(),
                "xavier_context_save",
                arguments.clone(),
            )
            .await
            .expect("bad session_id is a tool error");
            assert_tool_error(&out, "session_id must be a non-empty string");
        }
        let out = handle_context_tool(
            state,
            workspace,
            "xavier_context_save",
            json!({"session_id": "valid-session"}),
        )
        .await
        .expect("valid save");
        assert_eq!(out["isError"], false, "{out}");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn context_session_semantics_handler_level() {
        crate::isolate_test_process!();
        let (state, workspace, _work) = workspace_with_documents(Vec::new()).await;
        let call = |name: &'static str, args: Value| {
            let (s, w) = (state.clone(), workspace.clone());
            async move {
                handle_context_tool(s, w, name, args)
                    .await
                    .expect("tool result")
            }
        };
        for bad in ["../../x", "a..b", &"s".repeat(129), "bad id"] {
            let out = call("xavier_context_save", json!({"session_id": bad})).await;
            assert_tool_error(&out, "invalid argument");
            let out = call("xavier_context_restore", json!({"session_id": bad})).await;
            assert_tool_error(&out, "invalid argument");
        }
        let out = call(
            "xavier_context_save",
            json!({"session_id": "s1", "name": "n".repeat(201)}),
        )
        .await;
        assert_tool_error(&out, "name must be at most 200");

        for (sid, name) in [("sess-a", "first"), ("sess-b", "second")] {
            let out = call(
                "xavier_context_save",
                json!({"session_id": sid, "name": name}),
            )
            .await;
            assert_eq!(out["isError"], false, "{out}");
        }
        for (sid, mine, other) in [("sess-a", "first", "second"), ("sess-b", "second", "first")] {
            let out = call("xavier_context_restore", json!({"session_id": sid})).await;
            assert_eq!(out["isError"], false, "{out}");
            let payload: Value = serde_json::from_str(tool_text(&out)).expect("json");
            let cps = payload["checkpoints"].as_array().expect("checkpoints");
            assert_eq!(cps.len(), 1, "{payload}");
            assert_eq!(cps[0]["name"], mine);
            assert!(!payload.to_string().contains(other));
            assert!(payload["context"]
                .as_str()
                .unwrap()
                .contains("no messages recorded for this session"));
        }

        let out = call("xavier_context_search", json!({"query": "first"})).await;
        assert_eq!(out["isError"], false, "{out}");
        assert!(tool_text(&out).contains("Name: first"), "{out}");
        let out = call(
            "xavier_context_search",
            json!({"query": "first", "session_id": "sess-b"}),
        )
        .await;
        assert!(!tool_text(&out).contains("Name: first"), "{out}");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn dispatch_rejects_bad_max_tokens() {
        crate::isolate_test_process!();
        let _home = IsolatedHome::acquire().await;
        let work = tempfile::tempdir().expect("workspace");
        let (state, mut workspace) = crate::server::mcp::tests::test_state().await;
        workspace.workspace_id = work.path().to_string_lossy().into_owned();
        for max_tokens in [json!(0), json!(-1), json!(1.5), json!("4000"), json!(true)] {
            let out = handle_context_tool(
                state.clone(),
                workspace.clone(),
                "xavier_dispatch_skill",
                json!({"task": "summarize notes", "max_tokens": max_tokens}),
            )
            .await
            .expect("bad max_tokens is a tool error");
            assert_tool_error(&out, "max_tokens must be a positive integer");
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn issue_context_rejects_bad_repo_path() {
        crate::isolate_test_process!();
        let (state, workspace) = crate::server::mcp::tests::test_state().await;
        let root = tempfile::tempdir().expect("repo root");
        let file = root.path().join("file.txt");
        std::fs::write(&file, "x").expect("fixture file");
        let base = json!({"issue_id": "1", "title": "Title", "body": "Body"});
        let with = |key: &str, value: Value| {
            let mut arguments = base.clone();
            arguments[key] = value;
            arguments
        };
        let cases = [
            (
                with(
                    "repo_path",
                    json!(format!("{}/sub/../..", root.path().display())),
                ),
                "repo_path must not contain '..' segments",
            ),
            (
                with("repo_path", json!(file.to_string_lossy())),
                "repo_path must be a directory",
            ),
            (
                with(
                    "repo_path",
                    json!(root.path().join("missing").to_string_lossy()),
                ),
                "repo_path cannot be resolved",
            ),
            (
                json!({"issue_id": "1", "body": "Body", "repo_path": root.path()}),
                "title must be a non-empty string",
            ),
            (
                json!({"issue_id": "1", "title": "Title", "repo_path": root.path()}),
                "body must be a string",
            ),
        ];
        for (arguments, needle) in cases {
            let out = handle_context_tool(
                state.clone(),
                workspace.clone(),
                "xavier_issue_context_package",
                arguments.clone(),
            )
            .await
            .unwrap_or_else(|e| panic!("{arguments} must be a tool error, got {e}"));
            assert_tool_error(&out, needle);
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn context_search_output_has_no_internal_metadata() {
        crate::isolate_test_process!();
        let (state, workspace, _work) = workspace_with_documents(vec![session_document(
            "doc-1",
            "s-1",
            json!({
                "content_iv": "iv-fixture-value",
                "encrypted_dek": "dek-fixture-value",
                "session_id": "s-1",
                "title": "checkpoint",
            }),
        )])
        .await;
        let out = handle_context_tool(
            state,
            workspace,
            "xavier_context_search",
            json!({"query": "checkpoint notes"}),
        )
        .await
        .expect("search");
        assert_eq!(out["isError"], false, "{out}");
        let text = tool_text(&out);
        assert!(text.contains("--- Result 1 ---"), "{text}");
        assert!(text.contains("title"), "{text}");
        for internal in [
            "content_iv",
            "encrypted_dek",
            "iv-fixture-value",
            "dek-fixture-value",
        ] {
            assert!(!text.contains(internal), "{internal} leaked: {text}");
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn context_search_rejects_non_string_session_id() {
        crate::isolate_test_process!();
        let (state, workspace, _work) = workspace_with_documents(vec![
            session_document("doc-a", "s-a", json!({"session_id": "s-a"})),
            session_document("doc-b", "s-b", json!({"session_id": "s-b"})),
        ])
        .await;
        for session_id in [json!(42), json!(["s-a"]), json!({"id": "s-a"})] {
            let out = handle_context_tool(
                state.clone(),
                workspace.clone(),
                "xavier_context_search",
                json!({"query": "checkpoint notes", "session_id": session_id}),
            )
            .await
            .expect("bad session_id is a tool error");
            assert_tool_error(&out, "session_id must be a string");
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn context_search_limit_bounds() {
        crate::isolate_test_process!();
        let documents = (0..3)
            .map(|i| session_document(&format!("doc-{i}"), "s-1", json!({"session_id": "s-1"})))
            .collect();
        let (state, workspace, _work) = workspace_with_documents(documents).await;
        for limit in [json!(0), json!(51), json!(-1), json!(2.5), json!("5")] {
            let out = handle_context_tool(
                state.clone(),
                workspace.clone(),
                "xavier_context_search",
                json!({"query": "checkpoint notes", "limit": limit}),
            )
            .await
            .expect("bad limit is a tool error");
            assert_eq!(out["isError"], true, "{limit}: {out}");
            assert!(tool_text(&out).contains("limit must be"), "{out}");
        }
        let out = handle_context_tool(
            state.clone(),
            workspace.clone(),
            "xavier_context_search",
            json!({"query": "checkpoint notes", "limit": 1}),
        )
        .await
        .expect("bounded search");
        assert_eq!(out["isError"], false, "{out}");
        assert!(tool_text(&out).contains("--- Result 1 ---"), "{out}");
        assert!(!tool_text(&out).contains("--- Result 2 ---"), "{out}");
        let out = handle_context_tool(
            state,
            workspace,
            "xavier_context_search",
            json!({"query": "checkpoint notes", "limit": 50}),
        )
        .await
        .expect("maximum limit");
        assert!(tool_text(&out).contains("--- Result 3 ---"), "{out}");
    }

    #[test]
    fn skill_ack_rejects_free_text_and_unknown_stage() {
        use super::build_ack_event;
        let ok = json!({"skill_name": "trace-audit", "stage": "invoked", "task_id": "t-1"});
        let first = build_ack_event(&ok).expect("valid ack");
        assert_eq!(first.event_id, build_ack_event(&ok).unwrap().event_id);
        assert_eq!(first.workspace_id, "default");

        for bad in [
            json!({"skill_name": "trace-audit", "stage": "selected", "task_id": "t-1"}),
            json!({"skill_name": "trace-audit", "stage": "invoked"}),
            json!({"skill_name": "trace-audit", "stage": "invoked", "task_id": "fix the login bug please"}),
            json!({"skill_name": "", "stage": "invoked", "task_id": "t-1"}),
            json!({"skill_name": "trace-audit", "stage": "invoked", "task_id": "t-1", "outcome": "Free Text!"}),
        ] {
            assert!(build_ack_event(&bad).is_err(), "accepted {bad}");
        }
    }
}
