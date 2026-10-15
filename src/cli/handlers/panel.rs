//! Panel handlers for chat processing and thread management.

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::Response,
    Json,
};

use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use xavier::agents::{runtime::SystemTimings, ui_render::UiRenderAgent};
use xavier::server::panel::{CreateThreadRequest, PanelChatRequest, PanelChatResponse};

/// Panel list threads.
pub async fn panel_list_threads(State(state): State<CliState>) -> Response {
    match state.panel_store.list_threads(50).await {
        Ok(threads) => {
            let mut summaries = Vec::new();
            for t in threads {
                let mut summary = xavier::codebase::conversations_db::ThreadSummary::from(&t);
                if let Ok(messages) = state.panel_store.get_thread_messages(&t.id).await {
                    summary.message_count = messages.len();
                }
                summaries.push(summary);
            }
            json_response(StatusCode::OK, serde_json::json!(summaries))
        }
        Err(error) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": error.to_string() }),
        ),
    }
}

/// Panel create thread.
pub async fn panel_create_thread(
    State(state): State<CliState>,
    Json(payload): Json<CreateThreadRequest>,
) -> Response {
    let title_hint = payload
        .title
        .or(payload.message)
        .unwrap_or_else(|| "New Thread".to_string());

    match state
        .panel_store
        .create_thread(Some(&title_hint), None, Some("cli"))
        .await
    {
        Ok(thread) => json_response(
            StatusCode::OK,
            serde_json::to_value(xavier::codebase::conversations_db::ThreadSummary::from(
                &thread,
            ))
            .unwrap_or_else(|_| serde_json::json!({})),
        ),
        Err(error) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": error.to_string() }),
        ),
    }
}

/// Panel get thread.
pub async fn panel_get_thread(
    State(state): State<CliState>,
    AxumPath(thread_id): AxumPath<String>,
) -> Response {
    match state.panel_store.get_thread(&thread_id).await {
        Ok(Some(thread)) => match state.panel_store.get_thread_messages(&thread_id).await {
            Ok(messages) => {
                let mut summary = xavier::codebase::conversations_db::ThreadSummary::from(&thread);
                summary.message_count = messages.len();
                json_response(
                    StatusCode::OK,
                    serde_json::to_value(xavier::codebase::conversations_db::ThreadDetail {
                        thread: summary,
                        messages,
                    })
                    .unwrap_or_else(|_| serde_json::json!({})),
                )
            }
            Err(error) => json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({ "error": error.to_string() }),
            ),
        },
        Ok(None) => json_response(
            StatusCode::NOT_FOUND,
            serde_json::json!({ "error": "thread not found" }),
        ),
        Err(error) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": error.to_string() }),
        ),
    }
}

/// Panel delete thread.
pub async fn panel_delete_thread(
    State(state): State<CliState>,
    AxumPath(thread_id): AxumPath<String>,
) -> Response {
    match state.panel_store.delete_thread(&thread_id).await {
        Ok(true) => axum::response::IntoResponse::into_response(StatusCode::NO_CONTENT),
        Ok(false) => json_response(
            StatusCode::NOT_FOUND,
            serde_json::json!({ "error": "thread not found" }),
        ),
        Err(error) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": error.to_string() }),
        ),
    }
}

/// Panel process chat.
pub async fn panel_process_chat(
    State(state): State<CliState>,
    Json(payload): Json<PanelChatRequest>,
) -> Response {
    match panel_process_chat_inner(&state, payload).await {
        Ok(response) => json_response(
            StatusCode::OK,
            serde_json::to_value(response).unwrap_or_else(|_| serde_json::json!({})),
        ),
        Err(error) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": error.to_string() }),
        ),
    }
}

/// Panel process chat inner.
pub async fn panel_process_chat_inner(
    state: &CliState,
    payload: PanelChatRequest,
) -> anyhow::Result<PanelChatResponse> {
    let thread = match payload.thread_id.as_deref() {
        Some(thread_id) => state
            .panel_store
            .get_thread(thread_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("thread {thread_id} not found"))?,
        None => {
            state
                .panel_store
                .create_thread(Some(&payload.message), None, Some("cli"))
                .await?
        }
    };

    state
        .panel_store
        .store_message(
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

    let started = std::time::Instant::now();

    // Real LLM call via ProxyUseCase (issue #590). On failure, fall back to memory search.
    let cmd = xavier::domain::proxy::ProxyChatCommand {
        model: "auto".to_string(),
        messages: vec![serde_json::json!({
            "role": "user",
            "content": payload.message.trim()
        })],
        temperature: None,
        max_tokens: None,
        lease_token: None,
    };

    let (assistant_content, used_fallback) = match state
        .proxy_use_case
        .execute_secured(
            cmd,
            false, // panel sessions are durable, not ephemeral
            state.secrets_engine.clone(),
            state.event_bus.clone(),
        )
        .await
    {
        Ok(resp) => {
            let content = resp
                .choices
                .first()
                .map(|c| c.message.content.clone())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "(respuesta vacía del LLM)".to_string());
            (content, false)
        }
        Err(e) => {
            tracing::warn!("Panel chat LLM error, falling back to memory: {}", e);
            state.usage_counters.record_memory_fallback();
            let query = payload.message.trim();
            match state.memory.search(query, 5, None).await {
                Ok(results) if !results.is_empty() => {
                    let context = results
                        .iter()
                        .map(|r| r.content.as_str())
                        .collect::<Vec<_>>()
                        .join("\n---\n");
                    (
                        format!("[Modo memoria - LLM no disponible]\n\n{}", context),
                        true,
                    )
                }
                _ => (format!("[LLM no disponible: {}]", e), true),
            }
        }
    };

    // The proxy supplies an answer without runtime reasoning/retrieval signals.
    let ui_render = UiRenderAgent::new().render_answer(&assistant_content);
    let timings = SystemTimings {
        system1_ms: 0,
        system2_ms: 0,
        system3_ms: 0,
        total_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    };
    let metadata = serde_json::json!({
        "rules": ui_render.rules_applied,
        "components": ui_render.components,
        "source": if used_fallback { "memory-fallback" } else { "llm" },
        "timings": timings,
    });

    state
        .panel_store
        .store_message(
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

    let messages = state.panel_store.get_thread_messages(&thread.id).await?;
    let mut summary = xavier::codebase::conversations_db::ThreadSummary::from(&thread);
    summary.message_count = messages.len();

    Ok(PanelChatResponse {
        thread: summary,
        messages,
    })
}
