use std::path::{Path, PathBuf};

use axum::{
    extract::Path as AxumPath,
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    agents::ui_render::UiRenderAgent,
    codebase::conversations_db::{ConversationsDb, Message, ThreadDetail, ThreadSummary},
    workspace::WorkspaceContext,
};

const PANEL_BUILD_DIR: &str = "panel-ui/build";

#[derive(Debug, Deserialize)]
pub struct CreateThreadRequest {
    pub title: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PanelChatRequest {
    pub thread_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PanelChatResponse {
    pub thread: ThreadSummary,
    pub messages: Vec<Message>,
}

pub async fn panel_index() -> impl IntoResponse {
    match tokio::fs::read_to_string(panel_build_path("index.html")).await {
        Ok(contents) => Html(contents).into_response(),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Panel frontend assets are missing. Build them first: cd panel-ui && npm install && npm run build",
        )
            .into_response(),
    }
}

pub async fn panel_asset(AxumPath(path): AxumPath<String>) -> impl IntoResponse {
    let asset_path = panel_build_path(&format!("assets/{path}"));
    match tokio::fs::read(&asset_path).await {
        Ok(bytes) => asset_response(bytes, asset_content_type(&asset_path)),
        Err(_) => (StatusCode::NOT_FOUND, "Asset not found").into_response(),
    }
}

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
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

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
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

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
                Err(error) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": error.to_string() })),
                )
                    .into_response(),
            }
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "thread not found" })),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

pub async fn delete_thread(
    Extension(_workspace): Extension<WorkspaceContext>,
    AxumPath(_thread_id): AxumPath<String>,
) -> impl IntoResponse {
    // Note: This logic for deletion should be implemented in ConversationsDb if needed.
    // For now, we only have placeholders.
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({ "error": "thread deletion not implemented" })),
    )
        .into_response()
}

pub async fn process_chat(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(payload): Json<PanelChatRequest>,
) -> impl IntoResponse {
    match process_chat_inner(
        &workspace.workspace.conversations_db,
        &workspace.workspace.runtime,
        &workspace,
        payload,
    )
    .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn process_chat_inner(
    db: &ConversationsDb,
    runtime: &std::sync::Arc<crate::agents::AgentRuntime>,
    workspace: &WorkspaceContext,
    payload: PanelChatRequest,
) -> anyhow::Result<PanelChatResponse> {
    let thread = match payload.thread_id {
        Some(thread_id) => match db.get_thread(&thread_id).await? {
            Some(thread) => thread,
            None => db.create_thread(Some(&payload.message), None, Some("panel")).await?,
        },
        None => db.create_thread(Some(&payload.message), None, Some("panel")).await?,
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

fn panel_build_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(PANEL_BUILD_DIR)
        .join(relative)
}

fn asset_content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|value| value.to_str()) {
        Some("js") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("json") => "application/json; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn asset_response(bytes: Vec<u8>, content_type: &'static str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .body(axum::body::Body::from(bytes))
        .expect("test assertion")
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agents::RuntimeConfig,
        memory::file_indexer::{FileIndexer, FileIndexerConfig},
        workspace::{
            EmbeddingProviderMode, PlanTier, SyncPolicy, WorkspaceConfig, WorkspaceContext,
            WorkspaceRegistry, WorkspaceState,
        },
        AppState,
    };
    use axum::{
        body::{to_bytes, Body},
        http::Request,
        routing::{get, post},
        Router,
    };
    use std::sync::Arc;
    use tower::util::ServiceExt;

    async fn test_state() -> (AppState, WorkspaceContext) {
        let db_path = std::env::temp_dir().join(format!("xavier-panel-{}.db", Ulid::new()));
        let code_db = Arc::new(code_graph::db::CodeGraphDB::new(&db_path).expect("test assertion"));
        let code_indexer = Arc::new(code_graph::indexer::Indexer::new(Arc::clone(&code_db)));
        let code_query = Arc::new(code_graph::query::QueryEngine::new(Arc::clone(&code_db)));
        let workspace_registry = Arc::new(WorkspaceRegistry::new());
        let workspace = WorkspaceState::new(
            WorkspaceConfig {
                id: "panel-test".to_string(),
                token: "panel-token".to_string(),
                plan: PlanTier::Personal,
                memory_backend: crate::memory::store::MemoryBackend::File,
                storage_limit_bytes: Some(10 * 1024 * 1024),
                request_limit: Some(10_000),
                request_unit_limit: Some(20_000),
                embedding_provider_mode: EmbeddingProviderMode::BringYourOwn,
                managed_google_embeddings: false,
                sync_policy: SyncPolicy::CloudMirror,
            },
            RuntimeConfig::default(),
            std::env::temp_dir().join(format!("xavier-panel-store-{}", Ulid::new())),
        )
        .await
        .expect("test assertion");
        workspace_registry
            .insert(workspace)
            .await
            .expect("test assertion");
        let workspace = workspace_registry
            .authenticate("panel-token")
            .await
            .expect("test assertion");

        (
            AppState {
                workspace_registry,
                indexer: FileIndexer::new(FileIndexerConfig::default(), Some(code_indexer.clone())),
                agent_indexer: crate::memory::agent_indexer::AgentIndexer::new(
                    FileIndexer::new(FileIndexerConfig::default(), Some(code_indexer.clone()))
                ),
                code_indexer,
                code_query,
                code_db,
                security_service: Arc::new(crate::app::security_service::SecurityService::new()),
            },
            workspace,
        )
    }

    fn test_router(state: AppState, workspace: WorkspaceContext) -> Router {
        Router::new()
            .route("/panel/api/threads", get(list_threads).post(create_thread))
            .route(
                "/panel/api/threads/{thread_id}",
                get(get_thread).delete(delete_thread),
            )
            .route("/panel/api/chat", post(process_chat))
            .layer(Extension(workspace))
            .with_state(state)
    }

    #[tokio::test]
    async fn creates_and_fetches_threads_via_http() {
        let (state, workspace) = test_state().await;
        let app = test_router(state, workspace);
        let create_request = Request::builder()
            .method("POST")
            .uri("/panel/api/threads")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"title":"Panel Thread"}"#))
            .expect("test assertion");

        let create_response = app
            .clone()
            .oneshot(create_request)
            .await
            .expect("test assertion");
        let create_body = to_bytes(create_response.into_body(), usize::MAX)
            .await
            .expect("test assertion");
        let summary: ThreadSummary = serde_json::from_slice(&create_body).expect("test assertion");

        let get_request = Request::builder()
            .method("GET")
            .uri(format!("/panel/api/threads/{}", summary.id))
            .body(Body::empty())
            .expect("test assertion");
        let get_response = app.oneshot(get_request).await.expect("test assertion");
        let get_body = to_bytes(get_response.into_body(), usize::MAX)
            .await
            .expect("test assertion");
        let detail: ThreadDetail = serde_json::from_slice(&get_body).expect("test assertion");
        assert_eq!(detail.thread.id, summary.id);
    }

    #[tokio::test]
    async fn chat_persists_assistant_ui_message() {
        let (state, workspace) = test_state().await;
        let app = test_router(state, workspace);
        let request = Request::builder()
            .method("POST")
            .uri("/panel/api/chat")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"message":"Explain xavier memory"}"#))
            .expect("test assertion");

        let response = app.oneshot(request).await.expect("test assertion");
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("test assertion");
        let payload: PanelChatResponse = serde_json::from_slice(&body).expect("test assertion");

        assert_eq!(payload.messages.len(), 2);
        assert_eq!(payload.messages[1].role, "assistant");
        assert!(payload.messages[1].openui_lang.is_some());
    }
}
