use super::types::{Bookmark, GraphData, Widget};
use crate::{
    codebase::connection_manager::ConnectionManager,
    memory::sqlite_store::{TABLE_PANEL_BOOKMARKS, TABLE_PANEL_GRAPHS, TABLE_PANEL_WIDGETS},
    memory::sqlite_vec_store::{project_id_for_path, VecSqliteMemoryStore, VecSqliteStoreConfig},
    workspace::WorkspaceContext,
};
use axum::{http::StatusCode, response::IntoResponse, Extension, Json};
use chrono::Utc;
use rusqlite::params;
use serde_json::json;

/// Resolve the ConnectionManager project_id used for panel bookmarks/widgets/graphs.
///
/// For the vec backend this MUST match [`VecSqliteMemoryStore`]'s hashed pool id
/// (not the literal `"vec_store"` alias that only points at `./vec-store.sqlite3`).
pub fn resolve_panel_project_id(workspace: &WorkspaceContext) -> String {
    let backend = workspace.workspace.durable_store_backend();

    if cfg!(test) {
        let base_id = if backend == "vec" { "vec_store" } else { "memory" };
        return format!("{}_test_{}", base_id, workspace.workspace_id);
    }

    if backend == "vec" {
        if let Some(id) = workspace
            .workspace
            .durable_store()
            .as_any()
            .downcast_ref::<VecSqliteMemoryStore>()
            .map(|s| s.connection_project_id().to_string())
        {
            return id;
        }
        // Fallback when the workspace store is not yet a live VecSqliteMemoryStore
        // (e.g. early boot paths): derive the same id as VecSqliteMemoryStore::new.
        return project_id_for_path(&VecSqliteStoreConfig::from_env().path);
    }

    "memory".to_string()
}

pub async fn list_bookmarks(
    Extension(workspace): Extension<WorkspaceContext>,
) -> impl IntoResponse {
    let workspace_id = workspace.workspace.config().id.clone();
    let project_id = resolve_panel_project_id(&workspace);

    match ConnectionManager::global()
        .with_conn(&project_id, move |conn| {
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
    let project_id = resolve_panel_project_id(&workspace);

    match ConnectionManager::global()
        .with_conn(&project_id, move |conn| {
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

pub async fn list_widgets(Extension(workspace): Extension<WorkspaceContext>) -> impl IntoResponse {
    let workspace_id = workspace.workspace.config().id.clone();
    let project_id = resolve_panel_project_id(&workspace);

    match ConnectionManager::global()
        .with_conn(&project_id, move |conn| {
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
    let project_id = resolve_panel_project_id(&workspace);

    match ConnectionManager::global()
        .with_conn(&project_id, move |conn| {
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

/// Default empty roadmap graph when the workspace has never saved one.
fn default_empty_panel_graph() -> GraphData {
    GraphData {
        id: "default".to_string(),
        name: "Workspace roadmap".to_string(),
        data: json!({ "nodes": [], "links": [] }),
        created_at: Utc::now(),
    }
}

/// Panel roadmap payload must be `{ "nodes": [], "links": [] }` (not freeform / edges).
fn validate_panel_graph_payload(data: &serde_json::Value) -> Result<(), String> {
    let obj = data
        .as_object()
        .ok_or_else(|| "data must be a JSON object".to_string())?;
    let nodes = obj
        .get("nodes")
        .ok_or_else(|| "data.nodes is required".to_string())?;
    let links = obj
        .get("links")
        .ok_or_else(|| "data.links is required".to_string())?;
    if !nodes.is_array() {
        return Err("data.nodes must be an array".to_string());
    }
    if !links.is_array() {
        return Err("data.links must be an array".to_string());
    }
    Ok(())
}

pub async fn get_graph(Extension(workspace): Extension<WorkspaceContext>) -> impl IntoResponse {
    let workspace_id = workspace.workspace.config().id.clone();
    let project_id = resolve_panel_project_id(&workspace);

    match ConnectionManager::global()
        .with_conn(&project_id, move |conn| {
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
                    data: serde_json::from_str(&data_str)
                        .unwrap_or_else(|_| json!({ "nodes": [], "links": [] })),
                    created_at: created_at_str.parse().unwrap_or_else(|_| Utc::now()),
                }))
            } else {
                Ok(None)
            }
        })
        .await
    {
        // Empty workspace: 200 + empty nodes/links so the UI never falls back to demo data.
        Ok(Some(graph)) => Json(graph).into_response(),
        Ok(None) => Json(default_empty_panel_graph()).into_response(),
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
    if let Err(message) = validate_panel_graph_payload(&payload.data) {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": message }))).into_response();
    }

    let workspace_id = workspace.workspace.config().id.clone();
    let project_id = resolve_panel_project_id(&workspace);

    match ConnectionManager::global()
        .with_conn(&project_id, move |conn| {
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
