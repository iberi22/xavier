//! Workspace DB handlers for Multi-DB Hub operations.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::cli::state::CliState;
use crate::workspace::{WorkspaceDb, WorkspaceDbKind};

#[derive(Debug, Deserialize)]
pub struct CreateDbRequest {
    pub db_id: String,
    pub display_name: String,
    pub kind: WorkspaceDbKind,
}

#[derive(Debug, Serialize)]
pub struct CreateDbResponse {
    pub success: bool,
    pub message: String,
    pub db: Option<WorkspaceDb>,
}

#[derive(Debug, Serialize)]
pub struct ListDbsResponse {
    pub databases: Vec<WorkspaceDb>,
}

#[derive(Debug, Serialize)]
pub struct DeleteDbResponse {
    pub success: bool,
    pub message: String,
}

/// POST /v1/workspaces/db
/// Creates a new independent SQLite DB for a workspace
pub async fn create_workspace_db_handler(
    State(state): State<CliState>,
    Json(payload): Json<CreateDbRequest>,
) -> Result<Json<CreateDbResponse>, (StatusCode, Json<CreateDbResponse>)> {
    match state
        .multi_db
        .create_database(payload.db_id, payload.display_name, payload.kind)
        .await
    {
        Ok(db) => Ok(Json(CreateDbResponse {
            success: true,
            message: "Database created and initialized successfully".to_string(),
            db: Some(db),
        })),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(CreateDbResponse {
                success: false,
                message: format!("Failed to create database: {}", e),
                db: None,
            }),
        )),
    }
}

/// GET /v1/workspaces/db
/// Lists all registered independent SQLite DBs
pub async fn list_workspace_dbs_handler(State(state): State<CliState>) -> Json<ListDbsResponse> {
    let databases = state.multi_db.list_databases().await;
    Json(ListDbsResponse { databases })
}

/// DELETE /v1/workspaces/db/:id
/// Deletes a specific database file and removes it from registry
pub async fn delete_workspace_db_handler(
    State(state): State<CliState>,
    Path(db_id): Path<String>,
) -> Result<Json<DeleteDbResponse>, (StatusCode, Json<DeleteDbResponse>)> {
    match state.multi_db.delete_database(&db_id).await {
        Ok(true) => Ok(Json(DeleteDbResponse {
            success: true,
            message: format!(
                "Database '{}' deleted successfully from registry and disk",
                db_id
            ),
        })),
        Ok(false) => Err((
            StatusCode::NOT_FOUND,
            Json(DeleteDbResponse {
                success: false,
                message: format!("Database '{}' not found in registry", db_id),
            }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(DeleteDbResponse {
                success: false,
                message: format!("Failed to delete database: {}", e),
            }),
        )),
    }
}
