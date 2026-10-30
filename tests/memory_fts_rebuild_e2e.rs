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

    // Insert enough rows so the FTS index structure becomes multi-leaf, ensuring that
    // deleting the index shadow table actually breaks MATCH for later rows.
    {
        let mut stmt = conn
            .prepare("INSERT INTO memory_fts(id, path, content) VALUES (?1, ?2, ?3)")
            .unwrap();
        for i in 1..=1000 {
            stmt.execute(rusqlite::params![i, format!("path{}", i), format!("content{}", i)])
                .unwrap();
        }
    }

    // delete the FTS shadow index table to simulate index corruption.
    // Unlike deleting `_data` which destroys the stored content and panics the vtable constructor,
    // deleting `_idx` leaves the data intact but breaks the search index demonstrably.
    conn.execute("DELETE FROM memory_fts_idx", []).unwrap();

    // Verify MATCH query demonstrably fails to find a known document BEFORE we rebuild it
    let res = conn.query_row(
        "SELECT count(*) FROM memory_fts WHERE memory_fts MATCH 'content500'",
        [],
        |row| row.get::<_, i64>(0)
    ).unwrap();
    assert_eq!(res, 0, "MATCH should fail to find the document because of index corruption");
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
    assert_eq!(parsed["rows"], 1000);

    // Verify MATCH query works after rebuild
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    {
        let mut stmt = conn
            .prepare("SELECT count(*) FROM memory_fts WHERE memory_fts MATCH 'content500'")
            .unwrap();
        let match_count: i64 = stmt.query_row([], |row| row.get(0)).unwrap();
        assert_eq!(match_count, 1, "MATCH should find the document after rebuild");
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
