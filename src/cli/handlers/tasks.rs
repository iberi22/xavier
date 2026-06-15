//! HTTP handlers for Xavier task management.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Response,
};
use serde::Deserialize;

use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use xavier::tasks::models::{TaskFilter, TaskStatus};
use xavier::tasks::TaskStore;

#[derive(Debug, Deserialize)]
pub struct TaskListQuery {
    pub project: Option<String>,
    pub status: Option<String>,
    pub search: Option<String>,
}

pub async fn tasks_list_handler(
    State(state): State<CliState>,
    Query(query): Query<TaskListQuery>,
) -> Response {
    let status = query
        .status
        .as_deref()
        .and_then(|value| value.parse::<TaskStatus>().ok());
    let filter = TaskFilter {
        project: query.project.clone(),
        status,
        search: query.search.clone(),
        ..Default::default()
    };

    match state.tasks.store.list_tasks(&filter).await {
        Ok(tasks) => json_response(
            StatusCode::OK,
            serde_json::json!({
                "status": "ok",
                "count": tasks.len(),
                "tasks": tasks,
            }),
        ),
        Err(error) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({
                "status": "error",
                "message": error.to_string(),
            }),
        ),
    }
}

pub async fn tasks_sync_handler(State(state): State<CliState>) -> Response {
    match state.tasks.store.list_projects().await {
        Ok(projects) => {
            let mut total_tasks = 0usize;
            for project in &projects {
                if let Ok(tasks) = state.tasks.get_project_tasks(&project.name).await {
                    total_tasks += tasks.len();
                }
            }

            json_response(
                StatusCode::OK,
                serde_json::json!({
                    "status": "ok",
                    "sync": {
                        "mode": "local",
                        "projects": projects.len(),
                        "tasks": total_tasks,
                        "synced": total_tasks,
                        "failed": 0,
                        "message": "Local task store checked; external Planka sync is not configured on this server route."
                    }
                }),
            )
        }
        Err(error) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({
                "status": "error",
                "message": error.to_string(),
            }),
        ),
    }
}
