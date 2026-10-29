use axum::{routing::get, Router};
use std::process::Command;
use tempfile::TempDir;
use tokio::net::TcpListener;

xavier::isolate_test_process!();

#[tokio::test]
#[serial_test::serial]
async fn test_provider_status_exit_codes() {
    let tmp_config = TempDir::new().unwrap();
    let tmp_data = TempDir::new().unwrap();

    let router = Router::new()
        .route(
            "/headless/provider/status",
            get(|| async { axum::Json(serde_json::json!({"status": "ok"})) }),
        )
        .route(
            "/401/headless/provider/status",
            get(|| async { axum::http::StatusCode::UNAUTHORIZED }),
        );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // Wait briefly for server to start up
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Disable headless to bypass local overrides and config logic and ensure pure CLI behavior
    let xavier_bin = env!("CARGO_BIN_EXE_xavier");

    let mut envs: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    envs.insert("XAVIER_TOKEN".to_string(), "test_token".to_string());
    envs.insert(
        "XAVIER_DATA_DIR".to_string(),
        tmp_data.path().to_string_lossy().to_string(),
    );
    envs.insert(
        "HOME".to_string(),
        tmp_config.path().to_string_lossy().to_string(),
    );
    envs.insert("XAVIER_HOST".to_string(), "127.0.0.1".to_string());
    envs.insert("XAVIER_PORT".to_string(), addr.port().to_string());

    // 1. Success (200) -> Exit 0
    let url_200 = format!("http://127.0.0.1:{}", addr.port());

    let status = Command::new(xavier_bin)
        .envs(&envs)
        .env("XAVIER_URL", &url_200)
        .arg("provider")
        .arg("status")
        .status()
        .unwrap();

    assert_eq!(status.code().unwrap(), 0);

    // 2. Unauthorized (401) -> Exit 3
    let url_401 = format!("http://127.0.0.1:{}/401", addr.port());

    let status = Command::new(xavier_bin)
        .envs(&envs)
        .env("XAVIER_URL", &url_401)
        .arg("provider")
        .arg("status")
        .status()
        .unwrap();

    assert_eq!(status.code().unwrap(), 3);

    // 3. Closed port / Connection Refused -> Exit 4
    let dummy_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dummy_port = dummy_listener.local_addr().unwrap().port();
    drop(dummy_listener); // ensure it's closed

    let url_closed = format!("http://127.0.0.1:{}", dummy_port);
    envs.insert("XAVIER_PORT".to_string(), dummy_port.to_string());

    let status = Command::new(xavier_bin)
        .envs(&envs)
        .env("XAVIER_URL", &url_closed)
        .arg("provider")
        .arg("status")
        .status()
        .unwrap();

    assert_eq!(status.code().unwrap(), 4);

    server_task.abort();
}
