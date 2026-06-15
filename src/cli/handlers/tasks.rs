//! HTTP handlers for Xavier task management.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Response,
};
use serde::Deserialize;

use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use crate::cli::commands::enums::TaskCommand;
use crate::cli::config::{require_xavier_token, resolve_base_url};
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

/// CLI task command dispatcher (calls HTTP API)
pub async fn handle_task_command(cmd: TaskCommand) -> anyhow::Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = crate::cli::commands::enums::CLI_HTTP_CLIENT.clone();

    match cmd {
        TaskCommand::List {
            project,
            status,
            search,
            format: _,
        } => {
            let mut url = format!("{}/tasks/list", base_url);
            if let Some(p) = &project {
                url.push_str(&format!("?project={}", p));
            }
            if let Some(s) = &status {
                let sep = if url.contains('?') { "&" } else { "?" };
                url.push_str(&format!("{}status={}", sep, s));
            }
            if let Some(q) = &search {
                let sep = if url.contains('?') { "&" } else { "?" };
                url.push_str(&format!("{}search={}", sep, q));
            }

            let resp = client.get(&url).header("X-Xavier-Token", &token).send().await?;

            if resp.status().is_success() {
                let data: serde_json::Value = resp.json().await.unwrap_or_default();
                println!("{}", serde_json::to_string_pretty(&data)?);
            } else {
                println!("Task list failed: {}", resp.status());
            }
        }
        TaskCommand::Create {
            title,
            project,
            description,
        } => {
            let payload = serde_json::json!({
                "title": title,
                "project": project,
                "description": description,
            });

            let resp = client
                .post(format!("{}/tasks/create", base_url))
                .header("X-Xavier-Token", &token)
                .json(&payload)
                .send()
                .await?;

            if resp.status().is_success() {
                let data: serde_json::Value = resp.json().await.unwrap_or_default();
                println!("{}", serde_json::to_string_pretty(&data)?);
            } else {
                println!("Task creation failed: {}", resp.status());
            }
        }
        TaskCommand::Run { id } => {
            let resp = client
                .post(format!("{}/tasks/{}/run", base_url, id))
                .header("X-Xavier-Token", &token)
                .send()
                .await?;

            if resp.status().is_success() {
                let data: serde_json::Value = resp.json().await.unwrap_or_default();
                println!("{}", serde_json::to_string_pretty(&data)?);
            } else {
                println!("Task run failed: {}", resp.status());
            }
        }
        TaskCommand::Move { id, status } => {
            let payload = serde_json::json!({"status": status});

            let resp = client
                .patch(format!("{}/tasks/{}/move", base_url, id))
                .header("X-Xavier-Token", &token)
                .json(&payload)
                .send()
                .await?;

            if resp.status().is_success() {
                let data: serde_json::Value = resp.json().await.unwrap_or_default();
                println!("{}", serde_json::to_string_pretty(&data)?);
            } else {
                println!("Task move failed: {}", resp.status());
            }
        }
    }

    Ok(())
}
