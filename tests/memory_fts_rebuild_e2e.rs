//! <ledger id>: feat-memory-write-integrity

use tempfile::tempdir;

xavier::isolate_test_process!();

#[test]
fn fts_rebuild_ok() {
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

    // delete the FTS shadow content (as requested by instructions, since memory_fts is a normal FTS5 table, dropping shadow tables directly simulates corruption, but dropping memory_fts_idx ensures rebuild succeeds without constructor fail on open).
    conn.execute("DELETE FROM memory_fts_idx", []).unwrap();
    conn.close().unwrap();

    let exe = env!("CARGO_BIN_EXE_xavier");

    // Run FTS rebuild
    let output = std::process::Command::new(exe)
        .env("HOME", tmp.path())
        .env("XAVIER_DATA_DIR", tmp.path())
        .env("XAVIER_CONFIG_PATH", tmp.path().join("config.toml"))
        .env("XAVIER_RUNTIME_STATE_PATH", tmp.path().join("runtime.json"))
        .arg("memory")
        .arg("fts-rebuild")
        .arg("--db")
        .arg(&db_path)
        .arg("--json")
        .output()
        .unwrap();

    if output.status.code() != Some(0) {
        panic!(
            "STDOUT: {}\nSTDERR: {}",
            String::from_utf8(output.stdout).unwrap(),
            String::from_utf8(output.stderr).unwrap()
        );
    }

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("output should be valid JSON");

    assert_eq!(parsed["status"], "ok");
    assert_eq!(parsed["rows"], 3);

    // Verify MATCH query works
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    {
        let mut stmt = conn
            .prepare("SELECT count(*) FROM memory_fts WHERE memory_fts MATCH 'content1'")
            .unwrap();
        let match_count: i64 = stmt.query_row([], |row| row.get(0)).unwrap();
        assert_eq!(match_count, 1);

        let mut stmt = conn
            .prepare("SELECT count(*) FROM memory_fts WHERE memory_fts MATCH 'content2'")
            .unwrap();
        let match_count: i64 = stmt.query_row([], |row| row.get(0)).unwrap();
        assert_eq!(match_count, 1);
    }
    conn.close().unwrap();

    // Verify fts-check is ok now
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
fn fts_rebuild_missing_db() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("missing.sqlite3");

    let exe = env!("CARGO_BIN_EXE_xavier");
    let output = std::process::Command::new(exe)
        .env("HOME", tmp.path())
        .env("XAVIER_DATA_DIR", tmp.path())
        .env("XAVIER_CONFIG_PATH", tmp.path().join("config.toml"))
        .env("XAVIER_RUNTIME_STATE_PATH", tmp.path().join("runtime.json"))
        .arg("memory")
        .arg("fts-rebuild")
        .arg("--db")
        .arg(&db_path)
        .arg("--json")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("output should be valid JSON");

    assert_eq!(parsed["status"], "error");
    assert_eq!(parsed["db"].as_str().unwrap(), db_path.to_str().unwrap());
}
