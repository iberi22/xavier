#![allow(unused_imports)]
#[path = "bounded_cli_support/mod.rs"]
mod bounded_cli_support;
use bounded_cli_support::Sandbox;
use reqwest::{Client, Method, StatusCode};
use serde_json::Value;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const ROOT: &str = "wp13o-e2e-root-token";
const JWT_SECRET: &str = "wp13o-e2e-jwt-secret-0123456789abcdef";

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port()
}

async fn wait_until_ready(base_url: &str, timeout: Duration) {
    let http = Client::new();
    let mut last_ready = String::new();
    let mut ready = false;
    let deadline = std::time::Instant::now() + timeout;
    let mut backoff = 100;
    while std::time::Instant::now() < deadline {
        let resp = http
            .get(format!("{base_url}/ready"))
            .timeout(Duration::from_secs(2))
            .send()
            .await;
        if let Ok(r) = resp {
            if let Ok(body) = r.text().await {
                // Only a parsed body that is not the gate's `starting` reply counts.
                if let Ok(v) = serde_json::from_str::<Value>(&body) {
                    if v.get("status").and_then(|s| s.as_str()) != Some("starting") {
                        ready = true;
                        break;
                    }
                }
                last_ready = body;
            }
        }
        tokio::time::sleep(Duration::from_millis(backoff)).await;
        backoff = std::cmp::min(250, backoff + 50);
    }
    assert!(
        ready,
        "daemon at {base_url} still starting after {timeout:?}; last /ready body: {last_ready}"
    );
}

struct Daemon {
    _child: ChildGuard,
    url: String,
    http: Client,
}

impl Daemon {
    async fn spawn(bin: &Path, home: &Path) -> Self {
        assert!(
            home.starts_with(std::env::temp_dir()),
            "scratch root must be an absolute temp path"
        );
        let port = free_port();
        assert_ne!(port, 8006, "never the live daemon port");
        let url = format!("http://127.0.0.1:{port}");
        for d in ["home", "data", "state", "ws", "cwd"] {
            std::fs::create_dir_all(home.join(d)).unwrap();
        }
        let mut cmd = Command::new(bin);
        for (k, _) in std::env::vars() {
            if k.starts_with("XAVIER_") || k.starts_with("XDG_") || k.starts_with("DBUS_") {
                cmd.env_remove(k);
            }
        }
        cmd.arg("http")
            .arg(port.to_string())
            .arg("--mcp-port")
            .arg("0")
            .current_dir(home.join("cwd"))
            .env("HOME", home.join("home"))
            .env("XDG_CONFIG_HOME", home.join("home/.config"))
            .env("XDG_DATA_HOME", home.join("home/.local/share"))
            .env("XDG_CACHE_HOME", home.join("home/.cache"))
            .env("XDG_STATE_HOME", home.join("home/.local/state"))
            .env("XAVIER_HOST", "127.0.0.1")
            .env("XAVIER_PORT", port.to_string())
            .env("XAVIER_URL", &url)
            .env("XAVIER_MCP_PORT", "0")
            .env("XAVIER_TOKEN", ROOT)
            .env("XAVIER_JWT_SECRET", JWT_SECRET)
            .env("XAVIER_DATA_DIR", home.join("data"))
            .env("XAVIER_STATE_DIR", home.join("state"))
            .env("XAVIER_WORKSPACE_DIR", home.join("ws"))
            .env("XAVIER_INGESTION_INTERVAL_SECS", "0")
            .env("XAVIER_CODE_GRAPH_DB_PATH", home.join("data/codegraph.db"))
            .env(
                "XAVIER_MEMORY_VEC_PATH",
                home.join("data/vec-store.sqlite3"),
            )
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let child = cmd.spawn().expect("failed to spawn xavier binary");
        let guard = ChildGuard(child);
        let http = Client::new();
        let mut healthy = false;
        for _ in 0..120 {
            if let Ok(r) = http.get(format!("{url}/health")).send().await {
                if r.status().is_success() {
                    let body = r.text().await.unwrap_or_default();
                    if !body.contains("\"status\":\"starting\"") {
                        healthy = true;
                        break;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        assert!(healthy, "daemon not healthy at {url} within 60s");
        Self {
            _child: guard,
            url,
            http,
        }
    }
}

struct BinCopy(PathBuf);

impl BinCopy {
    fn new() -> Self {
        let src = PathBuf::from(env!("CARGO_BIN_EXE_xavier"));
        let dst = src.with_file_name(format!(
            "xavier-sp-cli-real-e2e-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        if std::fs::hard_link(&src, &dst).is_err() {
            std::fs::copy(&src, &dst).expect("copy daemon binary");
        }
        Self(dst)
    }
}

impl Drop for BinCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn scratch() -> (tempfile::TempDir, PathBuf, BinCopy) {
    let t = tempfile::tempdir().expect("tempdir");
    let p = t.path().to_path_buf();
    (t, p, BinCopy::new())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_space_cli_real_e2e() {
    //! feat-espacio-runtime: e2e test for xavier space CLI against a real server
    xavier::isolate_test_process!();

    let (_keep, home, bin) = scratch();
    let d = Daemon::spawn(&bin.0, &home).await;
    wait_until_ready(&d.url, Duration::from_secs(60)).await;

    let sandbox = Sandbox::new();

    // Create space `demo`
    let run = sandbox.run(
        sandbox
            .command(&d.url)
            .arg("space")
            .arg("create")
            .arg("demo")
            .arg("--label")
            .arg("a")
            .env("XAVIER_TOKEN", ROOT),
        Duration::from_secs(10),
    );
    assert!(
        run.status.success(),
        "create should succeed. Stderr: {}",
        run.stderr
    );
    assert_eq!(run.stdout.trim(), "Created space demo");

    // List spaces
    let run = sandbox.run(
        sandbox
            .command(&d.url)
            .arg("space")
            .arg("list")
            .arg("--json")
            .env("XAVIER_TOKEN", ROOT),
        Duration::from_secs(10),
    );
    assert!(
        run.status.success(),
        "list should succeed. Stderr: {}",
        run.stderr
    );
    assert!(run.stdout.contains("\"id\":\"demo\""));

    // Check space status
    let run = sandbox.run(
        sandbox
            .command(&d.url)
            .arg("space")
            .arg("status")
            .arg("demo")
            .env("XAVIER_TOKEN", ROOT),
        Duration::from_secs(10),
    );
    assert!(
        run.status.success(),
        "status should succeed. Stderr: {}",
        run.stderr
    );

    // Verify space.json exists in temp state dir
    let space_json_path = home.join("state/spaces/demo/space.json");
    assert!(
        space_json_path.exists(),
        "space.json should exist under the temp state dir"
    );

    // Use space
    let run = sandbox.run(
        sandbox
            .command(&d.url)
            .arg("space")
            .arg("use")
            .arg("demo")
            .env("XAVIER_TOKEN", ROOT),
        Duration::from_secs(10),
    );
    assert!(
        run.status.success(),
        "use should succeed. Stderr: {}",
        run.stderr
    );
    assert!(run.stdout.contains("Active space set to demo"));

    // Attempt second create (should fail because space 'demo' already exists)
    let run = sandbox.run(
        sandbox
            .command(&d.url)
            .arg("space")
            .arg("create")
            .arg("demo")
            .arg("--label")
            .arg("a")
            .env("XAVIER_TOKEN", ROOT),
        Duration::from_secs(10),
    );
    assert!(!run.status.success(), "second create should fail");
    assert!(
        run.stderr.contains("already exists")
            || run.stderr.contains("conflict")
            || run.stderr.contains("Failed to create space"),
        "Stderr: {}",
        run.stderr
    );

    // Delete space
    let run = sandbox.run(
        sandbox
            .command(&d.url)
            .arg("space")
            .arg("delete")
            .arg("demo")
            .env("XAVIER_TOKEN", ROOT),
        Duration::from_secs(10),
    );
    assert!(
        run.status.success(),
        "delete should succeed. Stderr: {}",
        run.stderr
    );
    assert_eq!(run.stdout.trim(), "Deleted space demo");

    // List spaces after delete
    let run = sandbox.run(
        sandbox
            .command(&d.url)
            .arg("space")
            .arg("list")
            .arg("--json")
            .env("XAVIER_TOKEN", ROOT),
        Duration::from_secs(10),
    );
    assert!(
        run.status.success(),
        "list after delete should succeed. Stderr: {}",
        run.stderr
    );
    assert!(!run.stdout.contains("\"id\":\"demo\""));
}
