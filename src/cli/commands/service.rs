use crate::cli::commands::enums::ServiceCommand;
use anyhow::Result;
use serde_json::json;
use std::env;
use std::process::Command;
use std::time::Duration;

fn systemctl_cmd() -> String {
    env::var("XAVIER_SYSTEMCTL_CMD").unwrap_or_else(|_| "systemctl".to_string())
}

fn journalctl_cmd() -> String {
    env::var("XAVIER_JOURNALCTL_CMD").unwrap_or_else(|_| "journalctl".to_string())
}

pub async fn run_service_cmd(cmd: &ServiceCommand) -> Result<()> {
    match cmd {
        ServiceCommand::Status { json } => {
            let output = Command::new(systemctl_cmd())
                .args(["--user", "status", "xavier.service"])
                .output()?;
            let is_active = output.status.success();
            if *json {
                println!(
                    "{}",
                    serde_json::to_string(&json!({
                        "status": if is_active { "active" } else { "inactive" },
                        "details": String::from_utf8_lossy(&output.stdout).trim()
                    }))?
                );
            } else {
                eprintln!("{}", String::from_utf8_lossy(&output.stdout));
            }
            if !is_active {
                std::process::exit(1);
            }
        }
        ServiceCommand::Restart { timeout, json } => {
            let mut restart_cmd = Command::new(systemctl_cmd());
            restart_cmd.args(["--user", "restart", "xavier.service"]);
            let status = restart_cmd.status()?;
            if !status.success() {
                if *json {
                    println!(
                        "{}",
                        serde_json::to_string(&json!({ "error": "failed to restart service" }))?
                    );
                } else {
                    eprintln!("failed to restart service");
                }
                std::process::exit(1);
            }

            // Polling for readiness
            let start = std::time::Instant::now();
            let timeout_dur = Duration::from_secs(*timeout);
            let mut ready = false;
            let http_client = reqwest::Client::new();
            let url = format!(
                "{}/ready",
                crate::cli::config::resolve_base_url().trim_end_matches('/')
            );

            while start.elapsed() < timeout_dur {
                if let Ok(resp) = http_client.get(&url).send().await {
                    if resp.status().is_success() {
                        ready = true;
                        break;
                    }
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }

            if *json {
                println!("{}", serde_json::to_string(&json!({ "ready": ready }))?);
            } else if ready {
                eprintln!("Service restarted successfully.");
            } else {
                eprintln!("Service restarted but did not become ready in time.");
            }

            if !ready {
                std::process::exit(1);
            }
        }
        ServiceCommand::Reload { json } => {
            let mut cmd = Command::new(systemctl_cmd());
            cmd.args(["--user", "reload", "xavier.service"]);
            let status = cmd.status().unwrap_or_else(|_| {
                Command::new(systemctl_cmd())
                    .args(["--user", "restart", "xavier.service"])
                    .status()
                    .expect("Failed to restart service")
            });
            if !status.success() {
                std::process::exit(1);
            }
            if *json {
                println!(
                    "{}",
                    serde_json::to_string(&json!({ "status": "reloaded" }))?
                );
            } else {
                eprintln!("Service reloaded.");
            }
        }
        ServiceCommand::Start { json } => {
            let mut cmd = Command::new(systemctl_cmd());
            cmd.args(["--user", "start", "xavier.service"]);
            let status = cmd.status()?;
            if !status.success() {
                std::process::exit(1);
            }
            if *json {
                println!(
                    "{}",
                    serde_json::to_string(&json!({ "status": "started" }))?
                );
            } else {
                eprintln!("Service started.");
            }
        }
        ServiceCommand::Stop { json } => {
            let mut cmd = Command::new(systemctl_cmd());
            cmd.args(["--user", "stop", "xavier.service"]);
            let status = cmd.status()?;
            if !status.success() {
                std::process::exit(1);
            }
            if *json {
                println!(
                    "{}",
                    serde_json::to_string(&json!({ "status": "stopped" }))?
                );
            } else {
                eprintln!("Service stopped.");
            }
        }
    }
    Ok(())
}

pub async fn run_logs_cmd(
    since: &Option<String>,
    grep: &Option<String>,
    lines: &Option<usize>,
    follow: bool,
    json: bool,
) -> Result<()> {
    let mut cmd = Command::new(journalctl_cmd());
    cmd.args(["--user", "-u", "xavier.service"]);
    if let Some(s) = since {
        cmd.arg("--since").arg(s);
    }
    if let Some(g) = grep {
        cmd.arg("--grep").arg(g);
    }
    if let Some(l) = lines {
        cmd.arg("-n").arg(l.to_string());
    }
    if follow {
        cmd.arg("-f");
    }
    if json {
        cmd.arg("-o").arg("json");
    }

    let mut child = cmd.spawn()?;
    let status = child.wait()?;
    if !status.success() {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;
    use tokio::task;

    #[tokio::test]
    #[serial_test::serial]
    async fn test_service_status_json() {
        let temp_dir = TempDir::new().unwrap();
        let mock_systemctl_path = temp_dir.path().join("mock_systemctl.sh");

        let mock_script = "#!/bin/sh\nif [ \"$1\" = \"--user\" ] && [ \"$2\" = \"status\" ] && [ \"$3\" = \"xavier.service\" ]; then\n    echo \"mock status output\"\n    exit 0\nfi\nexit 1\n";
        fs::write(&mock_systemctl_path, mock_script).unwrap();
        let mut perms = fs::metadata(&mock_systemctl_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&mock_systemctl_path, perms).unwrap();

        env::set_var("XAVIER_SYSTEMCTL_CMD", mock_systemctl_path);

        let cmd = ServiceCommand::Status { json: true };
        let result = run_service_cmd(&cmd).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_logs_json() {
        let temp_dir = TempDir::new().unwrap();
        let mock_journalctl_path = temp_dir.path().join("mock_journalctl.sh");

        let mock_script = "#!/bin/sh\necho '{\"message\": \"test log\"}'\nexit 0\n";
        fs::write(&mock_journalctl_path, mock_script).unwrap();
        let mut perms = fs::metadata(&mock_journalctl_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&mock_journalctl_path, perms).unwrap();

        env::set_var("XAVIER_JOURNALCTL_CMD", mock_journalctl_path);

        let result = run_logs_cmd(&None, &None, &None, false, true).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_service_restart() {
        let temp_dir = TempDir::new().unwrap();
        let mock_systemctl_path = temp_dir.path().join("mock_systemctl2.sh");

        let mock_script = "#!/bin/sh\nif [ \"$1\" = \"--user\" ] && [ \"$2\" = \"restart\" ] && [ \"$3\" = \"xavier.service\" ]; then\n    exit 0\nfi\nexit 1\n";
        fs::write(&mock_systemctl_path, mock_script).unwrap();
        let mut perms = fs::metadata(&mock_systemctl_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&mock_systemctl_path, perms).unwrap();

        env::set_var("XAVIER_SYSTEMCTL_CMD", mock_systemctl_path);

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        env::set_var("XAVIER_URL", format!("http://127.0.0.1:{}", port));

        let addr = format!("127.0.0.1:{}", port);

        let router = axum::Router::new().route(
            "/ready",
            axum::routing::get(|| async { axum::http::StatusCode::OK }),
        );

        let listener_task = task::spawn(async move {
            let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
            let _ = axum::serve(listener, router).await;
        });

        tokio::time::sleep(Duration::from_millis(100)).await;

        let cmd = ServiceCommand::Restart {
            timeout: 2,
            json: true,
        };
        let result = run_service_cmd(&cmd).await;

        listener_task.abort();
        env::remove_var("XAVIER_URL");

        assert!(result.is_ok());
    }
}
