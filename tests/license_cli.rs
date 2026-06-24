//! CLI Integration Tests for License Management
//!
//! Tests the xavier binary CLI license commands.

use std::process::Command;
use tempfile::tempdir;

fn xavier_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_xavier"))
}

#[test]
fn test_license_cli_default_status() {
    let temp_dir = tempdir().unwrap();
    let config_path = temp_dir.path().join("xavier.config.json");

    let output = xavier_binary()
        .args(&["license", "status"])
        .env("XAVIER_CONFIG_PATH", config_path.to_str().unwrap())
        .output()
        .expect("failed to execute xavier binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Xavier License Status"));
    assert!(stdout.contains("Core License:  AGPL-3.0"));
    assert!(stdout.contains("Mesh License:  ❌ Not Accepted"));
}

#[test]
fn test_license_cli_accept() {
    let temp_dir = tempdir().unwrap();
    let config_path = temp_dir.path().join("xavier.config.json");

    // Accept license
    let output = xavier_binary()
        .args(&["license", "accept"])
        .env("XAVIER_CONFIG_PATH", config_path.to_str().unwrap())
        .output()
        .expect("failed to execute xavier binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Mesh License accepted and saved!"));

    // Check status
    let output = xavier_binary()
        .args(&["license", "status"])
        .env("XAVIER_CONFIG_PATH", config_path.to_str().unwrap())
        .output()
        .expect("failed to execute xavier binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Mesh License:  ✅ Accepted"));
}

#[test]
fn test_license_cli_duplicate_accept() {
    let temp_dir = tempdir().unwrap();
    let config_path = temp_dir.path().join("xavier.config.json");

    // Accept once
    xavier_binary()
        .args(&["license", "accept"])
        .env("XAVIER_CONFIG_PATH", config_path.to_str().unwrap())
        .output()
        .unwrap();

    // Accept again
    let output = xavier_binary()
        .args(&["license", "accept"])
        .env("XAVIER_CONFIG_PATH", config_path.to_str().unwrap())
        .output()
        .expect("failed to execute xavier binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Mesh License already accepted"));
}

#[test]
fn test_license_cli_commercial_accept() {
    let temp_dir = tempdir().unwrap();
    let config_path = temp_dir.path().join("xavier.config.json");

    // Accept commercial
    let output = xavier_binary()
        .args(&["license", "accept", "--commercial", "swal-test-key"])
        .env("XAVIER_CONFIG_PATH", config_path.to_str().unwrap())
        .output()
        .expect("failed to execute xavier binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Commercial License accepted and saved!"));

    // Check status
    let output = xavier_binary()
        .args(&["license", "status"])
        .env("XAVIER_CONFIG_PATH", config_path.to_str().unwrap())
        .output()
        .expect("failed to execute xavier binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Core License:  Xavier Commercial License"));
    assert!(stdout.contains("Mesh License:  ✅ Accepted"));
    assert!(stdout.contains("Enterprise Features: ✅ Unlocked"));
}

#[test]
fn test_license_cli_show() {
    let output = xavier_binary()
        .args(&["license", "show"])
        .output()
        .expect("failed to execute xavier binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Xavier Licensing Summary"));
    assert!(stdout.contains("AGPL-3.0 License"));
    assert!(stdout.contains("Xavier Mesh License v1.0"));
}
