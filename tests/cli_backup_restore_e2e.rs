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

    // 1. Create state: store memories and code-graph index
    // Memories (store.sqlite3)
    let db_path = data_dir.join("store.sqlite3");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute(
        "CREATE TABLE memories (id INTEGER PRIMARY KEY, content TEXT)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO memories (content) VALUES ('searchable_memory_test')",
        [],
    )
    .unwrap();
    drop(conn);

    // Code-graph index (code_graph.db)
    let cg_db_path = data_dir.join("code_graph.db");
    let cg_conn = rusqlite::Connection::open(&cg_db_path).unwrap();
    cg_conn
        .execute(
            "CREATE TABLE symbols (id INTEGER PRIMARY KEY, symbol TEXT)",
            [],
        )
        .unwrap();
    cg_conn
        .execute(
            "INSERT INTO symbols (symbol) VALUES ('test_graph_symbol')",
            [],
        )
        .unwrap();
    drop(cg_conn);

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

    // 5. Verify restored state (assert memories searchable and graph symbols present)
    assert!(data_dir.exists());

    let restored_db_path = data_dir.join("store.sqlite3");
    assert!(restored_db_path.exists());
    let restored_conn = rusqlite::Connection::open(&restored_db_path).unwrap();
    let memory_content: String = restored_conn
        .query_row("SELECT content FROM memories WHERE id = 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(memory_content, "searchable_memory_test");

    let restored_cg_path = data_dir.join("code_graph.db");
    assert!(restored_cg_path.exists());
    let restored_cg_conn = rusqlite::Connection::open(&restored_cg_path).unwrap();
    let symbol: String = restored_cg_conn
        .query_row("SELECT symbol FROM symbols WHERE id = 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(symbol, "test_graph_symbol");
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
