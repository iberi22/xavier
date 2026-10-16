//! Dump path precedence through the real CLI and HTTP daemon, without indexing.
#[allow(unused_imports)] // Shared helper also re-exports its optional mock server.
mod bounded_cli_support;

use bounded_cli_support::Sandbox;
use serde_json::{json, Value};
use std::fs::{self, File};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

xavier::isolate_test_process!();

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn repository(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    fs::create_dir_all(path.join(".git")).expect("create isolated repo marker");
    path.canonicalize().expect("canonical temp repo")
}

fn assert_dump(body: &Value, requested: &Path, resolved: &Path) {
    assert_eq!(body["status"], "ok", "{body}");
    assert_eq!(
        body["requested_path"],
        requested.to_str().unwrap(),
        "{body}"
    );
    assert_eq!(body["resolved_path"], resolved.to_str().unwrap(), "{body}");
    let expected = resolved.join(".xavier/codegraph.json");
    assert_eq!(body["path"], expected.to_str().unwrap(), "{body}");
    let dump: Value = serde_json::from_slice(&fs::read(expected).expect("read reported dump"))
        .expect("reported dump is JSON");
    assert!(dump["symbols"].is_array(), "dump must contain the graph");
}

#[tokio::test]
async fn dump_uses_explicit_path_then_cli_cwd_then_daemon_default() {
    // One shared budget covers startup, both CLI processes and HTTP requests.
    let deadline = Instant::now() + Duration::from_secs(27);
    let sandbox = Sandbox::new();
    let workspace = repository(sandbox.0.path(), "workspace");
    let daemon_cwd = repository(&workspace, "daemon");
    let caller_cwd = repository(&workspace, "caller");
    let explicit = repository(&caller_cwd, "explicit");
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve random port");
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let url = format!("http://127.0.0.1:{port}");
    let log = File::create(sandbox.0.path().join("daemon.log")).unwrap();
    let mut daemon = Daemon(
        sandbox
            .command(&url)
            .current_dir(&daemon_cwd)
            .env("XAVIER_WORKSPACE_DIR", &workspace)
            .env("XAVIER_HOST", "127.0.0.1")
            .env("XAVIER_PORT", port.to_string())
            .env("XAVIER_MCP_PORT", "0")
            .env("XAVIER_CODE_GRAPH_SKIP_EMBED", "1")
            .env("XAVIER_INGESTION_INTERVAL_SECS", "0")
            .env("XAVIER_AGENT_SCANNER_INTERVAL", "0")
            .arg("http")
            .arg(port.to_string())
            .args(["--mcp-port", "0"])
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .expect("spawn real xavier HTTP daemon"),
    );
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(1))
        .build()
        .unwrap();
    let boot_deadline = Instant::now() + Duration::from_secs(15);
    loop {
        assert!(
            daemon.0.try_wait().unwrap().is_none(),
            "daemon exited during boot"
        );
        if let Ok(response) = client.get(format!("{url}/health")).send().await {
            if response.status().is_success() {
                break;
            }
        }
        assert!(
            Instant::now() < boot_deadline,
            "daemon did not become ready within 15s"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    for (arg, expected) in [(Some("explicit"), &explicit), (None, &caller_cwd)] {
        let mut command = sandbox.command(&url);
        command.current_dir(&caller_cwd).args(["code", "dump"]);
        if let Some(arg) = arg {
            command.arg(arg);
        }
        let run = sandbox.run(
            &mut command,
            deadline.saturating_duration_since(Instant::now()),
        );
        assert!(run.status.success(), "{run:?}");
        let body: Value = serde_json::from_str(&run.stdout).expect("CLI prints dump response JSON");
        // The CLI canonicalizes before sending: requested_path is the absolute
        // wire path, even when the positional argument was relative or absent.
        assert_dump(&body, expected, expected);
        assert!(!daemon_cwd.join(".xavier/codegraph.json").exists());
        assert!(!workspace.join(".xavier/codegraph.json").exists());
    }

    // Only a request with no path exercises the daemon's default. The CLI
    // always sends its own cwd, so an absent CLI argument cannot reach it.
    let response = client
        .post(format!("{url}/code/dump"))
        .header("X-Xavier-Token", "bounded-test-token")
        .json(&json!({}))
        .timeout(deadline.saturating_duration_since(Instant::now()))
        .send()
        .await
        .expect("request daemon default dump");
    assert!(response.status().is_success());
    let body: Value = response.json().await.expect("default dump JSON");
    assert_dump(&body, Path::new("."), &daemon_cwd);
    assert!(!workspace.join(".xavier/codegraph.json").exists());
    assert!(
        Instant::now() < deadline,
        "dump contract exceeded total budget"
    );
}
