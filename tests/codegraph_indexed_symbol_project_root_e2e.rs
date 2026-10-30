#![allow(dead_code)]

xavier::isolate_test_process!();

use code_graph::{
    db::CodeGraphDB,
    indexer::Indexer,
};
use std::sync::Arc;
use std::fs::File;
use std::io::Write;
use rusqlite::Connection;

#[tokio::test(flavor = "current_thread")]
async fn test_indexed_symbol_project_root() {
    let temp_db_dir = tempfile::tempdir().unwrap();
    let db_path = temp_db_dir.path().join("test.db");

    let db = Arc::new(CodeGraphDB::new(&db_path).unwrap());

    // 1. Default indexer -> root should be canonicalized temp dir
    let temp1 = tempfile::tempdir().unwrap();
    let root1 = temp1.path();
    let mut lib1 = File::create(root1.join("lib.rs")).unwrap();
    lib1.write_all(b"fn alpha() {}").unwrap();

    let indexer1 = Indexer::new(db.clone());
    indexer1.index(root1, false).await.unwrap();

    db.checkpoint_wal().unwrap();

    let canonical_root1 = std::fs::canonicalize(root1).unwrap().to_string_lossy().into_owned();

    let conn = Connection::open(&db_path).unwrap();

    let root_from_db: Option<String> = conn.query_row(
        "SELECT project_root FROM symbols WHERE name = 'alpha'",
        rusqlite::params![],
        |row: &rusqlite::Row| row.get(0)
    ).unwrap();
    assert_eq!(root_from_db, Some(canonical_root1));

    // We cannot easily do an incremental index with a completely different root dir without erasing the old data
    // because `Indexer::index` verifies hash parity and deletes everything not in the new root. Wait! It deletes it because of hash parity!
    // But `alpha` is still in the DB! Let's check why `beta` is not there.
    // If we use the same root but change the indexer's project_id, we can index a second file.

    let mut lib2 = File::create(root1.join("lib2.rs")).unwrap();
    lib2.write_all(b"fn beta() {}").unwrap();

    let indexer2 = Indexer::with_project_id(db.clone(), "proj-b");
    indexer2.index(root1, true).await.unwrap();

    db.checkpoint_wal().unwrap();

    let root_from_db2: Option<String> = conn.query_row(
        "SELECT project_root FROM symbols WHERE name = 'beta'",
        rusqlite::params![],
        |row: &rusqlite::Row| row.get(0)
    ).unwrap();
    assert_eq!(root_from_db2, Some("proj-b".to_string()));

    // first dir's rows are unchanged
    let root_from_db1_again: Option<String> = conn.query_row(
        "SELECT project_root FROM symbols WHERE name = 'alpha'",
        rusqlite::params![],
        |row: &rusqlite::Row| row.get(0)
    ).unwrap();
    assert_eq!(root_from_db1_again, Some(std::fs::canonicalize(root1).unwrap().to_string_lossy().into_owned()));
}
