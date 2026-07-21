//! HTTP and CLI handlers for Xavier task management.

use anyhow::Result;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Response,
};
use serde::Deserialize;
use serde_json::Value;
use tracing::info;

use crate::cli::commands::enums::TaskCommand;
use crate::cli::config::{require_xavier_token, resolve_base_url};
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

/// Tasks list handler.
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

/// Tasks run handler.
pub async fn tasks_run_handler(
    State(state): State<CliState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    let task_result = state.tasks.store.get_task(&id).await;
    let task = match task_result {
        Ok(Some(t)) => t,
        Ok(None) => {
            return json_response(
                StatusCode::NOT_FOUND,
                serde_json::json!({"status": "error", "message": "Task not found"}),
            )
        }
        Err(e) => {
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"status": "error", "message": e.to_string()}),
            )
        }
    };

    let agent_id = match &task.assignee {
        Some(a) => a.clone(),
        None => {
            return json_response(
                StatusCode::BAD_REQUEST,
                serde_json::json!({"status": "error", "message": "Task has no assignee"}),
            )
        }
    };

    // Transition task to InProgress
    let _ = state.tasks.move_task(&id, TaskStatus::InProgress).await;

    let mut agent_config =
        xavier::agents::AgentConfig::new(agent_id.clone()).with_task(task.title.clone());

    // Try to get provider/model from task metadata or defaults
    // For now, we use defaults or simple mapping
    let agent_entry = state.agent_registry.get(&agent_id).await;
    if let Some(entry) = agent_entry {
        if let Some(name) = entry.metadata.name {
            agent_config.name = name;
        }
        if let Some(role) = entry.metadata.role {
            agent_config =
                agent_config.with_context(vec![("role".to_string(), role)].into_iter().collect());
        }
    }

    let mut agent = xavier::agents::Agent::new(agent_config);
    let memory = state.qmd_memory.clone();
    let registry = state.agent_registry.clone();
    let task_service = state.tasks.clone();

    let task_id_for_async = id.clone();
    tokio::spawn(async move {
        match agent.run(memory, Some(registry)).await {
            Ok(_) => {
                info!("Task {} finished successfully", task_id_for_async);
                let _ = task_service
                    .move_task(&task_id_for_async, TaskStatus::Done)
                    .await;
            }
            Err(e) => {
                info!("Task {} failed: {}", task_id_for_async, e);
                let _ = task_service
                    .move_task(&task_id_for_async, TaskStatus::Failed)
                    .await;
            }
        }
    });

    json_response(
        StatusCode::OK,
        serde_json::json!({
            "status": "ok",
            "message": "Task execution started",
            "task_id": id,
            "agent_id": agent_id,
        }),
    )
}

/// Tasks sync handler.
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

/// Handle task command.
pub async fn handle_task_command(cmd: TaskCommand) -> Result<()> {
    match cmd {
        TaskCommand::List {
            project,
            status,
            search,
            format,
        } => task_list(project, status, search, format).await,
        TaskCommand::Create {
            title,
            project,
            description,
        } => task_create(title, project, description).await,
        TaskCommand::Run { id } => task_run(id).await,
        TaskCommand::Move { id, status } => task_move(id, status).await,
    }
}

async fn task_list(
    project: Option<String>,
    status: Option<String>,
    search: Option<String>,
    format: String,
) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = crate::cli::commands::enums::CLI_HTTP_CLIENT.clone();

    let mut url = format!("{}/v1/tasks", base_url);
    let mut params: Vec<(String, String)> = Vec::new();
    if let Some(ref p) = project {
        params.push(("project".to_string(), p.clone()));
    }
    if let Some(ref s) = status {
        params.push(("status".to_string(), s.clone()));
    }
    if let Some(ref q) = search {
        params.push(("search".to_string(), q.clone()));
    }
    if !params.is_empty() {
        let qs = params
            .iter()
            .map(|(key, value)| format!("{}={}", key, urlencoding(value)))
            .collect::<Vec<_>>()
            .join("&");
        url = format!("{}?{}", url, qs);
    }

    let resp = client
        .get(&url)
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    let body: Value = if resp.status().is_success() {
        resp.json().await.unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let tasks = extract_tasks(&body);

    match format.as_str() {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&tasks)?);
        }
        _ => {
            if tasks.is_empty() {
                println!("No tasks found.");
                return Ok(());
            }

            println!();
            println!(
                "{:<30} {:<12} {:<10} {:<20} {:<15}",
                "ID", "Status", "Priority", "Project", "Title"
            );
            println!("{}", "-".repeat(95));

            for task in &tasks {
                let id = task["id"].as_str().unwrap_or("?");
                let title = task["title"].as_str().unwrap_or("?");
                let project_name = task["project"].as_str().unwrap_or("?");
                let status = task["status"].as_str().unwrap_or("?");
                let priority = task["priority"].as_str().unwrap_or("medium");

                println!(
                    "{:<30} {:<12} {:<10} {:<20} {:<15}",
                    truncate(id, 29),
                    status,
                    priority,
                    project_name,
                    truncate(title, 14),
                );
            }
            println!("{}", "-".repeat(95));
            println!("Total: {} tasks", tasks.len());
        }
    }

    Ok(())
}

async fn task_create(title: String, project: String, description: Option<String>) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = crate::cli::commands::enums::CLI_HTTP_CLIENT.clone();

    let payload = serde_json::json!({
        "title": title,
        "project": project,
        "description": description.unwrap_or_default(),
    });

    let resp = client
        .post(format!("{}/v1/tasks", base_url))
        .header("X-Xavier-Token", &token)
        .json(&payload)
        .send()
        .await?;

    if resp.status().is_success() {
        let task: Value = resp.json().await?;
        let id = task["id"].as_str().unwrap_or("?");
        println!("Task created: {} - {}", id, title);
    } else {
        let text = resp.text().await?;
        println!("Failed to create task: {}", text);
    }

    Ok(())
}

async fn task_run(id: String) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = crate::cli::commands::enums::CLI_HTTP_CLIENT.clone();

    let resp = client
        .post(format!("{}/v1/tasks/{}/run", base_url, id))
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if resp.status().is_success() {
        println!("Task {} is now in progress.", id);
    } else {
        let text = resp.text().await?;
        println!("Failed to run task: {}", text);
    }

    Ok(())
}

async fn task_move(id: String, status: String) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = crate::cli::commands::enums::CLI_HTTP_CLIENT.clone();

    let payload = serde_json::json!({ "status": status });
    let resp = client
        .post(format!("{}/v1/tasks/{}/move", base_url, id))
        .header("X-Xavier-Token", &token)
        .json(&payload)
        .send()
        .await?;

    if resp.status().is_success() {
        println!("Task {} moved to {}.", id, status);
    } else {
        let text = resp.text().await?;
        println!("Failed to move task: {}", text);
    }

    Ok(())
}

fn extract_tasks(body: &Value) -> Vec<Value> {
    if let Some(tasks) = body.as_array() {
        return tasks.clone();
    }

    body.get("tasks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn urlencoding(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            b' ' => result.push_str("%20"),
            _ => result.push_str(&format!("%{:02X}", byte)),
        }
    }
    result
}
