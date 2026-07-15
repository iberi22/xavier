//! Context-related MCP tools
//!
//! Provides tools for saving, restoring, and searching optimized context,
//! as well as reporting token savings.

use super::types::*;
use crate::context::{
    ContextBudgetConfig, ContextBuilder, ContextBuilderConfig, ContextDocument, ContextLevel,
    Orchestrator,
};
use crate::memory::schema::MemoryQueryFilters;
use crate::observability::token_accounting::TRACKER;
use crate::workspace::WorkspaceContext;
use crate::AppState;
use serde_json::{json, Value};
use std::sync::Arc;

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
    ]
}

pub async fn handle_context_tool(
    _state: AppState,
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
                "shallow" => (ContextLevel::Minimal, 50),
                "deep" => (ContextLevel::Maximum, 1000),
                _ => (ContextLevel::Medium, 200),
            };

            // Fetch session history
            let messages = workspace
                .workspace
                .conversations_db
                .get_thread_messages(session_id)
                .await?;

            let original_token_count: usize = messages
                .iter()
                .map(|m| m.tokens.unwrap_or(0) as usize)
                .sum();

            let context_docs: Vec<ContextDocument> = messages
                .into_iter()
                .map(|m| {
                    ContextDocument::new(m.id, session_id, m.role, m.content)
                        .with_token_count(m.tokens.unwrap_or(0) as usize)
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
            let optimized_token_count = context_string.split_whitespace().count();

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
                    format!("{}...", &doc.content[..200])
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
            super::server::mcp_text_result(serde_json::to_string_pretty(&stats)?, false)
        }
        _ => Err(anyhow::anyhow!("Unknown context tool: {}", name)),
    }
}
