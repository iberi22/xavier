//! feat-codegraph-reindex: Incremental reindexing tests

xavier::isolate_test_process!();

use axum::{routing::post, Json, Router};
use serde_json::json;
use std::fs;
use std::process::Command;
use tempfile::tempdir;

#[tokio::test(flavor = "multi_thread")]
async fn test_codegraph_reindex_and_index_self_e2e() {
    let app = Router::new()
        .route(
            "/code/index-self",
            post(|| async { Json(json!({"status": "ok", "message": "Index self complete"})) }),
        )
        .route(
            "/code/scan",
            post(|| async { Json(json!({"status": "ok", "message": "Scan complete"})) }),
        )
        .route(
            "/code/reindex",
            post(|| async {
                Json(json!({
                    "status": "ok",
                    "stats": {
                        "total_files": 3,
                        "updated": 1,
                        "removed": 1,
                        "added": 1,
                        "unchanged": 0
                    },
                    "message": "Reindex complete"
                }))
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let tmp = tempdir().unwrap();
    let repo = tmp.path().join("myrepo");
    fs::create_dir_all(&repo).unwrap();

    let rust_file = repo.join("main.rs");
    fs::write(&rust_file, "fn main() { println!(\"Hello\"); }").unwrap();

    let python_file = repo.join("script.py");
    fs::write(&python_file, "def foo(): pass").unwrap();

    let ts_file = repo.join("app.ts");
    fs::write(&ts_file, "function bar() {}").unwrap();

    // Index Self
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xavier"));
    let output = cmd
        .current_dir(&repo)
        .env("XAVIER_CLI_TEST", "1")
        .env("XAVIER_URL", &url)
        .env("XAVIER_TOKEN", "test-token")
        .arg("code")
        .arg("index-self")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "index-self failed: stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Initial Scan
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xavier"));
    let output = cmd
        .current_dir(&repo)
        .env("XAVIER_CLI_TEST", "1")
        .env("XAVIER_URL", &url)
        .env("XAVIER_TOKEN", "test-token")
        .arg("code")
        .arg("scan")
        .arg(".")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "scan failed: stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Modify Rust file
    fs::write(&rust_file, "fn main() { println!(\"Hello World\"); }").unwrap();

    // Delete Python file
    fs::remove_file(&python_file).unwrap();

    // Add new TS file
    let ts_file2 = repo.join("new.ts");
    fs::write(&ts_file2, "function baz() {}").unwrap();

    // Reindex
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xavier"));
    let output = cmd
        .current_dir(&repo)
        .env("XAVIER_CLI_TEST", "1")
        .env("XAVIER_URL", &url)
        .env("XAVIER_TOKEN", "test-token")
        .arg("code")
        .arg("reindex")
        .arg("--path")
        .arg(".")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "reindex failed: stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout_str = String::from_utf8(output.stdout).unwrap();
    assert!(stdout_str.contains("\"status\": \"ok\""));
}
