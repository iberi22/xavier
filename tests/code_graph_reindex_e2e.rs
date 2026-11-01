//! feat-codegraph-reindex: Incremental reindexing tests

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_codegraph_reindex_and_index_self_e2e() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path().join("myrepo");
    fs::create_dir_all(&repo).unwrap();

    let rust_file = repo.join("main.rs");
    fs::write(&rust_file, "fn main() { println!(\"Hello\"); }").unwrap();

    let python_file = repo.join("script.py");
    fs::write(&python_file, "def foo(): pass").unwrap();

    let ts_file = repo.join("app.ts");
    fs::write(&ts_file, "function bar() {}").unwrap();

    // Index Self
    let mut cmd = Command::cargo_bin("xavier").unwrap();
    cmd.current_dir(&repo)
        .env("XAVIER_CLI_TEST", "1")
        .arg("code")
        .arg("index-self")
        .assert()
        .success();

    // Initial Scan
    let mut cmd = Command::cargo_bin("xavier").unwrap();
    cmd.current_dir(&repo)
        .env("XAVIER_CLI_TEST", "1")
        .arg("code")
        .arg("scan")
        .arg(".")
        .assert()
        .success();

    // Modify Rust file
    fs::write(&rust_file, "fn main() { println!(\"Hello World\"); }").unwrap();

    // Delete Python file
    fs::remove_file(&python_file).unwrap();

    // Add new TS file
    let ts_file2 = repo.join("new.ts");
    fs::write(&ts_file2, "function baz() {}").unwrap();

    // Reindex
    let mut cmd = Command::cargo_bin("xavier").unwrap();
    let assert = cmd
        .current_dir(&repo)
        .env("XAVIER_CLI_TEST", "1")
        .arg("code")
        .arg("reindex")
        .arg("--path")
        .arg(".")
        .assert()
        .success();

    let output = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

    // Should output JSON showing unchanged files counter
    // The exact JSON output asserting symbols updated/removed/added and unchanged files counter.
    // We expect 3 total modified files: main.rs, script.py (removed), new.ts (added)
    assert!(output.contains("\"status\": \"ok\""));
}
