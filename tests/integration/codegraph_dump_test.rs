//! Integration test to verify that `perform_dump` successfully creates
//! `.xavier/codegraph.json` after scanning/indexing a small-tree directory structure.

use std::fs;
use std::sync::Arc;
use tempfile::tempdir;

use code_graph::db::CodeGraphDB;
use code_graph::indexer::Indexer;
use code_graph::query::QueryEngine;
use xavier::cli::code_dump::perform_dump;
use xavier::cli::state::CodeGraphState;

#[tokio::test]
async fn test_scan_and_dump_workflow_produces_valid_json() {
    // 1. Create a temp directory
    let temp_dir = tempdir().expect("failed to create temp dir");
    let temp_path = temp_dir.path();

    // 2. Create mock `.git` folder so find_repo_root resolves to `temp_path`
    fs::create_dir_all(temp_path.join(".git")).expect("failed to create mock .git");

    // 3. Write a small source file (e.g. `src/lib.rs` with `pub fn my_cool_test_func() {}`)
    let src_dir = temp_path.join("src");
    fs::create_dir_all(&src_dir).expect("failed to create src dir");
    fs::write(
        src_dir.join("lib.rs"),
        r#"
        pub fn my_cool_test_func() {
            println!("Hello from tests!");
        }
        "#,
    )
    .expect("failed to write lib.rs");

    // 4. Initialize in-memory CodeGraphDB and state
    let db = Arc::new(CodeGraphDB::in_memory().expect("failed to create CodeGraphDB"));
    let indexer = Arc::new(Indexer::new(Arc::clone(&db)));
    let query = Arc::new(QueryEngine::new(Arc::clone(&db)));
    let state = CodeGraphState {
        db: Arc::clone(&db),
        indexer,
        query,
    };

    // 5. Index/scan the temporary directory
    state
        .indexer
        .index(temp_path, true)
        .await
        .expect("indexing failed");

    // Verify symbols are inserted into the database
    let stats = db.stats().expect("failed to query db stats");
    assert!(stats.total_symbols >= 1, "Expected at least 1 symbol in the DB, got {}", stats.total_symbols);

    // 6. Call perform_dump
    let path_str = temp_path.to_str().expect("failed to convert path to string");
    let dump_path = perform_dump(&state, path_str)
        .await
        .expect("perform_dump failed");

    // Verify correct dump path
    let expected_dump_path = temp_path.join(".xavier").join("codegraph.json");
    assert_eq!(dump_path, expected_dump_path);
    assert!(dump_path.exists(), "Dump file does not exist at {}", dump_path.display());

    // 7. Parse the output codegraph.json
    let dump_content = fs::read_to_string(&dump_path).expect("failed to read dump file");
    let dump_json: serde_json::Value = serde_json::from_str(&dump_content)
        .expect("codegraph.json is not a valid JSON document");

    // Assert that the symbols array is present and contains our function
    let symbols = dump_json
        .get("symbols")
        .and_then(|s| s.as_array())
        .expect("No 'symbols' array in codegraph.json");

    assert!(symbols.len() >= 1, "Expected symbols array to have length >= 1");
    let contains_func = symbols.iter().any(|sym| {
        sym.get("name")
            .and_then(|n| n.as_str())
            .map(|n| n == "my_cool_test_func")
            .unwrap_or(false)
    });
    assert!(contains_func, "Dumped symbols do not contain 'my_cool_test_func'. Dump: {}", dump_content);
}
