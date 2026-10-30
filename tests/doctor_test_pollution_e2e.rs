use std::fs;
use std::process::Command;
use tempfile::tempdir;

#[serial_test::serial]
#[test]
fn test_doctor_pollution_clean() {
    xavier::isolate_test_process!();

    let temp_home = tempdir().unwrap();
    let temp_data = tempdir().unwrap();
    let temp_config = temp_home.path().join("config.toml");
    fs::write(&temp_config, "{}").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_xavier"))
        .arg("doctor")
        .arg("--json")
        .arg("--verbose")
        .env("HOME", temp_home.path())
        .env("XAVIER_DATA_DIR", temp_data.path())
        .env("XAVIER_CONFIG_PATH", &temp_config)
        .env("XAVIER_URL", "http://127.0.0.1:0")
        .env("XAVIER_TEST", "1") // Ensure it doesn't fail just for being in temp dir
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);

    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("Failed to parse JSON stdout");
    let checks = json
        .get("checks")
        .and_then(|v| v.as_array())
        .expect("Expected checks array");
    let pollution_check = checks.iter().find(|c| {
        c.get("name").and_then(|n| n.as_str()) == Some("Test Pollution")
            && c.get("detail")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .contains("no test pollution found")
    });

    assert!(pollution_check.is_some(), "Test Pollution check not found");
    let pollution_check = pollution_check.unwrap();
    assert_eq!(
        pollution_check.get("status").and_then(|s| s.as_str()),
        Some("Ok")
    );
}

#[serial_test::serial]
#[test]
fn test_doctor_pollution_runtime_overlay() {
    xavier::isolate_test_process!();

    let temp_home = tempdir().unwrap();
    let temp_data = tempdir().unwrap();
    let temp_config = temp_home.path().join("config.toml");
    fs::write(&temp_config, "{}").unwrap();

    let overlay_path = temp_home.path().join("runtime.json");
    let non_existent_path = std::path::PathBuf::from("/opt/xavier_non_existent_data_for_test");

    let overlay_content = serde_json::json!({
        "memory": {
            "data_dir": non_existent_path.to_str().unwrap()
        }
    });
    let original_bytes = overlay_content.to_string().into_bytes();
    fs::write(&overlay_path, &original_bytes).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_xavier"))
        .arg("doctor")
        .arg("--json")
        .env("HOME", temp_home.path())
        .env("XAVIER_DATA_DIR", temp_data.path())
        .env("XAVIER_CONFIG_PATH", &temp_config)
        .env("XAVIER_RUNTIME_STATE_PATH", &overlay_path)
        .env("XAVIER_URL", "http://127.0.0.1:0")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);

    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("Failed to parse JSON stdout");
    let checks = json
        .get("checks")
        .and_then(|v| v.as_array())
        .expect("Expected checks array");
    let pollution_check = checks.iter().find(|c| {
        c.get("name").and_then(|n| n.as_str()) == Some("Test Pollution")
            && c.get("detail")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .contains("points to a non-existent path")
    });

    assert!(pollution_check.is_some(), "Test Pollution check not found");
    let pollution_check = pollution_check.unwrap();
    assert_eq!(
        pollution_check.get("status").and_then(|s| s.as_str()),
        Some("Warn")
    );
    let detail = pollution_check
        .get("detail")
        .and_then(|d| d.as_str())
        .unwrap();
    assert!(detail.contains(non_existent_path.to_str().unwrap()));

    // Check exit code is not a crash (0 because it's just a WARN)
    assert_eq!(output.status.code(), Some(1));

    // Files are unchanged
    let current_bytes = fs::read(&overlay_path).unwrap();
    assert_eq!(original_bytes, current_bytes);
}

#[serial_test::serial]
#[test]
fn test_doctor_pollution_file_should_not_exist() {
    xavier::isolate_test_process!();

    let temp_home = tempdir().unwrap();
    let temp_data = tempdir().unwrap();
    let temp_config = temp_home.path().join("config.toml");
    fs::write(&temp_config, "{}").unwrap();

    let should_not_exist_file = temp_data.path().join("SHOULD-NOT-EXIST.db");
    fs::write(&should_not_exist_file, "dummy").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_xavier"))
        .arg("doctor")
        .arg("--json")
        .env("HOME", temp_home.path())
        .env("XAVIER_DATA_DIR", temp_data.path())
        .env("XAVIER_CONFIG_PATH", &temp_config)
        .env("XAVIER_URL", "http://127.0.0.1:0")
        .env("XAVIER_TEST", "1")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);

    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("Failed to parse JSON stdout");
    let checks = json
        .get("checks")
        .and_then(|v| v.as_array())
        .expect("Expected checks array");
    let pollution_check = checks.iter().find(|c| {
        c.get("name").and_then(|n| n.as_str()) == Some("Test Pollution")
            && c.get("detail")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .contains("Found SHOULD-NOT-EXIST file/dir")
    });

    assert!(pollution_check.is_some(), "Test Pollution check not found");
    let pollution_check = pollution_check.unwrap();
    assert_eq!(
        pollution_check.get("status").and_then(|s| s.as_str()),
        Some("Warn")
    );
    let detail = pollution_check
        .get("detail")
        .and_then(|d| d.as_str())
        .unwrap();
    assert!(detail.contains("SHOULD-NOT-EXIST.db"));

    assert_eq!(output.status.code(), Some(1));

    // File unchanged
    assert!(should_not_exist_file.exists());
}

#[serial_test::serial]
#[test]
fn test_doctor_pollution_invalid_json() {
    xavier::isolate_test_process!();

    let temp_home = tempdir().unwrap();
    let temp_data = tempdir().unwrap();
    let temp_config = temp_home.path().join("config.toml");
    fs::write(&temp_config, "{}").unwrap();

    let overlay_path = temp_home.path().join("runtime.json");
    let original_bytes = b"{ invalid json";
    fs::write(&overlay_path, original_bytes).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_xavier"))
        .arg("doctor")
        .arg("--json")
        .env("HOME", temp_home.path())
        .env("XAVIER_DATA_DIR", temp_data.path())
        .env("XAVIER_CONFIG_PATH", &temp_config)
        .env("XAVIER_RUNTIME_STATE_PATH", &overlay_path)
        .env("XAVIER_URL", "http://127.0.0.1:0")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);

    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("Failed to parse JSON stdout");
    let checks = json
        .get("checks")
        .and_then(|v| v.as_array())
        .expect("Expected checks array");
    let pollution_check = checks.iter().find(|c| {
        c.get("name").and_then(|n| n.as_str()) == Some("Test Pollution")
            && c.get("detail")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .contains("Failed to parse runtime state JSON")
    });

    assert!(pollution_check.is_some(), "Test Pollution check not found");
    let pollution_check = pollution_check.unwrap();
    assert_eq!(
        pollution_check.get("status").and_then(|s| s.as_str()),
        Some("Warn")
    );

    assert_eq!(output.status.code(), Some(1));

    let current_bytes = fs::read(&overlay_path).unwrap();
    assert_eq!(original_bytes, current_bytes.as_slice());
}

#[serial_test::serial]
#[test]
fn test_doctor_pollution_temp_dir_without_test_mode() {
    xavier::isolate_test_process!();

    let temp_home = tempdir().unwrap();
    let temp_data = tempdir().unwrap();
    let temp_config = temp_home.path().join("config.toml");
    fs::write(&temp_config, "{}").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_xavier"))
        .arg("doctor")
        .arg("--json")
        .env("HOME", temp_home.path())
        .env("XAVIER_DATA_DIR", temp_data.path())
        .env("XAVIER_CONFIG_PATH", &temp_config)
        .env("XAVIER_URL", "http://127.0.0.1:0")
        // Intentionally missing XAVIER_TEST and XAVIER_TEST_MODE
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);

    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("Failed to parse JSON stdout");
    let checks = json
        .get("checks")
        .and_then(|v| v.as_array())
        .expect("Expected checks array");
    let pollution_check = checks.iter().find(|c| {
        c.get("name").and_then(|n| n.as_str()) == Some("Test Pollution")
            && c.get("detail")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .contains("XAVIER_DATA_DIR is in temp dir but not in test mode")
    });

    assert!(pollution_check.is_some(), "Test Pollution check not found");
    let pollution_check = pollution_check.unwrap();
    assert_eq!(
        pollution_check.get("status").and_then(|s| s.as_str()),
        Some("Warn")
    );

    assert_eq!(output.status.code(), Some(1));
}
