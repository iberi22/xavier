#![allow(dead_code)]

xavier::isolate_test_process!();

use code_graph::{db::CodeGraphDB, indexer::Indexer};
use rusqlite::Connection;
use std::fs::File;
use std::io::Write;
use std::sync::Arc;

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn test_indexed_symbol_project_root() {
    let temp_db_dir = tempfile::tempdir().unwrap();
    let db_path = temp_db_dir.path().join("test.db");

    let db = Arc::new(CodeGraphDB::new(&db_path).unwrap());

    // 1. First project
    let temp1 = tempfile::tempdir().unwrap();
    let root1 = temp1.path();
    let mut lib1 = File::create(root1.join("lib.rs")).unwrap();
    lib1.write_all(b"fn alpha() {}").unwrap();

    let indexer1 = Indexer::new(db.clone());
    indexer1.index(root1, false).await.unwrap();

    db.checkpoint_wal().unwrap();

    let canonical_root1 = std::fs::canonicalize(root1)
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let conn = Connection::open(&db_path).unwrap();

    let root_from_db: Option<String> = conn
        .query_row(
            "SELECT project_root FROM symbols WHERE name = 'alpha'",
            rusqlite::params![],
            |row: &rusqlite::Row| row.get(0),
        )
        .unwrap();
    assert_eq!(root_from_db, Some(canonical_root1.clone()));

    // 2. Second project
    let temp2 = tempfile::tempdir().unwrap();
    let root2 = temp2.path();
    let mut lib2 = File::create(root2.join("lib2.rs")).unwrap();
    lib2.write_all(b"fn beta() {}").unwrap();

    let indexer2 = Indexer::with_project_id(db.clone(), "proj-b");
    indexer2.index(root2, true).await.unwrap();

    db.checkpoint_wal().unwrap();

    let root_from_db2: Option<String> = conn
        .query_row(
            "SELECT project_root FROM symbols WHERE name = 'beta'",
            rusqlite::params![],
            |row: &rusqlite::Row| row.get(0),
        )
        .unwrap();
    assert_eq!(root_from_db2, Some("proj-b".to_string()));

    // Document existing limitation:
    // `Indexer::index(incremental=true)` globally compares db paths with the current root
    // and deletes anything missing, effectively dropping previous projects' files.
    let alpha_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM symbols WHERE name = 'alpha'",
            rusqlite::params![],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        alpha_count, 0,
        "alpha is deleted due to unscoped clearing limitation"
    );
}
