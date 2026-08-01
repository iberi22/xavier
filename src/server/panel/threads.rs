use super::types::CreateThreadRequest;
use crate::{
    codebase::conversations_db::{ThreadDetail, ThreadSummary},
    workspace::WorkspaceContext,
};
use axum::{extract::Path as AxumPath, http::StatusCode, response::IntoResponse, Extension, Json};

/// List threads.
pub async fn list_threads(Extension(workspace): Extension<WorkspaceContext>) -> impl IntoResponse {
    match workspace.workspace.conversations_db.list_threads(50).await {
        Ok(threads) => {
            let mut summaries = Vec::new();
            for t in threads {
                let mut summary = ThreadSummary::from(&t);
                if let Ok(messages) = workspace
                    .workspace
                    .conversations_db
                    .get_thread_messages(&t.id)
                    .await
                {
                    summary.message_count = messages.len();
                }
                summaries.push(summary);
            }
            Json(summaries).into_response()
        }
        Err(error) => crate::error::ApiError::internal(error.to_string()).into_response(),
    }
}

/// Create thread.
pub async fn create_thread(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(payload): Json<CreateThreadRequest>,
) -> impl IntoResponse {
    let title_hint = payload
        .title
        .or(payload.message)
        .unwrap_or_else(|| "New Thread".to_string());

    match workspace
        .workspace
        .conversations_db
        .create_thread(Some(&title_hint), None, Some("panel"))
        .await
    {
        Ok(thread) => Json(ThreadSummary::from(&thread)).into_response(),
        Err(error) => crate::error::ApiError::internal(error.to_string()).into_response(),
    }
}

/// Get thread.
pub async fn get_thread(
    Extension(workspace): Extension<WorkspaceContext>,
    AxumPath(thread_id): AxumPath<String>,
) -> impl IntoResponse {
    match workspace
        .workspace
        .conversations_db
        .get_thread(&thread_id)
        .await
    {
        Ok(Some(thread)) => {
            match workspace
                .workspace
                .conversations_db
                .get_thread_messages(&thread_id)
                .await
            {
                Ok(messages) => {
                    let mut summary = ThreadSummary::from(&thread);
                    summary.message_count = messages.len();
                    Json(ThreadDetail {
                        thread: summary,
                        messages,
                    })
                    .into_response()
                }
                Err(error) => crate::error::ApiError::internal(error.to_string()).into_response(),
            }
        }
        Ok(None) => crate::error::ApiError::not_found("thread not found").into_response(),
        Err(error) => crate::error::ApiError::internal(error.to_string()).into_response(),
    }
}

/// Delete thread.
pub async fn delete_thread(
    Extension(_workspace): Extension<WorkspaceContext>,
    AxumPath(_thread_id): AxumPath<String>,
) -> impl IntoResponse {
    // Note: This logic for deletion should be implemented in ConversationsDb if needed.
    // For now, we only have placeholders.
    crate::error::ApiError::new(
        StatusCode::NOT_IMPLEMENTED,
        "NOT_IMPLEMENTED",
        "thread deletion not implemented",
    )
    .into_response()
}
