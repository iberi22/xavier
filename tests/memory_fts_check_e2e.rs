//! <ledger id>: feat-memory-write-integrity

use tempfile::tempdir;

xavier::isolate_test_process!();

#[test]
fn fts_check_ok() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("memory-store.sqlite3");

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute(
        "CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(id UNINDEXED, path, content, code_tokens);",
        []
    ).unwrap();
    conn.execute(
        "INSERT INTO memory_fts(id, path, content) VALUES (1, 'path1', 'content1'), (2, 'path2', 'content2'), (3, 'path3', 'content3')",
        []
    ).unwrap();
    conn.close().unwrap();

    let exe = env!("CARGO_BIN_EXE_xavier");
    let output = std::process::Command::new(exe)
        .env("HOME", tmp.path())
        .env("XAVIER_DATA_DIR", tmp.path())
        .env("XAVIER_CONFIG_PATH", tmp.path().join("config.toml"))
        .env("XAVIER_RUNTIME_STATE_PATH", tmp.path().join("runtime.json"))
        .arg("memory")
        .arg("fts-check")
        .arg("--db")
        .arg(&db_path)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn fts_check_corrupt() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("memory-store.sqlite3");

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute(
        "CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(id UNINDEXED, path, content, code_tokens);",
        []
    ).unwrap();
    conn.execute(
        "INSERT INTO memory_fts(id, path, content) VALUES (1, 'path1', 'content1'), (2, 'path2', 'content2'), (3, 'path3', 'content3')",
        []
    ).unwrap();

    // Corrupt index
    assert_eq!(
        conn.execute("DELETE FROM memory_fts_data WHERE id > 1", [])
            .unwrap(),
        2
    );
    conn.close().unwrap();

    let exe = env!("CARGO_BIN_EXE_xavier");
    let output = std::process::Command::new(exe)
        .env("HOME", tmp.path())
        .env("XAVIER_DATA_DIR", tmp.path())
        .env("XAVIER_CONFIG_PATH", tmp.path().join("config.toml"))
        .env("XAVIER_RUNTIME_STATE_PATH", tmp.path().join("runtime.json"))
        .arg("memory")
        .arg("fts-check")
        .arg("--db")
        .arg(&db_path)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn fts_check_missing_db() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("missing.sqlite3");

    let exe = env!("CARGO_BIN_EXE_xavier");
    let output = std::process::Command::new(exe)
        .env("HOME", tmp.path())
        .env("XAVIER_DATA_DIR", tmp.path())
        .env("XAVIER_CONFIG_PATH", tmp.path().join("config.toml"))
        .env("XAVIER_RUNTIME_STATE_PATH", tmp.path().join("runtime.json"))
        .arg("memory")
        .arg("fts-check")
        .arg("--db")
        .arg(&db_path)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn fts_check_operational_error() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("memory-store.sqlite3");

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    // Create a non-FTS table, this will cause `SELECT count(*) FROM memory_fts WHERE memory_fts MATCH 'test'` to fail with operational error
    conn.execute("CREATE TABLE memory_fts (id INTEGER);", [])
        .unwrap();
    conn.close().unwrap();

    let exe = env!("CARGO_BIN_EXE_xavier");
    let output = std::process::Command::new(exe)
        .env("HOME", tmp.path())
        .env("XAVIER_DATA_DIR", tmp.path())
        .env("XAVIER_CONFIG_PATH", tmp.path().join("config.toml"))
        .env("XAVIER_RUNTIME_STATE_PATH", tmp.path().join("runtime.json"))
        .arg("memory")
        .arg("fts-check")
        .arg("--db")
        .arg(&db_path)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn fts_check_json_escaping() {
    let tmp = tempdir().unwrap();
    // Test that the emitted JSON properly escapes quotes and backslashes in paths
    let db_path = tmp.path().join("test\"path\\db.sqlite3");

    let exe = env!("CARGO_BIN_EXE_xavier");
    let output = std::process::Command::new(exe)
        .env("HOME", tmp.path())
        .env("XAVIER_DATA_DIR", tmp.path())
        .env("XAVIER_CONFIG_PATH", tmp.path().join("config.toml"))
        .env("XAVIER_RUNTIME_STATE_PATH", tmp.path().join("runtime.json"))
        .arg("memory")
        .arg("fts-check")
        .arg("--json")
        .arg("--db")
        .arg(&db_path)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("output should be valid JSON");

    assert_eq!(parsed["status"], "error");
    assert_eq!(parsed["db"].as_str().unwrap(), db_path.to_str().unwrap());
}

#[test]
fn fts_check_not_a_database_is_corruption() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("memory-store.sqlite3");
    // Not an SQLite file at all: SQLITE_NOTADB must be reported as corruption
    // (exit 1), not as an operational error (exit 2).
    std::fs::write(&db_path, vec![0xA5u8; 8192]).unwrap();

    let exe = env!("CARGO_BIN_EXE_xavier");
    let output = std::process::Command::new(exe)
        .env("HOME", tmp.path())
        .env("XAVIER_DATA_DIR", tmp.path())
        .env("XAVIER_CONFIG_PATH", tmp.path().join("config.toml"))
        .env("XAVIER_RUNTIME_STATE_PATH", tmp.path().join("runtime.json"))
        .arg("memory")
        .arg("fts-check")
        .arg("--json")
        .arg("--db")
        .arg(&db_path)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["status"], "corrupt");
}
