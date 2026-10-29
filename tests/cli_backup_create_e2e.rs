//! feat-cli-backup-local: E2E tests for `xavier backup create`

xavier::isolate_test_process!();

use serde_json::Value;
use std::fs;
use tempfile::TempDir;

#[test]
#[serial_test::serial]
fn test_backup_create_success() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().join("data");
    fs::create_dir_all(&data_dir).unwrap();

    let db_path = data_dir.join("test.db");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute(
        "CREATE TABLE test_table (id INTEGER PRIMARY KEY, name TEXT)",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO test_table (name) VALUES ('test_row')", [])
        .unwrap();

    let txt_path = data_dir.join("hello.txt");
    fs::write(&txt_path, "world").unwrap();

    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", tmp.path())
        .env("XAVIER_CONFIG_PATH", tmp.path().join("config"))
        .env("XAVIER_DATA_DIR", &data_dir)
        .arg("backup")
        .arg("create")
        .arg("--json");

    let output = cmd.output().unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);

    let parsed: Value = serde_json::from_str(&stdout).expect("Failed to parse JSON output");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["files"], 2);

    let out_dir = std::path::PathBuf::from(parsed["path"].as_str().unwrap());
    assert!(out_dir.exists());

    let manifest_path = out_dir.join("manifest.json");
    assert!(manifest_path.exists());
    let manifest_content = fs::read_to_string(&manifest_path).unwrap();
    let manifest: Value = serde_json::from_str(&manifest_content).unwrap();

    let files = manifest["files"].as_array().unwrap();
    assert_eq!(files.len(), 2);

    let db_backup_path = out_dir.join("test.db");
    assert!(db_backup_path.exists());

    let txt_backup_path = out_dir.join("hello.txt");
    assert!(txt_backup_path.exists());
    assert_eq!(fs::read_to_string(&txt_backup_path).unwrap(), "world");

    let backup_conn = rusqlite::Connection::open(&db_backup_path).unwrap();
    let count: i64 = backup_conn
        .query_row("SELECT COUNT(*) FROM test_table", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
#[serial_test::serial]
fn test_backup_create_unwritable_path() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().join("data");
    fs::create_dir_all(&data_dir).unwrap();

    // Create an unwritable path by pointing it to a file that already exists,
    // or by setting permissions if on unix, but to be cross-platform,
    // we can just make the out dir point to an existing file.
    let bad_out = tmp.path().join("bad_out");
    fs::write(&bad_out, "not a dir").unwrap();

    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", tmp.path())
        .env("XAVIER_CONFIG_PATH", tmp.path().join("config"))
        .env("XAVIER_DATA_DIR", &data_dir)
        .arg("backup")
        .arg("create")
        .arg("--out")
        .arg(&bad_out)
        .arg("--json");

    let output = cmd.output().unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);

    let parsed: Value = serde_json::from_str(&stdout).expect("Failed to parse JSON output");
    assert_eq!(parsed["ok"], false);
    assert!(parsed["error"].is_string());
}
