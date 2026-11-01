xavier::isolate_test_process!();

use axum::extract::State;
use axum::http::HeaderMap;
use axum::{routing::post, Json, Router};
use serde_json::{json, Value};
use std::fs;
use std::process::Command;
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
        .route(
            "/memory/add",
            post(
                |headers: HeaderMap,
                 State(st): State<MockState>,
                 Json(_body): Json<Value>| async move {
                    let ws = headers
                        .get("X-Workspace-Id")
                        .map(|v| v.to_str().unwrap().to_string())
                        .unwrap_or_else(|| "default".to_string());
                    st.memory_spaces.lock().await.push(ws);
                    Json(json!({"status": "ok"}))
                },
            ),
        )
        .route(
            "/memory/search",
            post(
                |headers: HeaderMap,
                 State(st): State<MockState>,
                 Json(_body): Json<Value>| async move {
                    let ws = headers
                        .get("X-Workspace-Id")
                        .map(|v| v.to_str().unwrap().to_string())
                        .unwrap_or_else(|| "default".to_string());
                    st.searched_spaces.lock().await.push(ws);
                    Json(json!({"results": []}))
                },
            ),
        )
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let xavier_bin = env!("CARGO_BIN_EXE_xavier");
    let space_id = "test-space-id";

    // Create
    let status = Command::new(xavier_bin)
        .env("XAVIER_DATA_DIR", data_dir.to_str().unwrap())
        .arg("space")
        .arg("create")
        .arg(space_id)
        .arg("--local")
        .status()
        .expect("failed to execute xavier space create");
    assert!(status.success());

    // List
    let out = Command::new(xavier_bin)
        .env("XAVIER_DATA_DIR", data_dir.to_str().unwrap())
        .arg("space")
        .arg("list")
        .arg("--json")
        .arg("--local")
        .output()
        .expect("failed to execute xavier space list");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains(space_id));

    // Use
    let out_use = Command::new(xavier_bin)
        .env("XAVIER_DATA_DIR", data_dir.to_str().unwrap())
        .arg("space")
        .arg("use")
        .arg(space_id)
        .output()
        .expect("failed to execute xavier space use");
    assert!(out_use.status.success());
    assert!(String::from_utf8_lossy(&out_use.stdout)
        .contains(&format!("Active space set to {}", space_id)));

    // Store memory in it
    let out_add = Command::new(xavier_bin)
        .env("XAVIER_DATA_DIR", data_dir.to_str().unwrap())
        .env("XAVIER_URL", &url) // offline tests override URL via XAVIER_URL
        .arg("add")
        .arg("hello memory")
        .status()
        .expect("failed to execute xavier add");
    assert!(out_add.success());

    assert_eq!(state.memory_spaces.lock().await.last().unwrap(), space_id);

    // Search scoped to it
    let out_search = Command::new(xavier_bin)
        .env("XAVIER_DATA_DIR", data_dir.to_str().unwrap())
        .env("XAVIER_URL", &url)
        .arg("search")
        .arg("hello")
        .status()
        .expect("failed to execute xavier search");
    assert!(out_search.success());

    assert_eq!(state.searched_spaces.lock().await.last().unwrap(), space_id);

    // Delete
    let status_del = Command::new(xavier_bin)
        .env("XAVIER_DATA_DIR", data_dir.to_str().unwrap())
        .arg("space")
        .arg("delete")
        .arg(space_id)
        .arg("--local")
        .status()
        .expect("failed to execute xavier space delete");
    assert!(status_del.success());

    // List lacks it
    let out2 = Command::new(xavier_bin)
        .env("XAVIER_DATA_DIR", data_dir.to_str().unwrap())
        .arg("space")
        .arg("list")
        .arg("--json")
        .arg("--local")
        .output()
        .expect("failed to execute xavier space list after delete");
    assert!(out2.status.success());
    assert!(!String::from_utf8_lossy(&out2.stdout).contains(space_id));
}
