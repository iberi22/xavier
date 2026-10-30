//! <ledger id>: feat-cli-health-stats-parity

use axum::{routing::get, Router};
use std::process::Command;
use tempfile::TempDir;
use tokio::net::TcpListener;

xavier::isolate_test_process!();

struct CliTestEnv {
    root: TempDir,
}

impl CliTestEnv {
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        for dir in ["home", "data", "state", "work"] {
            std::fs::create_dir(root.path().join(dir)).unwrap();
        }
        let config = serde_json::json!({
            "license": { "mesh_accepted": true, "license_type": "AGPL-3.0" }
        });
        std::fs::write(root.path().join("config.json"), config.to_string()).unwrap();
        Self { root }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_xavier"));
        command
            .env_clear()
            .env("HOME", self.root.path().join("home"))
            .env("XAVIER_CONFIG_PATH", self.root.path().join("config.json"))
            .env(
                "XAVIER_RUNTIME_STATE_PATH",
                self.root.path().join("state/runtime.json"),
            )
            .env("XAVIER_DATA_DIR", self.root.path().join("data"))
            .env("XAVIER_STATE_DIR", self.root.path().join("state"))
            .env("XAVIER_TOKEN", "test_token")
            .current_dir(self.root.path().join("work"));
        command
    }
}

// Multi_thread + serial override is needed since we spawn an axum server.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn test_tasks_list_json() {
    let env = CliTestEnv::new();

    let router = Router::new()
        .route(
            "/v1/tasks",
            get(|| async { axum::Json(serde_json::json!([{"id": "t1", "title": "task1", "project": "p1", "status": "todo", "priority": "high"}])) }),
        );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let output = env
        .command()
        .env("XAVIER_URL", format!("http://{addr}"))
        .arg("tasks")
        .arg("list")
        .arg("--json")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v[0]["id"], "t1");
    server_task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn test_tasks_list_json_request_errors() {
    let env = CliTestEnv::new();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    for url in [format!("http://{addr}"), "http://[".to_string()] {
        let text = env
            .command()
            .env("XAVIER_URL", &url)
            .args(["tasks", "list"])
            .output()
            .unwrap();
        let json = env
            .command()
            .env("XAVIER_URL", &url)
            .args(["tasks", "list", "--json"])
            .output()
            .unwrap();

        assert_eq!(text.status.code(), Some(1));
        assert_eq!(json.status.code(), text.status.code());
        let error: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
        assert_eq!(error.as_object().unwrap().len(), 1);
        assert!(!error["error"].as_str().unwrap().is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn test_tasks_list_json_http_error() {
    let env = CliTestEnv::new();
    let router = Router::new().route(
        "/v1/tasks",
        get(|| async {
            (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                axum::Json(serde_json::json!({ "message": "Unavailable" })),
            )
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let text = env
        .command()
        .env("XAVIER_URL", format!("http://{addr}"))
        .args(["tasks", "list"])
        .output()
        .unwrap();
    let json = env
        .command()
        .env("XAVIER_URL", format!("http://{addr}"))
        .args(["tasks", "list", "--json"])
        .output()
        .unwrap();

    assert_eq!(text.status.code(), Some(0));
    assert_eq!(json.status.code(), text.status.code());
    let error: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(error, serde_json::json!({ "error": "Unavailable" }));
    server_task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn test_plugin_list_json() {
    let env = CliTestEnv::new();
    let output = env
        .command()
        .env("XAVIER_PLUGINS_INDEX", "invalid") // use embedded
        .arg("plugin")
        .arg("list")
        .arg("--json")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(v
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["name"] == "codegraph"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn test_governance_list_json() {
    let env = CliTestEnv::new();
    let output = env
        .command()
        .arg("governance")
        .arg("list")
        .arg("--json")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(v.as_array().is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn test_governance_status_json() {
    let env = CliTestEnv::new();
    let output = env
        .command()
        .arg("governance")
        .arg("status")
        .arg("nonexistent")
        .arg("--json")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(v["error"].as_str().is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn test_mini_expert_list_json() {
    let env = CliTestEnv::new();
    let output = env
        .command()
        .arg("mini-expert")
        .arg("list")
        .arg("--json")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(v.as_array().is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn test_repo_config_show_json() {
    let env = CliTestEnv::new();
    let output = env
        .command()
        .arg("repo")
        .arg("config")
        .arg("show")
        .arg("-C")
        .arg(env.root.path().join("data"))
        .arg("--json")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(v["project_id_origin"].as_str().is_some());
}
