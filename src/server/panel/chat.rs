use super::types::{PanelChatRequest, PanelChatResponse};
use crate::codebase::conversations_db::ThreadSummary;
use crate::{
    agents::ui_render::UiRenderAgent, codebase::conversations_db::ConversationsDb,
    workspace::WorkspaceContext,
};
use axum::{response::IntoResponse, Extension, Json};
use serde_json::json;

/// Process chat.
pub async fn process_chat(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(payload): Json<PanelChatRequest>,
) -> impl IntoResponse {
    let db = std::sync::Arc::clone(&workspace.workspace.conversations_db);
    let runtime = std::sync::Arc::clone(&workspace.workspace.runtime);
    let wc = workspace.clone();
    let response =
        tokio::task::spawn(async move { process_chat_inner(db, runtime, wc, payload).await })
            .await
            .unwrap_or_else(|_| Err(anyhow::anyhow!("chat task panicked")));

    match response {
        Ok(response) => Json(response).into_response(),
        Err(error) => crate::error::ApiError::internal(error.to_string()).into_response(),
    }
}

async fn process_chat_inner(
    db: std::sync::Arc<ConversationsDb>,
    runtime: std::sync::Arc<crate::agents::AgentRuntime>,
    workspace: WorkspaceContext,
    payload: PanelChatRequest,
) -> anyhow::Result<PanelChatResponse> {
    let thread = match payload.thread_id {
        Some(thread_id) => match db.get_thread(&thread_id).await? {
            Some(thread) => thread,
            None => {
                db.create_thread(Some(&payload.message), None, Some("panel"))
                    .await?
            }
        },
        None => {
            db.create_thread(Some(&payload.message), None, Some("panel"))
                .await?
        }
    };

    db.store_message(
        &thread.id,
        "user",
        &payload.message,
        None,
        None,
        None,
        Some("{}"),
        None,
    )
    .await?;

    let trace = runtime
        .run_with_trace(&payload.message, Some(thread.id.clone()), None)
        .await?;
    workspace
        .workspace
        .record_optimization(
            trace.optimization.route_category,
            trace.optimization.semantic_cache_hit,
            trace.optimization.llm_used,
            trace.optimization.model.as_deref(),
        )
        .await?;
    let ui_render = UiRenderAgent::new().render(&trace);

    let metadata = json!({
        "confidence": trace.agent.confidence,
        "timings": trace.agent.system_timings,
        "components": ui_render.components,
        "rules": ui_render.rules_applied,
        "documents": trace.retrieval.total_results,
        "evidence": trace.reasoning.supporting_evidence.len(),
        "optimization": trace.optimization,
    });

    db.store_message(
        &thread.id,
        "assistant",
        &ui_render.plain_text,
        None,
        Some(&ui_render.openui_lang),
        None,
        Some(&metadata.to_string()),
        None,
    )
    .await?;

    workspace
        .workspace
        .record_session_exchange(&thread.id, "panel", &payload.message, &trace.agent.response)
        .await?;

    let messages = db.get_thread_messages(&thread.id).await?;
    let mut summary = ThreadSummary::from(&thread);
    summary.message_count = messages.len();

    Ok(PanelChatResponse {
        thread: summary,
        messages,
    })
}
