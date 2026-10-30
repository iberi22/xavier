//! <ledger id>: feat-repo-memory-instance
//! Tests for the CLI xavier add --path flag.

#![cfg(any(test, feature = "test-utils"))]

#[path = "router_harness/mod.rs"]
mod router_harness;

xavier::isolate_test_process!();

use std::process::Command;
use xavier::security::auth::UserRole;

#[tokio::test]
#[serial_test::serial]
async fn test_add_path_valid() {
    let tmp = tempfile::tempdir().expect("tempdir");

    // Start router harness
    let _app = router_harness::test_app().await;
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{}", port);
    let server = axum_server::bind(format!("127.0.0.1:{}", port).parse::<std::net::SocketAddr>().unwrap()).serve(_app.app.clone().into_make_service());
    tokio::spawn(server);

    // Add with valid path
    let output = Command::new(env!("CARGO_BIN_EXE_xavier"))
        .arg("add")
        .arg("--path")
        .arg("swal/demo/notes")
        .arg("unique text")
        .env("HOME", tmp.path())
        .env("XAVIER_CONFIG_PATH", tmp.path().join("config.toml"))
        .env("XAVIER_RUNTIME_STATE_PATH", tmp.path().join("state"))
        .env("XAVIER_DATA_DIR", tmp.path().join("data"))
        .env("XAVIER_URL", &url)
        .env("XAVIER_TOKEN", router_harness::jwt(UserRole::Admin))
        .output()
        .expect("Failed to execute xavier add");

    assert!(
        output.status.success(),
        "Failed to add memory: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Search and verify path
    let search_output = Command::new(env!("CARGO_BIN_EXE_xavier"))
        .arg("search")
        .arg("unique text")
        .env("HOME", tmp.path())
        .env("XAVIER_CONFIG_PATH", tmp.path().join("config.toml"))
        .env("XAVIER_RUNTIME_STATE_PATH", tmp.path().join("state"))
        .env("XAVIER_DATA_DIR", tmp.path().join("data"))
        .env("XAVIER_URL", &url)
        .env("XAVIER_TOKEN", router_harness::jwt(UserRole::Admin))
        .output()
        .expect("Failed to execute xavier search");

    assert!(
        search_output.status.success(),
        "Search failed: {:?}",
        String::from_utf8_lossy(&search_output.stderr)
    );

    let stdout = String::from_utf8_lossy(&search_output.stdout);
    assert!(
        stdout.contains("\"path\": \"swal/demo/notes\""),
        "Path not found in output: {}",
        stdout
    );
}

#[tokio::test]
#[serial_test::serial]
async fn test_add_path_invalid() {
    let tmp = tempfile::tempdir().expect("tempdir");

    // Start router harness
    let _app = router_harness::test_app().await;
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{}", port);
    let server = axum_server::bind(format!("127.0.0.1:{}", port).parse::<std::net::SocketAddr>().unwrap()).serve(_app.app.clone().into_make_service());
    tokio::spawn(server);

    let invalid_paths = vec!["../x", "/home"];

    for path in invalid_paths {
        let output = Command::new(env!("CARGO_BIN_EXE_xavier"))
            .arg("add")
            .arg("--path")
            .arg(path)
            .arg("fail")
            .env("HOME", tmp.path())
            .env("XAVIER_CONFIG_PATH", tmp.path().join("config.toml"))
            .env("XAVIER_RUNTIME_STATE_PATH", tmp.path().join("state"))
            .env("XAVIER_DATA_DIR", tmp.path().join("data"))
            .env("XAVIER_URL", &url)
            .env("XAVIER_TOKEN", router_harness::jwt(UserRole::Admin))
            .output()
            .expect("Failed to execute xavier add");

        assert!(
            !output.status.success(),
            "Command succeeded unexpectedly for path: {}",
            path
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("Invalid argument 'path'"),
            "Error did not contain expected message. stderr: {}",
            stderr
        );
    }
}
