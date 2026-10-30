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
        .env("HOME", temp_home.path())
        .env("XAVIER_DATA_DIR", temp_data.path())
        .env("XAVIER_CONFIG_PATH", &temp_config)
        .env("XAVIER_URL", "http://127.0.0.1:0")
        .env("XAVIER_TEST", "1") // Ensure it doesn't fail just for being in temp dir
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("no test pollution found"));
    assert!(stdout.contains("Test Pollution"));
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
    let non_existent_path = temp_home.path().join("non_existent_data");

    let overlay_content = serde_json::json!({
        "data_dir": non_existent_path.to_str().unwrap()
    });
    fs::write(&overlay_path, overlay_content.to_string()).unwrap();

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
    assert!(stdout.contains("points to a non-existent path"));
    assert!(stdout.contains(non_existent_path.to_str().unwrap()));

    // Check exit code is not a crash (0 because it's just a WARN)
    assert_eq!(output.status.code(), Some(0));

    // Files are unchanged
    assert!(overlay_path.exists());
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
    assert!(stdout.contains("Found SHOULD-NOT-EXIST file/dir"));
    assert!(stdout.contains("SHOULD-NOT-EXIST.db"));
    assert_eq!(output.status.code(), Some(0));

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
    fs::write(&overlay_path, "{ invalid json").unwrap();

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
    assert!(stdout.contains("Failed to parse runtime state JSON"));
    assert_eq!(output.status.code(), Some(0));
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
    assert!(stdout.contains("XAVIER_DATA_DIR is in temp dir but not in test mode"));
    assert_eq!(output.status.code(), Some(0));
}
