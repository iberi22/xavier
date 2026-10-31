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

    let url = "http://127.0.0.1:0";

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
        .env("XAVIER_URL", url)
        .env("XAVIER_TOKEN", router_harness::jwt(UserRole::Admin))
        .output()
        .expect("Failed to execute xavier add");

    assert!(
        output.status.success(),
        "Failed to add memory: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Search offline to verify that the path was properly persisted
    let search_output = Command::new(env!("CARGO_BIN_EXE_xavier"))
        .arg("search")
        .arg("unique text")
        .arg("--offline-ok")
        .env("HOME", tmp.path())
        .env("XAVIER_CONFIG_PATH", tmp.path().join("config.toml"))
        .env("XAVIER_RUNTIME_STATE_PATH", tmp.path().join("state"))
        .env("XAVIER_DATA_DIR", tmp.path().join("data"))
        .env("XAVIER_URL", url)
        .env("XAVIER_TOKEN", router_harness::jwt(UserRole::Admin))
        .output()
        .expect("Failed to execute xavier search");

    assert!(
        search_output.status.success(),
        "Failed to search memory: {:?}",
        String::from_utf8_lossy(&search_output.stderr)
    );

    let search_stdout = String::from_utf8_lossy(&search_output.stdout);
    assert!(
        search_stdout.contains("swal/demo/notes"),
        "Search output did not contain path 'swal/demo/notes':\n{}",
        search_stdout
    );
    assert!(
        search_stdout.contains("unique text"),
        "Search output did not contain 'unique text':\n{}",
        search_stdout
    );
}

#[tokio::test]
#[serial_test::serial]
async fn test_add_path_invalid() {
    let tmp = tempfile::tempdir().expect("tempdir");

    let url = "http://127.0.0.1:0";

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
            .env("XAVIER_URL", url)
            .env("XAVIER_TOKEN", router_harness::jwt(UserRole::Admin))
            .output()
            .expect("Failed to execute xavier add");

        assert!(
            !output.status.success(),
            "Command succeeded unexpectedly for path: {}",
            path
        );
    }
}
