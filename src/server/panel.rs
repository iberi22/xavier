//! Xavier administration web panel
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
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
    codebase::connection_manager::ConnectionManager,
    codebase::conversations_db::{ConversationsDb, Message, ThreadDetail, ThreadSummary},
    memory::sqlite_store::{TABLE_PANEL_BOOKMARKS, TABLE_PANEL_GRAPHS, TABLE_PANEL_WIDGETS},
    workspace::WorkspaceContext,
};
use chrono::{DateTime, Utc};
use rusqlite::params;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub title: String,
    pub url: String,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Widget {
    pub id: String,
    #[serde(rename = "type")]
    pub widget_type: String,
    pub config: serde_json::Value,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphData {
    pub id: String,
    pub name: String,
    pub data: serde_json::Value,
    pub created_at: DateTime<Utc>,
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

pub async fn list_bookmarks(
    Extension(workspace): Extension<WorkspaceContext>,
) -> impl IntoResponse {
    let workspace_id = workspace.workspace.config().id.clone();
    let backend = workspace.workspace.durable_store_backend();
    let project_id = if backend == "vec" { "vec_store" } else { "memory" };

    match ConnectionManager::global()
        .with_conn(project_id, move |conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT id, title, url, metadata, created_at FROM {} WHERE workspace_id = ? ORDER BY created_at DESC",
                TABLE_PANEL_BOOKMARKS
            ))?;
            let mut rows = stmt.query(params![workspace_id])?;
            let mut bookmarks = Vec::new();
            while let Some(row) = rows.next()? {
                let metadata_str: String = row.get(3)?;
                let created_at_str: String = row.get(4)?;
                bookmarks.push(Bookmark {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    url: row.get(2)?,
                    metadata: serde_json::from_str(&metadata_str).unwrap_or_default(),
                    created_at: created_at_str.parse().unwrap_or_else(|_| Utc::now()),
                });
            }
            Ok(bookmarks)
        })
        .await
    {
        Ok(bookmarks) => Json(bookmarks).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

pub async fn save_bookmark(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(payload): Json<Bookmark>,
) -> impl IntoResponse {
    let workspace_id = workspace.workspace.config().id.clone();
    let backend = workspace.workspace.durable_store_backend();
    let project_id = if backend == "vec" { "vec_store" } else { "memory" };

    match ConnectionManager::global()
        .with_conn(project_id, move |conn| {
            conn.execute(
                &format!(
                    "INSERT OR REPLACE INTO {} (id, workspace_id, title, url, metadata, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    TABLE_PANEL_BOOKMARKS
                ),
                params![
                    payload.id,
                    workspace_id,
                    payload.title,
                    payload.url,
                    serde_json::to_string(&payload.metadata).unwrap_or_default(),
                    payload.created_at.to_rfc3339(),
                ],
            )?;
            Ok(())
        })
        .await
    {
        Ok(_) => StatusCode::OK.into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

pub async fn list_widgets(
    Extension(workspace): Extension<WorkspaceContext>,
) -> impl IntoResponse {
    let workspace_id = workspace.workspace.config().id.clone();
    let backend = workspace.workspace.durable_store_backend();
    let project_id = if backend == "vec" { "vec_store" } else { "memory" };

    match ConnectionManager::global()
        .with_conn(project_id, move |conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT id, type, config, x, y, w, h, created_at FROM {} WHERE workspace_id = ? ORDER BY created_at ASC",
                TABLE_PANEL_WIDGETS
            ))?;
            let mut rows = stmt.query(params![workspace_id])?;
            let mut widgets = Vec::new();
            while let Some(row) = rows.next()? {
                let config_str: String = row.get(2)?;
                let created_at_str: String = row.get(7)?;
                widgets.push(Widget {
                    id: row.get(0)?,
                    widget_type: row.get(1)?,
                    config: serde_json::from_str(&config_str).unwrap_or_default(),
                    x: row.get(3)?,
                    y: row.get(4)?,
                    w: row.get(5)?,
                    h: row.get(6)?,
                    created_at: created_at_str.parse().unwrap_or_else(|_| Utc::now()),
                });
            }
            Ok(widgets)
        })
        .await
    {
        Ok(widgets) => Json(widgets).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

pub async fn save_widget(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(payload): Json<Widget>,
) -> impl IntoResponse {
    let workspace_id = workspace.workspace.config().id.clone();
    let backend = workspace.workspace.durable_store_backend();
    let project_id = if backend == "vec" { "vec_store" } else { "memory" };

    match ConnectionManager::global()
        .with_conn(project_id, move |conn| {
            conn.execute(
                &format!(
                    "INSERT OR REPLACE INTO {} (id, workspace_id, type, config, x, y, w, h, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    TABLE_PANEL_WIDGETS
                ),
                params![
                    payload.id,
                    workspace_id,
                    payload.widget_type,
                    serde_json::to_string(&payload.config).unwrap_or_default(),
                    payload.x,
                    payload.y,
                    payload.w,
                    payload.h,
                    payload.created_at.to_rfc3339(),
                ],
            )?;
            Ok(())
        })
        .await
    {
        Ok(_) => StatusCode::OK.into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

pub async fn get_graph(Extension(workspace): Extension<WorkspaceContext>) -> impl IntoResponse {
    let workspace_id = workspace.workspace.config().id.clone();
    let backend = workspace.workspace.durable_store_backend();
    let project_id = if backend == "vec" { "vec_store" } else { "memory" };

    match ConnectionManager::global()
        .with_conn(project_id, move |conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT id, name, data, created_at FROM {} WHERE workspace_id = ? LIMIT 1",
                TABLE_PANEL_GRAPHS
            ))?;
            let mut rows = stmt.query(params![workspace_id])?;
            if let Some(row) = rows.next()? {
                let data_str: String = row.get(2)?;
                let created_at_str: String = row.get(3)?;
                Ok(Some(GraphData {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    data: serde_json::from_str(&data_str).unwrap_or_default(),
                    created_at: created_at_str.parse().unwrap_or_else(|_| Utc::now()),
                }))
            } else {
                Ok(None)
            }
        })
        .await
    {
        Ok(Some(graph)) => Json(graph).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "graph data not found" })),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

pub async fn save_graph(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(payload): Json<GraphData>,
) -> impl IntoResponse {
    let workspace_id = workspace.workspace.config().id.clone();
    let backend = workspace.workspace.durable_store_backend();
    let project_id = if backend == "vec" { "vec_store" } else { "memory" };

    match ConnectionManager::global()
        .with_conn(project_id, move |conn| {
            conn.execute(
                &format!(
                    "INSERT OR REPLACE INTO {} (id, workspace_id, name, data, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    TABLE_PANEL_GRAPHS
                ),
                params![
                    payload.id,
                    workspace_id,
                    payload.name,
                    serde_json::to_string(&payload.data).unwrap_or_default(),
                    payload.created_at.to_rfc3339(),
                ],
            )?;
            Ok(())
        })
        .await
    {
        Ok(_) => StatusCode::OK.into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
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
    use ulid::Ulid;

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
                agent_indexer: crate::memory::agent_indexer::AgentIndexer::new(FileIndexer::new(
                    FileIndexerConfig::default(),
                    Some(code_indexer.clone()),
                )),
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

    #[tokio::test]
    async fn bookmarks_crud() {
        let (state, workspace) = test_state().await;
        let app = test_router(state, workspace)
            .route("/panel/api/bookmarks", get(list_bookmarks).post(save_bookmark));

        let bookmark = Bookmark {
            id: "test-id".to_string(),
            title: "Test Bookmark".to_string(),
            url: "https://example.com".to_string(),
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
        };

        let create_request = Request::builder()
            .method("POST")
            .uri("/panel/api/bookmarks")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&bookmark).unwrap()))
            .expect("test assertion");

        let response = app.clone().oneshot(create_request).await.expect("test assertion");
        assert_eq!(response.status(), StatusCode::OK);

        let get_request = Request::builder()
            .method("GET")
            .uri("/panel/api/bookmarks")
            .body(Body::empty())
            .expect("test assertion");

        let response = app.oneshot(get_request).await.expect("test assertion");
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("test assertion");
        let bookmarks: Vec<Bookmark> = serde_json::from_slice(&body).expect("test assertion");
        assert_eq!(bookmarks.len(), 1);
        assert_eq!(bookmarks[0].id, "test-id");
    }
}
