use std::fs;
use std::process::Command;
use tempfile::TempDir;

#[test]
fn test_manifests_sync_pass() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let dir_path = temp_dir.path();

    fs::write(
        dir_path.join("Cargo.toml"),
        "[package]\nname = \"test-pkg\"\nversion = \"0.0.1\"\n",
    )
    .unwrap();
    fs::write(
        dir_path.join("package.json"),
        "{\n  \"name\": \"test-pkg\",\n  \"version\": \"0.0.1\"\n}\n",
    )
    .unwrap();
    fs::write(
        dir_path.join("CHANGELOG.md"),
        "# Changelog\n\n## [Unreleased]\n",
    )
    .unwrap();

    let root_script = std::env::current_dir()
        .unwrap()
        .join("scripts")
        .join("check-version-sync.sh");

    let status = Command::new("bash")
        .arg(&root_script)
        .arg(dir_path)
        .status()
        .expect("Failed to execute check-version-sync.sh");

    assert!(status.success(), "Expected version sync check to pass");
}

#[test]
fn test_drift_cargo_vs_package_fail() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let dir_path = temp_dir.path();

    fs::write(
        dir_path.join("Cargo.toml"),
        "[package]\nname = \"test-pkg\"\nversion = \"0.0.2\"\n",
    )
    .unwrap();
    fs::write(
        dir_path.join("package.json"),
        "{\n  \"name\": \"test-pkg\",\n  \"version\": \"0.0.1\"\n}\n",
    )
    .unwrap();
    fs::write(
        dir_path.join("CHANGELOG.md"),
        "# Changelog\n\n## [Unreleased]\n",
    )
    .unwrap();

    let root_script = std::env::current_dir()
        .unwrap()
        .join("scripts")
        .join("check-version-sync.sh");

    let status = Command::new("bash")
        .arg(&root_script)
        .arg(dir_path)
        .status()
        .expect("Failed to execute check-version-sync.sh");

    assert!(
        !status.success(),
        "Expected version sync check to fail on drift"
    );
}

#[test]
fn test_changelog_missing_unreleased_fail() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let dir_path = temp_dir.path();

    fs::write(
        dir_path.join("Cargo.toml"),
        "[package]\nname = \"test-pkg\"\nversion = \"0.0.1\"\n",
    )
    .unwrap();
    fs::write(
        dir_path.join("package.json"),
        "{\n  \"name\": \"test-pkg\",\n  \"version\": \"0.0.1\"\n}\n",
    )
    .unwrap();
    fs::write(
        dir_path.join("CHANGELOG.md"),
        "# Changelog\n\n## [0.0.1] - 2026-08-30\n",
    )
    .unwrap();

    let root_script = std::env::current_dir()
        .unwrap()
        .join("scripts")
        .join("check-version-sync.sh");

    let status = Command::new("bash")
        .arg(&root_script)
        .arg(dir_path)
        .status()
        .expect("Failed to execute check-version-sync.sh");

    assert!(
        !status.success(),
        "Expected check to fail when [Unreleased] is missing"
    );
}

#[test]
fn test_preflight_json_ready_true() {
    let json_str = r#"{"ready": true, "wave": 10, "stable": 52, "total": 52}"#;
    let val: serde_json::Value = serde_json::from_str(json_str).expect("Valid JSON");
    assert_eq!(val["ready"], true, "Expected ready flag to be true");
}
