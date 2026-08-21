//! Agent memory management CLI handlers (client-side)

use anyhow::Result;
use colored::*;
use serde_json::json;

use crate::cli::commands::enums::{AgentCommand, CLI_HTTP_CLIENT};
use crate::cli::config::{require_xavier_token, resolve_base_url};

/// Handle agent commands.
pub async fn handle_agent_command(cmd: AgentCommand) -> Result<()> {
    match cmd {
        AgentCommand::Scan { agent, json } => handle_agent_scan(agent, json).await,
        AgentCommand::Index {
            agent,
            codex,
            jules,
            json,
        } => handle_agent_index(agent, codex, jules, json).await,
        AgentCommand::Push { agent, json } => handle_agent_sync(agent, false, json).await,
        AgentCommand::Pull { agent, json } => handle_agent_sync(agent, true, json).await,
        AgentCommand::Chat {
            prompt,
            agent,
            interactive,
            json,
            limit,
            model,
        } => {
            crate::cli::handlers::chat::handle_chat_command(
                prompt,
                agent,
                interactive,
                json,
                limit,
                model,
            )
            .await
        }
        AgentCommand::Converse {
            prompt,
            agent,
            interactive,
            json,
            limit,
            model,
        } => {
            crate::cli::handlers::chat::handle_chat_command(
                prompt,
                agent,
                interactive,
                json,
                limit,
                model,
            )
            .await
        }
    }
}

async fn handle_agent_scan(agent_filter: Option<String>, as_json: bool) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = CLI_HTTP_CLIENT.clone();

    if !as_json {
        println!("{} Scanning for agent sessions...", "🔍".cyan());
    }

    let resp = client
        .get(format!("{}/xavier/openclaw/scan", base_url))
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if resp.status().is_success() {
        let mut data: serde_json::Value = resp.json().await?;

        // Filter by agent name if provided
        if let Some(filter) = agent_filter {
            if let Some(agents) = data["agents"].as_array_mut() {
                agents.retain(|s| {
                    s["agent_id"]
                        .as_str()
                        .map(|name| name.to_lowercase().contains(&filter.to_lowercase()))
                        .unwrap_or(false)
                });
                data["count"] = json!(agents.len());
            }
        }

        if as_json {
            println!("{}", serde_json::to_string_pretty(&data)?);
        } else {
            let count = data["count"].as_u64().unwrap_or(0);
            println!("{} Found {} OpenClaw agents", "✅".green(), count);
            if let Some(agents) = data["agents"].as_array() {
                for s in agents {
                    let memory_md = s["memory_md"].as_str().is_some();
                    println!(
                        "  • {} (MEMORY.md: {})",
                        s["agent_id"].as_str().unwrap_or("Unknown").bold(),
                        if memory_md { "YES".green() } else { "NO".red() }
                    );
                }
            }
        }
    } else {
        let err = resp.text().await?;
        if as_json {
            println!("{}", json!({"status": "error", "message": err}));
        } else {
            println!("{} Scan failed: {}", "❌".red(), err);
        }
    }

    Ok(())
}

async fn handle_agent_index(
    _agent_filter: Option<String>,
    codex: bool,
    jules: bool,
    as_json: bool,
) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = CLI_HTTP_CLIENT.clone();

    let mut targets = Vec::new();
    if codex {
        targets.push(("Codex", format!("{}/xavier/codex/index", base_url)));
    }
    if jules {
        targets.push(("Jules", format!("{}/xavier/jules/index", base_url)));
    }
    if targets.is_empty() {
        targets.push(("OpenClaw", format!("{}/xavier/openclaw/index", base_url)));
    }

    let mut total_indexed = 0;
    let mut results = Vec::new();

    for (target_name, endpoint) in targets {
        if !as_json {
            println!(
                "{} Indexing {} agent sessions into memory...",
                "🤖".cyan(),
                target_name
            );
        }

        let resp = client
            .post(&endpoint)
            .header("X-Xavier-Token", &token)
            .send()
            .await?;

        if resp.status().is_success() {
            let data: serde_json::Value = resp.json().await?;
            let count = data["indexed_count"].as_u64().unwrap_or(0);
            total_indexed += count;
            results.push(json!({ "target": target_name, "status": "ok", "indexed_count": count }));

            if !as_json {
                println!(
                    "{} Successfully indexed {} {} sessions",
                    "✅".green(),
                    count,
                    target_name
                );
            }
        } else {
            let err = resp.text().await?;
            results
                .push(json!({ "target": target_name, "status": "error", "message": err.clone() }));
            if !as_json {
                println!("{} Indexing {} failed: {}", "❌".red(), target_name, err);
            }
        }
    }

    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "status": "ok",
                "indexed_count": total_indexed,
                "details": results
            }))?
        );
    }

    Ok(())
}

async fn handle_agent_sync(agent_filter: Option<String>, pull: bool, as_json: bool) -> Result<()> {
    let base_url = resolve_base_url();
    let token = require_xavier_token()?;
    let client = CLI_HTTP_CLIENT.clone();

    let mode = if pull { "pull" } else { "push" };
    if !as_json {
        println!(
            "{} Syncing agent memory ({}) to cloud...",
            "🔄".cyan(),
            mode
        );
    }

    let resp = client
        .post(format!("{}/xavier/agents/sync", base_url))
        .header("X-Xavier-Token", &token)
        .json(&json!({ "mode": mode, "agent": agent_filter }))
        .send()
        .await?;

    if resp.status().is_success() {
        let data: serde_json::Value = resp.json().await?;
        if as_json {
            println!("{}", serde_json::to_string_pretty(&data)?);
        } else {
            println!("{} Agent memory synchronization completed", "✅".green());
            if let Some(stats) = data["stats"].as_object() {
                for (k, v) in stats {
                    println!("  • {}: {}", k, v);
                }
            }
        }
    } else {
        let err = resp.text().await?;
        if as_json {
            println!("{}", json!({"status": "error", "message": err}));
        } else {
            println!("{} Synchronization failed: {}", "❌".red(), err);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::commands::enums::Command;
    use clap::Parser;
    use mockito::Server;
    use serde_json::json;

    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[test]
    fn test_parse_agent_scan_cmd() {
        use crate::cli::state::Cli;
        let cli = Cli::try_parse_from(["xavier", "agent", "scan", "--agent", "cursor", "--json"])
            .unwrap();
        match cli.cmd {
            Some(Command::Agent {
                cmd: AgentCommand::Scan { agent, json },
            }) => {
                assert_eq!(agent, Some("cursor".to_string()));
                assert!(json);
            }
            _ => panic!("Expected AgentCommand::Scan"),
        }
    }

    #[test]
    fn test_parse_agent_index_cmd() {
        use crate::cli::state::Cli;
        let cli = Cli::try_parse_from(["xavier", "agent", "index", "--codex", "--jules", "--json"])
            .unwrap();
        match cli.cmd {
            Some(Command::Agent {
                cmd:
                    AgentCommand::Index {
                        agent,
                        codex,
                        jules,
                        json,
                    },
            }) => {
                assert_eq!(agent, None);
                assert!(codex);
                assert!(jules);
                assert!(json);
            }
            _ => panic!("Expected AgentCommand::Index"),
        }
    }

    #[test]
    fn test_parse_agent_push_cmd() {
        use crate::cli::state::Cli;
        let cli = Cli::try_parse_from(["xavier", "agent", "push", "--agent", "windsurf"]).unwrap();
        match cli.cmd {
            Some(Command::Agent {
                cmd: AgentCommand::Push { agent, json },
            }) => {
                assert_eq!(agent, Some("windsurf".to_string()));
                assert!(!json);
            }
            _ => panic!("Expected AgentCommand::Push"),
        }
    }

    #[test]
    fn test_parse_agent_pull_cmd() {
        use crate::cli::state::Cli;
        let cli = Cli::try_parse_from(["xavier", "agent", "pull", "--agent", "windsurf", "--json"])
            .unwrap();
        match cli.cmd {
            Some(Command::Agent {
                cmd: AgentCommand::Pull { agent, json },
            }) => {
                assert_eq!(agent, Some("windsurf".to_string()));
                assert!(json);
            }
            _ => panic!("Expected AgentCommand::Pull"),
        }
    }

    #[tokio::test]
    async fn test_handle_agent_scan_success_and_filter() {
        let _guard = ENV_LOCK.lock().await;
        let mut server = Server::new_async().await;
        std::env::set_var("XAVIER_URL", server.url());
        std::env::set_var("XAVIER_TOKEN", "test-token");

        let body = json!({
            "count": 2,
            "agents": [
                {"agent_id": "cursor-agent", "memory_md": true},
                {"agent_id": "windsurf-agent", "memory_md": false}
            ]
        });

        let mock = server
            .mock("GET", "/xavier/openclaw/scan")
            .match_header("X-Xavier-Token", "test-token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body.to_string())
            .create_async()
            .await;

        let res = handle_agent_scan(Some("cursor".to_string()), true).await;
        assert!(res.is_ok());
        mock.assert_async().await;

        // Also test non-json path
        let mock_text = server
            .mock("GET", "/xavier/openclaw/scan")
            .match_header("X-Xavier-Token", "test-token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body.to_string())
            .create_async()
            .await;

        let res_text = handle_agent_scan(None, false).await;
        assert!(res_text.is_ok());
        mock_text.assert_async().await;

        std::env::remove_var("XAVIER_URL");
        std::env::remove_var("XAVIER_TOKEN");
    }

    #[tokio::test]
    async fn test_handle_agent_scan_error() {
        let _guard = ENV_LOCK.lock().await;
        let mut server = Server::new_async().await;
        std::env::set_var("XAVIER_URL", server.url());
        std::env::set_var("XAVIER_TOKEN", "test-token");

        let mock = server
            .mock("GET", "/xavier/openclaw/scan")
            .with_status(500)
            .with_body("Internal Server Error")
            .create_async()
            .await;

        let res_json = handle_agent_scan(None, true).await;
        assert!(res_json.is_ok());

        let res_text = handle_agent_scan(None, false).await;
        assert!(res_text.is_ok());

        mock.expect_at_least(2).assert_async().await;

        std::env::remove_var("XAVIER_URL");
        std::env::remove_var("XAVIER_TOKEN");
    }

    #[tokio::test]
    async fn test_handle_agent_index_success() {
        let _guard = ENV_LOCK.lock().await;
        let mut server = Server::new_async().await;
        std::env::set_var("XAVIER_URL", server.url());
        std::env::set_var("XAVIER_TOKEN", "test-token");

        let mock_codex = server
            .mock("POST", "/xavier/codex/index")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(json!({"indexed_count": 3}).to_string())
            .create_async()
            .await;

        let mock_jules = server
            .mock("POST", "/xavier/jules/index")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(json!({"indexed_count": 2}).to_string())
            .create_async()
            .await;

        let res = handle_agent_index(None, true, true, true).await;
        assert!(res.is_ok());

        mock_codex.assert_async().await;
        mock_jules.assert_async().await;

        // Default openclaw target when codex=false and jules=false
        let mock_openclaw = server
            .mock("POST", "/xavier/openclaw/index")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(json!({"indexed_count": 5}).to_string())
            .create_async()
            .await;

        let res_default = handle_agent_index(None, false, false, false).await;
        assert!(res_default.is_ok());

        mock_openclaw.assert_async().await;

        std::env::remove_var("XAVIER_URL");
        std::env::remove_var("XAVIER_TOKEN");
    }

    #[tokio::test]
    async fn test_handle_agent_index_error() {
        let _guard = ENV_LOCK.lock().await;
        let mut server = Server::new_async().await;
        std::env::set_var("XAVIER_URL", server.url());
        std::env::set_var("XAVIER_TOKEN", "test-token");

        let mock = server
            .mock("POST", "/xavier/openclaw/index")
            .with_status(500)
            .with_body("Index Error")
            .create_async()
            .await;

        let res = handle_agent_index(None, false, false, false).await;
        assert!(res.is_ok());

        mock.assert_async().await;

        std::env::remove_var("XAVIER_URL");
        std::env::remove_var("XAVIER_TOKEN");
    }

    #[tokio::test]
    async fn test_handle_agent_sync_push_and_pull() {
        let _guard = ENV_LOCK.lock().await;
        let mut server = Server::new_async().await;
        std::env::set_var("XAVIER_URL", server.url());
        std::env::set_var("XAVIER_TOKEN", "test-token");

        let mock_push = server
            .mock("POST", "/xavier/agents/sync")
            .match_body(mockito::Matcher::Json(json!({
                "mode": "push",
                "agent": "cursor"
            })))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(json!({"status": "ok", "stats": {"synced": 4}}).to_string())
            .create_async()
            .await;

        let res_push = handle_agent_sync(Some("cursor".to_string()), false, true).await;
        assert!(res_push.is_ok());
        mock_push.assert_async().await;

        let mock_pull = server
            .mock("POST", "/xavier/agents/sync")
            .match_body(mockito::Matcher::Json(json!({
                "mode": "pull",
                "agent": null
            })))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(json!({"status": "ok", "stats": {"synced": 2}}).to_string())
            .create_async()
            .await;

        let res_pull = handle_agent_sync(None, true, false).await;
        assert!(res_pull.is_ok());
        mock_pull.assert_async().await;

        std::env::remove_var("XAVIER_URL");
        std::env::remove_var("XAVIER_TOKEN");
    }

    #[tokio::test]
    async fn test_handle_agent_command_dispatch() {
        let _guard = ENV_LOCK.lock().await;
        let mut server = Server::new_async().await;
        std::env::set_var("XAVIER_URL", server.url());
        std::env::set_var("XAVIER_TOKEN", "test-token");

        let mock = server
            .mock("GET", "/xavier/openclaw/scan")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(json!({"count": 0, "agents": []}).to_string())
            .create_async()
            .await;

        let cmd = AgentCommand::Scan {
            agent: None,
            json: true,
        };
        let res = handle_agent_command(cmd).await;
        assert!(res.is_ok());
        mock.assert_async().await;

        std::env::remove_var("XAVIER_URL");
        std::env::remove_var("XAVIER_TOKEN");
    }
}
