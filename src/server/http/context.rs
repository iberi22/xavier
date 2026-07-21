// SPDX-License-Identifier: MIT OR LICENSE-MESH
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

pub async fn v1_context_regenerate(
    Extension(workspace): Extension<WorkspaceContext>,
    Extension(_state): Extension<crate::AppState>,
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

    // 3. Build optimized context
    let builder_config = ContextBuilderConfig::default();
    let builder = ContextBuilder::new(builder_config);

    // For now, memories and skills are empty as they'll be integrated later in fusion step
    let context_string = builder.build(level, &selected_docs, &[], &[]);
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

pub async fn v1_context_deepen(
    Extension(workspace): Extension<WorkspaceContext>,
    Extension(_state): Extension<crate::AppState>,
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

pub async fn v1_context_stats() -> impl IntoResponse {
    let stats = TRACKER.get_stats().await;
    Json(stats).into_response()
}
