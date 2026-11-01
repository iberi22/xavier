//! feat-cli-backup-local: E2E tests for `xavier backup restore`

xavier::isolate_test_process!();

use serde_json::Value;
use std::fs;
use tempfile::TempDir;

#[test]
#[serial_test::serial]
fn test_backup_restore_round_trip() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().join("data");
    fs::create_dir_all(&data_dir).unwrap();
    let runtime_state_path = tmp.path().join("runtime.json");
    fs::write(&runtime_state_path, "{}").unwrap();

    let out_dir = tmp.path().join("backup_out");

    // 1. Create state
    let db_path = data_dir.join("test.db");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute(
        "CREATE TABLE test_table (id INTEGER PRIMARY KEY, name TEXT)",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO test_table (name) VALUES ('hello_restore')", [])
        .unwrap();
    drop(conn);

    let txt_path = data_dir.join("hello.txt");
    fs::write(&txt_path, "world").unwrap();

    // 2. Backup create
    let mut cmd_create = std::process::Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd_create
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", tmp.path())
        .env("XAVIER_CONFIG_PATH", tmp.path().join("config"))
        .env("XAVIER_RUNTIME_STATE_PATH", &runtime_state_path)
        .env("XAVIER_DATA_DIR", &data_dir)
        .arg("backup")
        .arg("create")
        .arg("--out")
        .arg(&out_dir)
        .arg("--json");

    let output_create = cmd_create.output().unwrap();
    assert!(output_create.status.success());
    let parsed: Value =
        serde_json::from_str(&String::from_utf8_lossy(&output_create.stdout)).unwrap();
    assert_eq!(parsed["ok"], true);

    // 3. Wipe state
    fs::remove_dir_all(&data_dir).unwrap();
    assert!(!data_dir.exists());

    // 4. Backup restore
    let mut cmd_restore = std::process::Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd_restore
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", tmp.path())
        .env("XAVIER_CONFIG_PATH", tmp.path().join("config"))
        .env("XAVIER_RUNTIME_STATE_PATH", &runtime_state_path)
        .env("XAVIER_DATA_DIR", &data_dir)
        .arg("backup")
        .arg("restore")
        .arg("--from")
        .arg(&out_dir);

    let output_restore = cmd_restore.output().unwrap();
    assert!(output_restore.status.success());

    // 5. Verify restored state
    assert!(data_dir.exists());
    let restored_db_path = data_dir.join("test.db");
    assert!(restored_db_path.exists());

    let restored_conn = rusqlite::Connection::open(&restored_db_path).unwrap();
    let name: String = restored_conn
        .query_row("SELECT name FROM test_table WHERE id = 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(name, "hello_restore");

    let restored_txt_path = data_dir.join("hello.txt");
    assert!(restored_txt_path.exists());
    assert_eq!(fs::read_to_string(&restored_txt_path).unwrap(), "world");
}

#[test]
#[serial_test::serial]
fn test_backup_restore_corrupted_archive_manifest() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().join("data");
    fs::create_dir_all(&data_dir).unwrap();
    let runtime_state_path = tmp.path().join("runtime.json");
    fs::write(&runtime_state_path, "{}").unwrap();

    let out_dir = tmp.path().join("backup_out");

    // 1. Create state
    let txt_path = data_dir.join("hello.txt");
    fs::write(&txt_path, "world").unwrap();

    // 2. Backup create
    let mut cmd_create = std::process::Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd_create
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", tmp.path())
        .env("XAVIER_CONFIG_PATH", tmp.path().join("config"))
        .env("XAVIER_RUNTIME_STATE_PATH", &runtime_state_path)
        .env("XAVIER_DATA_DIR", &data_dir)
        .arg("backup")
        .arg("create")
        .arg("--out")
        .arg(&out_dir)
        .arg("--json");

    let output_create = cmd_create.output().unwrap();
    assert!(output_create.status.success());

    // 3. Corrupt archive (modify manifest)
    let manifest_path = out_dir.join("manifest.json");
    fs::write(&manifest_path, "invalid json").unwrap();

    // 4. Backup restore
    let mut cmd_restore = std::process::Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd_restore
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", tmp.path())
        .env("XAVIER_CONFIG_PATH", tmp.path().join("config"))
        .env("XAVIER_RUNTIME_STATE_PATH", &runtime_state_path)
        .env("XAVIER_DATA_DIR", &data_dir)
        .arg("backup")
        .arg("restore")
        .arg("--from")
        .arg(&out_dir);

    let output_restore = cmd_restore.output().unwrap();
    assert!(!output_restore.status.success());
    let stderr = String::from_utf8_lossy(&output_restore.stderr);
    assert!(stderr.contains("Corrupted backup: invalid manifest.json"));
}

#[test]
#[serial_test::serial]
fn test_backup_restore_corrupted_archive_file_hash() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().join("data");
    fs::create_dir_all(&data_dir).unwrap();
    let runtime_state_path = tmp.path().join("runtime.json");
    fs::write(&runtime_state_path, "{}").unwrap();

    let out_dir = tmp.path().join("backup_out");

    // 1. Create state
    let txt_path = data_dir.join("hello.txt");
    fs::write(&txt_path, "world").unwrap();

    // 2. Backup create
    let mut cmd_create = std::process::Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd_create
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", tmp.path())
        .env("XAVIER_CONFIG_PATH", tmp.path().join("config"))
        .env("XAVIER_RUNTIME_STATE_PATH", &runtime_state_path)
        .env("XAVIER_DATA_DIR", &data_dir)
        .arg("backup")
        .arg("create")
        .arg("--out")
        .arg(&out_dir)
        .arg("--json");

    let output_create = cmd_create.output().unwrap();
    assert!(output_create.status.success());

    // 3. Corrupt archive (modify file)
    let backup_txt_path = out_dir.join("hello.txt");
    fs::write(&backup_txt_path, "corrupted_world").unwrap();

    // 4. Backup restore
    let mut cmd_restore = std::process::Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd_restore
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", tmp.path())
        .env("XAVIER_CONFIG_PATH", tmp.path().join("config"))
        .env("XAVIER_RUNTIME_STATE_PATH", &runtime_state_path)
        .env("XAVIER_DATA_DIR", &data_dir)
        .arg("backup")
        .arg("restore")
        .arg("--from")
        .arg(&out_dir);

    let output_restore = cmd_restore.output().unwrap();
    assert!(!output_restore.status.success());
    let stderr = String::from_utf8_lossy(&output_restore.stderr);
    assert!(stderr.contains("Corrupted backup: hash mismatch for hello.txt"));
}
