//! CLI task management commands.

use crate::cli::commands::enums::{TasksCommand, CLI_HTTP_CLIENT};
use crate::cli::config::{require_xavier_token, resolve_base_url};
use anyhow::Result;

/// Handle tasks command.
pub async fn handle_tasks_command(cmd: TasksCommand) -> Result<()> {
    let token = require_xavier_token()?;
    let base_url = resolve_base_url();
    let client = CLI_HTTP_CLIENT.clone();

    let response = match &cmd {
        TasksCommand::List {
            project,
            status,
            search,
            json: _,
        } => {
            let mut url = reqwest::Url::parse(&format!("{}/v1/tasks", base_url))?;
            {
                let mut pairs = url.query_pairs_mut();
                if let Some(project) = project {
                    pairs.append_pair("project", project);
                }
                if let Some(status) = status {
                    pairs.append_pair("status", status);
                }
                if let Some(search) = search {
                    pairs.append_pair("search", search);
                }
            }
            client
                .get(url)
                .header("X-Xavier-Token", &token)
                .send()
                .await?
        }
        TasksCommand::Sync => {
            client
                .post(format!("{}/v1/tasks/sync", base_url))
                .header("X-Xavier-Token", &token)
                .send()
                .await?
        }
    };

    let is_json_requested = match cmd {
        TasksCommand::List { json, .. } => json,
        _ => false,
    };

    let status = response.status();
    let body: serde_json::Value = response.json().await.unwrap_or_default();

    if is_json_requested {
        if status.is_success() {
            println!("{}", serde_json::to_string(&body)?);
        } else {
            let error_msg = body
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({ "error": error_msg })).unwrap()
            );
            std::process::exit(3); // Error exit code.
        }
        return Ok(());
    }

    if status.is_success() {
        println!("{}", serde_json::to_string_pretty(&body)?);
    } else {
        println!("Tasks request failed ({}):", status);
        println!("{}", serde_json::to_string_pretty(&body)?);
    }

    Ok(())
}
