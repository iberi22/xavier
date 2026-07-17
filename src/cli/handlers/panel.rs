//! Panel handlers for chat processing and thread management.

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::Response,
    Json,
};

use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use xavier::server::panel::{CreateThreadRequest, PanelChatRequest, PanelChatResponse};

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

pub async fn panel_delete_thread(
    State(_state): State<CliState>,
    AxumPath(_thread_id): AxumPath<String>,
) -> Response {
    json_response(
        StatusCode::NOT_IMPLEMENTED,
        serde_json::json!({ "error": "thread deletion not implemented" }),
    )
}

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

    // TODO(issue 03): memory-fallback
    // Here we would match on the result of calling the proxy. If it fails,
    // we would fall back to searching memory and generating a synthetic response
    // similar to `fallback_from_memory` in headless_api.rs.

    let assistant_content = format!("Structured Xavier response for: {}", payload.message.trim());
    let openui_lang = format!(
        "<SectionBlock title=\"Xavier\" description=\"{}\"><InfoCard title=\"Status\" value=\"Ready\" /></SectionBlock>",
        payload.message.replace('"', "'")
    );
    let metadata = serde_json::json!({
        "rules": ["deterministic", "ci-safe"],
        "components": ["SectionBlock", "InfoCard"],
        "timings": { "total_ms": 0 }
    });

    state
        .panel_store
        .store_message(
            &thread.id,
            "assistant",
            &assistant_content,
            None,
            Some(&openui_lang),
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
