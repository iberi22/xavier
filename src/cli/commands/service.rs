use crate::cli::commands::enums::ServiceCommand;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::env;
use std::io::Write;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

fn systemctl_cmd() -> String {
    env::var("XAVIER_SYSTEMCTL_CMD").unwrap_or_else(|_| "systemctl".to_string())
}

fn journalctl_cmd() -> String {
    env::var("XAVIER_JOURNALCTL_CMD").unwrap_or_else(|_| "journalctl".to_string())
}

fn systemctl(action: &str) -> Result<Output> {
    Command::new(systemctl_cmd())
        .args(["--user", action, "xavier.service", "--no-pager"])
        .output()
        .with_context(|| format!("failed to execute systemctl {action}"))
}

fn require_success(action: &str, output: &Output) -> Result<()> {
    if !output.status.success() {
        bail!(
            "systemctl {action} failed ({}): {} {}",
            output.status,
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

async fn service_response(cmd: &ServiceCommand) -> Result<(Value, i32)> {
    match cmd {
        ServiceCommand::Status { .. } => {
            let output = systemctl("status")?;
            let active = output.status.success();
            Ok((
                json!({
                    "status": if active { "active" } else { "inactive" },
                    "details": String::from_utf8_lossy(&output.stdout).trim(),
                    "stderr": String::from_utf8_lossy(&output.stderr).trim()
                }),
                if active { 0 } else { 1 },
            ))
        }
        ServiceCommand::Restart { timeout, .. } => {
            require_success("restart", &systemctl("restart")?)?;
            let start = Instant::now();
            let timeout_dur = Duration::from_secs(*timeout);
            let http_client = reqwest::Client::new();
            let url = format!(
                "{}/ready",
                crate::cli::config::resolve_base_url().trim_end_matches('/')
            );

            loop {
                let remaining = timeout_dur.saturating_sub(start.elapsed());
                if remaining.is_zero() {
                    break;
                }
                // Bound the entire request, including connection and response headers.
                if let Ok(resp) = http_client.get(&url).timeout(remaining).send().await {
                    if resp.status().is_success() {
                        return Ok((json!({ "ready": true }), 0));
                    }
                }
                let remaining = timeout_dur.saturating_sub(start.elapsed());
                tokio::time::sleep(Duration::from_millis(500).min(remaining)).await;
            }
            Ok((
                json!({
                    "ready": false,
                    "error": "service restarted but did not become ready in time"
                }),
                1,
            ))
        }
        ServiceCommand::Reload { .. } => {
            if let Ok(output) = systemctl("reload") {
                if output.status.success() {
                    return Ok((json!({ "status": "reloaded" }), 0));
                }
            }
            require_success("restart", &systemctl("restart")?)?;
            Ok((json!({ "status": "restarted" }), 0))
        }
        ServiceCommand::Start { .. } => {
            require_success("start", &systemctl("start")?)?;
            Ok((json!({ "status": "started" }), 0))
        }
        ServiceCommand::Stop { .. } => {
            require_success("stop", &systemctl("stop")?)?;
            Ok((json!({ "status": "stopped" }), 0))
        }
    }
}

async fn service_exit_code(cmd: &ServiceCommand, stdout: &mut impl Write) -> Result<i32> {
    let json = match cmd {
        ServiceCommand::Status { json }
        | ServiceCommand::Restart { json, .. }
        | ServiceCommand::Reload { json }
        | ServiceCommand::Start { json }
        | ServiceCommand::Stop { json } => *json,
    };
    let (response, code) = match service_response(cmd).await {
        Ok(response) => response,
        Err(error) => (json!({ "error": format!("{error:#}") }), 1),
    };
    if json {
        writeln!(stdout, "{response}")?;
    } else if let Some(error) = response["error"].as_str() {
        eprintln!("{error}");
    } else {
        match cmd {
            ServiceCommand::Status { .. } => {
                eprintln!("{}", response["details"].as_str().unwrap_or_default());
                eprintln!("{}", response["stderr"].as_str().unwrap_or_default());
            }
            ServiceCommand::Restart { .. } => eprintln!("Service restarted successfully."),
            ServiceCommand::Reload { .. } if response["status"] == "restarted" => {
                eprintln!("Service restarted (reload unavailable).");
            }
            ServiceCommand::Reload { .. } => eprintln!("Service reloaded."),
            ServiceCommand::Start { .. } => eprintln!("Service started."),
            ServiceCommand::Stop { .. } => eprintln!("Service stopped."),
        }
    }
    Ok(code)
}

pub async fn run_service_cmd(cmd: &ServiceCommand) -> Result<()> {
    if service_exit_code(cmd, &mut std::io::stdout()).await? != 0 {
        // Returning an error lets the CLI entry point choose exit code 1.
        bail!("service command failed");
    }
    Ok(())
}

fn logs_command(
    since: &Option<String>,
    grep: &Option<String>,
    lines: &Option<usize>,
    follow: bool,
    json: bool,
) -> Command {
    let mut cmd = Command::new(journalctl_cmd());
    cmd.args(["--user", "-u", "xavier.service", "--no-pager"]);
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
    cmd
}

fn logs_exit_code(cmd: &mut Command, json: bool, stdout: &mut impl Write) -> Result<i32> {
    // Inherit journal output so --follow streams JSON journal records immediately.
    let result = cmd
        .status()
        .context("failed to execute journalctl")
        .and_then(|status| {
            if !status.success() {
                bail!("journalctl failed ({status})");
            }
            Ok(())
        });
    if let Err(error) = result {
        if json {
            writeln!(stdout, "{}", json!({ "error": format!("{error:#}") }))?;
        } else {
            eprintln!("{error:#}");
        }
        return Ok(1);
    }
    Ok(0)
}

pub async fn run_logs_cmd(
    since: &Option<String>,
    grep: &Option<String>,
    lines: &Option<usize>,
    follow: bool,
    json: bool,
) -> Result<()> {
    let mut cmd = logs_command(since, grep, lines, follow, json);
    if logs_exit_code(&mut cmd, json, &mut std::io::stdout())? != 0 {
        bail!("logs command failed");
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use tempfile::TempDir;

    struct EnvGuard(Vec<(&'static str, Option<OsString>)>);

    impl EnvGuard {
        fn capture() -> Self {
            Self(
                [
                    "XAVIER_SYSTEMCTL_CMD",
                    "XAVIER_JOURNALCTL_CMD",
                    "XAVIER_URL",
                    "XAVIER_HOST",
                    "XAVIER_PORT",
                ]
                .into_iter()
                .map(|key| (key, env::var_os(key)))
                .collect(),
            )
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                match value {
                    Some(value) => env::set_var(key, value),
                    None => env::remove_var(key),
                }
            }
        }
    }

    fn mock_script(dir: &TempDir, body: &str) -> PathBuf {
        let path = dir.path().join("mock.sh");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    async fn response(cmd: ServiceCommand, expected_code: i32) -> Value {
        let mut stdout = Vec::new();
        let code = service_exit_code(&cmd, &mut stdout).await.unwrap();
        assert_eq!(code, expected_code);
        // Parsing the whole buffer rejects child output or multiple JSON objects.
        serde_json::from_slice(&stdout).unwrap()
    }

    #[tokio::test]
    #[serial]
    async fn service_success_captures_child_output() {
        let dir = TempDir::new().unwrap();
        let _env = EnvGuard::capture();
        let path = mock_script(&dir, "echo child-stdout; echo child-stderr >&2; exit 0");
        env::set_var("XAVIER_SYSTEMCTL_CMD", path);
        let status = response(ServiceCommand::Status { json: true }, 0).await;
        assert_eq!(status["status"], "active");
        assert_eq!(status["details"], "child-stdout");
        assert_eq!(status["stderr"], "child-stderr");
        assert_eq!(
            response(ServiceCommand::Start { json: true }, 0).await["status"],
            "started"
        );
        assert_eq!(
            response(ServiceCommand::Stop { json: true }, 0).await["status"],
            "stopped"
        );
        assert_eq!(
            response(ServiceCommand::Reload { json: true }, 0).await["status"],
            "reloaded"
        );
    }

    #[tokio::test]
    #[serial]
    async fn nonzero_systemctl_returns_failure_json() {
        let dir = TempDir::new().unwrap();
        let _env = EnvGuard::capture();
        env::set_var(
            "XAVIER_SYSTEMCTL_CMD",
            mock_script(&dir, "echo child-output; echo denied >&2; exit 3"),
        );
        assert_eq!(
            response(ServiceCommand::Status { json: true }, 1).await["status"],
            "inactive"
        );
        for cmd in [
            ServiceCommand::Start { json: true },
            ServiceCommand::Stop { json: true },
            ServiceCommand::Reload { json: true },
            ServiceCommand::Restart {
                timeout: 1,
                json: true,
            },
        ] {
            let value = response(cmd, 1).await;
            assert!(value["error"].as_str().unwrap().contains("denied"));
        }
        assert!(run_service_cmd(&ServiceCommand::Start { json: false })
            .await
            .is_err());
    }

    #[tokio::test]
    #[serial]
    async fn missing_systemctl_returns_failure_json_without_panicking() {
        let dir = TempDir::new().unwrap();
        let _env = EnvGuard::capture();
        env::set_var("XAVIER_SYSTEMCTL_CMD", dir.path().join("missing"));
        for cmd in [
            ServiceCommand::Status { json: true },
            ServiceCommand::Start { json: true },
            ServiceCommand::Stop { json: true },
            ServiceCommand::Reload { json: true },
            ServiceCommand::Restart {
                timeout: 1,
                json: true,
            },
        ] {
            assert!(response(cmd, 1).await["error"]
                .as_str()
                .unwrap()
                .contains("failed to execute systemctl"));
        }
    }

    #[tokio::test]
    #[serial]
    async fn reload_falls_back_to_restart_and_never_uses_a_pager() {
        let dir = TempDir::new().unwrap();
        let _env = EnvGuard::capture();
        env::set_var(
            "XAVIER_SYSTEMCTL_CMD",
            mock_script(
                &dir,
                r#"
[ "$1" = "--user" ] && [ "$3" = "xavier.service" ] && [ "$4" = "--no-pager" ] || exit 9
echo child-output
echo child-error >&2
case "$2" in
  reload) exit 1 ;;
  restart) exit 0 ;;
esac
exit 9
"#,
            ),
        );
        assert_eq!(
            response(ServiceCommand::Reload { json: true }, 0).await["status"],
            "restarted"
        );
    }

    #[tokio::test]
    #[serial]
    async fn restart_uses_configured_port() {
        let dir = TempDir::new().unwrap();
        let _env = EnvGuard::capture();
        env::set_var("XAVIER_SYSTEMCTL_CMD", mock_script(&dir, "exit 0"));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        env::remove_var("XAVIER_URL");
        env::set_var("XAVIER_HOST", "127.0.0.1");
        env::set_var(
            "XAVIER_PORT",
            listener.local_addr().unwrap().port().to_string(),
        );
        let server = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let n = stream.read(&mut request).await.unwrap();
            assert!(String::from_utf8_lossy(&request[..n]).starts_with("GET /ready "));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
        });
        let value = response(
            ServiceCommand::Restart {
                timeout: 2,
                json: true,
            },
            0,
        )
        .await;
        server.await.unwrap();
        assert_eq!(value["ready"], true);
    }

    #[tokio::test]
    #[serial]
    async fn readiness_deadline_bounds_a_server_that_never_responds() {
        let dir = TempDir::new().unwrap();
        let _env = EnvGuard::capture();
        env::set_var("XAVIER_SYSTEMCTL_CMD", mock_script(&dir, "exit 0"));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        env::set_var(
            "XAVIER_URL",
            format!("http://{}", listener.local_addr().unwrap()),
        );
        let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let _ = accepted_tx.send(());
            std::future::pending::<()>().await;
            drop(stream);
        });
        let start = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            response(
                ServiceCommand::Restart {
                    timeout: 1,
                    json: true,
                },
                1,
            ),
        )
        .await;
        server.abort();
        let value = result.expect("restart hung past its deadline");
        accepted_rx.await.unwrap();
        assert_eq!(value["ready"], false);
        assert!(value["error"].is_string());
        assert!(start.elapsed() < Duration::from_millis(1800));
    }

    #[tokio::test]
    #[serial]
    async fn journalctl_is_noninteractive_and_preserves_filters() {
        let dir = TempDir::new().unwrap();
        let _env = EnvGuard::capture();
        env::set_var("XAVIER_JOURNALCTL_CMD", mock_script(&dir, "exit 0"));
        let cmd = logs_command(
            &Some("1 hour ago".into()),
            &Some("error.*timeout".into()),
            &Some(17),
            true,
            true,
        );
        let args: Vec<_> = cmd.get_args().map(|arg| arg.to_str().unwrap()).collect();
        assert_eq!(
            args,
            [
                "--user",
                "-u",
                "xavier.service",
                "--no-pager",
                "--since",
                "1 hour ago",
                "--grep",
                "error.*timeout",
                "-n",
                "17",
                "-f",
                "-o",
                "json"
            ]
        );
        run_logs_cmd(&None, &None, &None, false, true)
            .await
            .unwrap();
        for binary in [mock_script(&dir, "exit 4"), dir.path().join("missing")] {
            env::set_var("XAVIER_JOURNALCTL_CMD", binary);
            let mut cmd = logs_command(&None, &None, &None, false, true);
            let mut stdout = Vec::new();
            assert_eq!(logs_exit_code(&mut cmd, true, &mut stdout).unwrap(), 1);
            let value: Value = serde_json::from_slice(&stdout).unwrap();
            assert!(value["error"].is_string());
            assert!(run_logs_cmd(&None, &None, &None, false, false)
                .await
                .is_err());
        }
    }
}
