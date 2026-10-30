//! feat-cli-backup-local: E2E tests for `xavier backup create`

xavier::isolate_test_process!();

use serde_json::Value;
use sha2::Digest;
use std::fs;
use tempfile::TempDir;

#[test]
#[serial_test::serial]
fn test_backup_create_success() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().join("data");
    fs::create_dir_all(&data_dir).unwrap();
    let runtime_state_path = tmp.path().join("runtime.json");
    fs::write(&runtime_state_path, "{}").unwrap();

    let db_path = data_dir.join("test.db");
    // Enable WAL mode
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();

    conn.execute(
        "CREATE TABLE test_table (id INTEGER PRIMARY KEY, name TEXT)",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO test_table (name) VALUES ('test_row')", [])
        .unwrap();

    let txt_path = data_dir.join("hello.txt");
    fs::write(&txt_path, "world").unwrap();

    // Nested path test
    let nested_dir = data_dir.join("sub").join("dir");
    fs::create_dir_all(&nested_dir).unwrap();
    let nested_txt_path = nested_dir.join("file.txt");
    fs::write(&nested_txt_path, "nested").unwrap();

    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", tmp.path())
        .env("XAVIER_CONFIG_PATH", tmp.path().join("config"))
        .env("XAVIER_RUNTIME_STATE_PATH", &runtime_state_path)
        .env("XAVIER_DATA_DIR", &data_dir)
        .arg("backup")
        .arg("create")
        .arg("--json");

    let output = cmd.output().unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);

    let parsed: Value = serde_json::from_str(&stdout).expect("Failed to parse JSON output");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["files"], 3);

    let out_dir = std::path::PathBuf::from(parsed["path"].as_str().unwrap());
    assert!(out_dir.exists());

    let manifest_path = out_dir.join("manifest.json");
    assert!(manifest_path.exists());
    let manifest_content = fs::read_to_string(&manifest_path).unwrap();
    let manifest: Value = serde_json::from_str(&manifest_content).unwrap();

    let files = manifest["files"].as_array().unwrap();
    assert_eq!(files.len(), 3);

    for f in files {
        let f_path = f["path"].as_str().unwrap();
        let f_size = f["size"].as_u64().unwrap();
        let f_sha256 = f["sha256"].as_str().unwrap();

        let backed_up_file = out_dir.join(f_path);
        assert!(backed_up_file.exists());

        let metadata = fs::metadata(&backed_up_file).unwrap();
        assert_eq!(metadata.len(), f_size);

        let content = fs::read(&backed_up_file).unwrap();
        let mut hasher = sha2::Sha256::new();
        sha2::Digest::update(&mut hasher, &content);
        let hash = sha2::Digest::finalize(hasher)
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();
        assert_eq!(hash, f_sha256);
    }

    let db_backup_path = out_dir.join("test.db");
    assert!(db_backup_path.exists());

    let txt_backup_path = out_dir.join("hello.txt");
    assert!(txt_backup_path.exists());
    assert_eq!(fs::read_to_string(&txt_backup_path).unwrap(), "world");

    let nested_backup_path = out_dir.join("sub").join("dir").join("file.txt");
    assert!(nested_backup_path.exists());
    assert_eq!(fs::read_to_string(&nested_backup_path).unwrap(), "nested");

    let backup_conn = rusqlite::Connection::open_with_flags(
        &db_backup_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let count: i64 = backup_conn
        .query_row("SELECT COUNT(*) FROM test_table", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
    let row_value: String = backup_conn
        .query_row("SELECT name FROM test_table", [], |row| row.get(0))
        .unwrap();
    assert_eq!(row_value, "test_row");

    // Ensure the original connection was open until now
    let count_orig: i64 = conn
        .query_row("SELECT COUNT(*) FROM test_table", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count_orig, 1);
}

#[test]
#[serial_test::serial]
fn test_backup_create_unwritable_path() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().join("data");
    fs::create_dir_all(&data_dir).unwrap();

    let runtime_state_path = tmp.path().join("runtime.json");
    fs::write(&runtime_state_path, "{}").unwrap();

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
        .env("XAVIER_RUNTIME_STATE_PATH", &runtime_state_path)
        .env("XAVIER_DATA_DIR", &data_dir)
        .arg("backup")
        .arg("create")
        .arg("--out")
        .arg(&bad_out)
        .arg("--json");

    let output = cmd.output().unwrap();
    assert!(!output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);

    let parsed: Value = serde_json::from_str(&stdout).expect("Failed to parse JSON output");
    assert_eq!(parsed["ok"], false);
    assert!(parsed["error"].is_string());
}

#[test]
#[serial_test::serial]
fn test_backup_create_out_rejection() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().join("data");
    fs::create_dir_all(&data_dir).unwrap();
    let runtime_state_path = tmp.path().join("runtime.json");
    fs::write(&runtime_state_path, "{}").unwrap();

    // Case 1: out dir equals data dir
    let out_eq = data_dir.clone();
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", tmp.path())
        .env("XAVIER_CONFIG_PATH", tmp.path().join("config"))
        .env("XAVIER_RUNTIME_STATE_PATH", &runtime_state_path)
        .env("XAVIER_DATA_DIR", &data_dir)
        .arg("backup")
        .arg("create")
        .arg("--out")
        .arg(&out_eq)
        .arg("--json");

    let output = cmd.output().unwrap();
    assert!(!output.status.success());
    let parsed: Value = serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).unwrap();
    assert_eq!(parsed["ok"], false);

    // Case 2: out dir is a descendant
    let out_desc = data_dir.join("sub").join("backups");
    let mut cmd2 = std::process::Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd2.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", tmp.path())
        .env("XAVIER_CONFIG_PATH", tmp.path().join("config"))
        .env("XAVIER_RUNTIME_STATE_PATH", &runtime_state_path)
        .env("XAVIER_DATA_DIR", &data_dir)
        .arg("backup")
        .arg("create")
        .arg("--out")
        .arg(&out_desc)
        .arg("--json");

    let output2 = cmd2.output().unwrap();
    assert!(!output2.status.success());
    let parsed2: Value = serde_json::from_str(&String::from_utf8_lossy(&output2.stdout)).unwrap();
    assert_eq!(parsed2["ok"], false);

    // Case 3: unix symlink resolves into data dir
    #[cfg(unix)]
    {
        let link_path = tmp.path().join("link_to_data");
        std::os::unix::fs::symlink(&data_dir, &link_path).unwrap();

        let mut cmd3 = std::process::Command::new(env!("CARGO_BIN_EXE_xavier"));
        cmd3.env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", tmp.path())
            .env("XAVIER_CONFIG_PATH", tmp.path().join("config"))
            .env("XAVIER_RUNTIME_STATE_PATH", &runtime_state_path)
            .env("XAVIER_DATA_DIR", &data_dir)
            .arg("backup")
            .arg("create")
            .arg("--out")
            .arg(&link_path)
            .arg("--json");

        let output3 = cmd3.output().unwrap();
        assert!(!output3.status.success());
        let parsed3: Value =
            serde_json::from_str(&String::from_utf8_lossy(&output3.stdout)).unwrap();
        assert_eq!(parsed3["ok"], false);
    }
}

#[test]
#[serial_test::serial]
fn test_backup_create_refuses_non_empty_dest() {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().join("data");
    fs::create_dir_all(&data_dir).unwrap();
    let runtime_state_path = tmp.path().join("runtime.json");
    fs::write(&runtime_state_path, "{}").unwrap();

    let txt_path = data_dir.join("hello.txt");
    fs::write(&txt_path, "world").unwrap();

    let out_dir = tmp.path().join("backup_out");

    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd.env_clear()
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

    let output = cmd.output().unwrap();
    assert!(output.status.success());
    let parsed: Value = serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).unwrap();
    assert_eq!(parsed["ok"], true);

    // capture bytes of backed up file
    let backed_up_file = out_dir.join("hello.txt");
    let bytes_before = fs::read(&backed_up_file).unwrap();

    // second backup
    let mut cmd2 = std::process::Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd2.env_clear()
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

    let output2 = cmd2.output().unwrap();
    assert!(!output2.status.success());

    let parsed2: Value = serde_json::from_str(&String::from_utf8_lossy(&output2.stdout)).unwrap();
    assert_eq!(parsed2["ok"], false);

    let err_msg = parsed2["error"].as_str().unwrap();
    assert!(err_msg.contains("not empty"));

    let bytes_after = fs::read(&backed_up_file).unwrap();
    assert_eq!(bytes_before, bytes_after);
}
