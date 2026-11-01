use assert_cmd::Command;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::{
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::Mutex;

#[derive(Clone, Default)]
struct MockState {
    pub memory_spaces: Arc<Mutex<Vec<String>>>,
    pub searched_spaces: Arc<Mutex<Vec<String>>>,
}

#[tokio::test]
async fn test_q5_spaces_tenants_cli() {
    let temp = TempDir::new().unwrap();
    let data_dir = temp.path().join("data");
    fs::create_dir_all(&data_dir).unwrap();

    let state = MockState::default();

    let app = Router::new()
        .route("/v1/memory/add", post(
            |headers: HeaderMap, State(st): State<MockState>, Json(_body): Json<Value>| async move {
                let ws = headers.get("X-Workspace-Id").map(|v| v.to_str().unwrap().to_string()).unwrap_or("default".to_string());
                st.memory_spaces.lock().await.push(ws);
                Json(json!({"status": "ok"}))
            }
        ))
        .route("/v1/memory/search", post(
            |headers: HeaderMap, State(st): State<MockState>, Json(_body): Json<Value>| async move {
                let ws = headers.get("X-Workspace-Id").map(|v| v.to_str().unwrap().to_string()).unwrap_or("default".to_string());
                st.searched_spaces.lock().await.push(ws);
                Json(json!({"results": []}))
            }
        ))
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let space_id = "test-space-id";

    // Create
    Command::cargo_bin("xavier")
        .unwrap()
        .env("XAVIER_DATA_DIR", data_dir.to_str().unwrap())
        .arg("space")
        .arg("create")
        .arg(space_id)
        .arg("--local")
        .assert()
        .success();

    // List
    let out = Command::cargo_bin("xavier")
        .unwrap()
        .env("XAVIER_DATA_DIR", data_dir.to_str().unwrap())
        .arg("space")
        .arg("list")
        .arg("--json")
        .arg("--local")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(String::from_utf8_lossy(&out).contains(space_id));

    // Use
    Command::cargo_bin("xavier")
        .unwrap()
        .env("XAVIER_DATA_DIR", data_dir.to_str().unwrap())
        .arg("space")
        .arg("use")
        .arg(space_id)
        .assert()
        .success()
        .stdout(predicates::str::contains(format!(
            "Active space set to {}",
            space_id
        )));

    // Store memory in it
    Command::cargo_bin("xavier")
        .unwrap()
        .env("XAVIER_DATA_DIR", data_dir.to_str().unwrap())
        .env("XAVIER_URL", &url) // offline tests override URL via XAVIER_URL
        .arg("add")
        .arg("hello memory")
        .assert()
        .success();

    assert_eq!(state.memory_spaces.lock().await.last().unwrap(), space_id);

    // Search scoped to it
    Command::cargo_bin("xavier")
        .unwrap()
        .env("XAVIER_DATA_DIR", data_dir.to_str().unwrap())
        .env("XAVIER_URL", &url)
        .arg("search")
        .arg("hello")
        .assert()
        .success();

    assert_eq!(state.searched_spaces.lock().await.last().unwrap(), space_id);

    // Delete
    Command::cargo_bin("xavier")
        .unwrap()
        .env("XAVIER_DATA_DIR", data_dir.to_str().unwrap())
        .arg("space")
        .arg("delete")
        .arg(space_id)
        .arg("--local")
        .assert()
        .success();

    // List lacks it
    let out2 = Command::cargo_bin("xavier")
        .unwrap()
        .env("XAVIER_DATA_DIR", data_dir.to_str().unwrap())
        .arg("space")
        .arg("list")
        .arg("--json")
        .arg("--local")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(!String::from_utf8_lossy(&out2).contains(space_id));
}
