//! <ledger id>: feat-cli-health-stats-parity

use axum::{routing::get, Router};
use std::process::Command;
use tempfile::TempDir;
use tokio::net::TcpListener;

// Multi_thread + serial override is needed since we spawn an axum server.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn test_tasks_list_json() {
    let tmp_config = TempDir::new().unwrap();
    let tmp_data = TempDir::new().unwrap();

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

    let xavier_bin = env!("CARGO_BIN_EXE_xavier");
    let mut envs = std::collections::HashMap::new();
    envs.insert("XAVIER_TOKEN".to_string(), "test_token".to_string());
    envs.insert(
        "XAVIER_DATA_DIR".to_string(),
        tmp_data.path().to_string_lossy().to_string(),
    );
    envs.insert(
        "HOME".to_string(),
        tmp_config.path().to_string_lossy().to_string(),
    );
    let url = format!("http://127.0.0.1:{}", addr.port());
    envs.insert("XAVIER_URL".to_string(), url);

    let output = Command::new(xavier_bin)
        .envs(&envs)
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
async fn test_plugin_list_json() {
    let tmp_config = TempDir::new().unwrap();
    let tmp_data = TempDir::new().unwrap();
    let xavier_bin = env!("CARGO_BIN_EXE_xavier");
    let mut envs = std::collections::HashMap::new();
    envs.insert(
        "XAVIER_DATA_DIR".to_string(),
        tmp_data.path().to_string_lossy().to_string(),
    );
    envs.insert(
        "HOME".to_string(),
        tmp_config.path().to_string_lossy().to_string(),
    );
    envs.insert("XAVIER_PLUGINS_INDEX".to_string(), "invalid".to_string()); // use embedded

    let output = Command::new(xavier_bin)
        .envs(&envs)
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
    let tmp_config = TempDir::new().unwrap();
    let tmp_data = TempDir::new().unwrap();
    let xavier_bin = env!("CARGO_BIN_EXE_xavier");
    let mut envs = std::collections::HashMap::new();
    envs.insert(
        "XAVIER_DATA_DIR".to_string(),
        tmp_data.path().to_string_lossy().to_string(),
    );
    envs.insert(
        "HOME".to_string(),
        tmp_config.path().to_string_lossy().to_string(),
    );

    let output = Command::new(xavier_bin)
        .envs(&envs)
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
    let tmp_config = TempDir::new().unwrap();
    let tmp_data = TempDir::new().unwrap();
    let xavier_bin = env!("CARGO_BIN_EXE_xavier");
    let mut envs = std::collections::HashMap::new();
    envs.insert(
        "XAVIER_DATA_DIR".to_string(),
        tmp_data.path().to_string_lossy().to_string(),
    );
    envs.insert(
        "HOME".to_string(),
        tmp_config.path().to_string_lossy().to_string(),
    );

    let output = Command::new(xavier_bin)
        .envs(&envs)
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
    let tmp_config = TempDir::new().unwrap();
    let tmp_data = TempDir::new().unwrap();
    let xavier_bin = env!("CARGO_BIN_EXE_xavier");
    let mut envs = std::collections::HashMap::new();
    envs.insert(
        "XAVIER_DATA_DIR".to_string(),
        tmp_data.path().to_string_lossy().to_string(),
    );
    envs.insert(
        "HOME".to_string(),
        tmp_config.path().to_string_lossy().to_string(),
    );

    let output = Command::new(xavier_bin)
        .envs(&envs)
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
    let tmp_config = TempDir::new().unwrap();
    let tmp_data = TempDir::new().unwrap();
    let xavier_bin = env!("CARGO_BIN_EXE_xavier");
    let mut envs = std::collections::HashMap::new();
    envs.insert(
        "XAVIER_DATA_DIR".to_string(),
        tmp_data.path().to_string_lossy().to_string(),
    );
    envs.insert(
        "HOME".to_string(),
        tmp_config.path().to_string_lossy().to_string(),
    );

    let output = Command::new(xavier_bin)
        .envs(&envs)
        .arg("repo")
        .arg("config")
        .arg("show")
        .arg("-C")
        .arg(tmp_data.path().to_string_lossy().to_string())
        .arg("--json")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(v["project_id_origin"].as_str().is_some());
}
