//! Integration tests for feat-issue-context-packager.
//!
//! Tests the full pipeline: issue parsing → CodeGraph mapping → PreciseChange generation → package assembly.
//! Uses in-memory CodeGraphDB + tempdir SnapshotManager for isolation.

use std::fs;
use std::sync::Arc;
use tempfile::tempdir;

use code_graph::db::CodeGraphDB;
use code_graph::indexer::Indexer;
use xavier::codebase::issue_context::{
    assemble_package, generate_changes, map_entities_to_codegraph, parse_issue_entities, IssueType,
};
use xavier::codebase::snapshot::SnapshotManager;

/// Helper: set up a temp repo with source files + index into in-memory CodeGraphDB.
async fn setup_test_env() -> (tempfile::TempDir, Arc<CodeGraphDB>, SnapshotManager) {
    let temp_dir = tempdir().expect("failed to create temp dir");
    let temp_path = temp_dir.path();

    // Create mock .git so find_repo_root resolves
    fs::create_dir_all(temp_path.join(".git")).expect("mock .git");

    // Create source files with known symbols
    let src_dir = temp_path.join("src");
    fs::create_dir_all(&src_dir).expect("src dir");

    fs::write(
        src_dir.join("lib.rs"),
        r#"
pub fn search_code(query: &str) -> Vec<String> {
    vec![query.to_string()]
}

pub struct PreciseChange {
    pub file: String,
    pub symbol: String,
}

pub fn build_precise_change(file: &str, symbol: &str) -> PreciseChange {
    PreciseChange {
        file: file.to_string(),
        symbol: symbol.to_string(),
    }
}
"#,
    )
    .expect("write lib.rs");

    fs::write(
        src_dir.join("db.rs"),
        r#"
pub fn init_db(path: &str) -> bool {
    !path.is_empty()
}
"#,
    )
    .expect("write db.rs");

    // Index into in-memory CodeGraphDB
    let db = Arc::new(CodeGraphDB::in_memory().expect("CodeGraphDB"));
    let indexer = Arc::new(Indexer::new(Arc::clone(&db)));
    indexer
        .index(temp_path, true)
        .await
        .expect("index temp repo");

    let snapshot = SnapshotManager::new(temp_path);
    (temp_dir, db, snapshot)
}

// ─── Unit-level tests (parse_issue_entities) ────────────────────────────────

#[test]
fn test_parse_files_from_title_and_body() {
    let title = "Fix search_code in db.rs";
    let body = "The function `search_code` in `src/lib.rs` needs improvement.\nAlso check `src/db.rs`.";
    let entities = parse_issue_entities(title, body);

    let files: Vec<&str> = entities
        .iter()
        .filter(|e| e.kind == "file")
        .map(|e| e.value.as_str())
        .collect();
    assert!(files.contains(&"src/lib.rs"), "Should find lib.rs");
    assert!(files.contains(&"src/db.rs"), "Should find db.rs");
}

#[test]
fn test_parse_symbols_from_backticks() {
    let title = "Improve `PreciseChange` and `build_precise_change`";
    let body = "We need to enhance the `PreciseChange` struct.";
    let entities = parse_issue_entities(title, body);

    let symbols: Vec<&str> = entities
        .iter()
        .filter(|e| e.kind == "symbol")
        .map(|e| e.value.as_str())
        .collect();
    assert!(symbols.contains(&"PreciseChange"));
    assert!(symbols.contains(&"build_precise_change"));
}

#[test]
fn test_parse_feature_refs() {
    let title = "[XAV-09] feat-issue-context-packager: implement";
    let body = "Feature: feat-issue-context-packager, related to FEAT-CG-001.";
    let entities = parse_issue_entities(title, body);

    let features: Vec<&str> = entities
        .iter()
        .filter(|e| e.kind == "feature")
        .map(|e| e.value.as_str())
        .collect();
    assert!(features.contains(&"feat-issue-context-packager"));
    assert!(features.contains(&"FEAT-CG-001"));
}

#[test]
fn test_parse_deduplication() {
    let title = "Fix `search_code`";
    let body = "The `search_code` function in `search_code` module needs work.";
    let entities = parse_issue_entities(title, body);

    let symbols: Vec<&str> = entities
        .iter()
        .filter(|e| e.kind == "symbol")
        .map(|e| e.value.as_str())
        .collect();
    assert_eq!(symbols.len(), 1, "Symbols should be deduplicated");
}

#[test]
fn test_parse_empty_body() {
    let entities = parse_issue_entities("Test issue", "");
    assert!(entities.is_empty(), "Empty body yields no entities");
}

// ─── Integration tests (map + generate + assemble) ──────────────────────────

#[tokio::test]
async fn test_map_entities_finds_symbols_in_codegraph() {
    let (_dir, db, snapshot) = setup_test_env().await;

    let title = "Fix search_code";
    let entities = parse_issue_entities(
        title,
        "The `search_code` function in `src/lib.rs` needs work.",
    );

    let mapped =
        map_entities_to_codegraph(&entities, &db, &snapshot, "test/repo", _dir.path(), title)
            .expect("map_entities");

    // search_code should be found in CodeGraph
    let search_sym = mapped.iter().find(|m| m.entity.value == "search_code");
    assert!(search_sym.is_some(), "search_code should be mapped");
    assert!(search_sym.unwrap().found, "search_code should be found in CodeGraph");
    assert!(search_sym.unwrap().relevance_score > 0.0, "Relevance score should be positive");

    // src/lib.rs should be found (file exists)
    let lib_file = mapped.iter().find(|m| m.entity.value == "src/lib.rs");
    assert!(lib_file.is_some(), "src/lib.rs should be mapped");
    assert!(lib_file.unwrap().found, "src/lib.rs should exist");
}

#[tokio::test]
async fn test_generate_changes_builds_precise_changes() {
    let (_dir, db, snapshot) = setup_test_env().await;

    let title = "Fix search_code";
    let entities = parse_issue_entities(
        title,
        "The `search_code` function in `src/lib.rs` needs work.",
    );

    let mapped =
        map_entities_to_codegraph(&entities, &db, &snapshot, "test/repo", _dir.path(), title)
            .expect("map_entities");

    let changes =
        generate_changes(&mapped, &snapshot, "test/repo", _dir.path()).expect("generate_changes");

    // Should produce at least one PreciseChange for search_code
    assert!(
        !changes.is_empty(),
        "Should generate at least one PreciseChange"
    );
    let sc_change = changes.iter().find(|c| c.symbol == "search_code");
    assert!(sc_change.is_some(), "PreciseChange for search_code");
    let change = sc_change.unwrap();
    assert_eq!(change.file, "src/lib.rs");
    assert!(!change.before_snippet.is_empty(), "before_snippet should have content");
}

#[tokio::test]
async fn test_assemble_package_full_pipeline() {
    let (_dir, db, snapshot) = setup_test_env().await;

    let package = assemble_package(
        "42",
        "Fix search_code and PreciseChange",
        "test/repo",
        "The `search_code` function in `src/lib.rs` needs improvement.\nAlso `build_precise_change` in `src/lib.rs`.",
        &db,
        &snapshot,
        _dir.path(),
    )
    .expect("assemble_package");

    assert_eq!(package.issue_id, "42");
    assert_eq!(package.title, "Fix search_code and PreciseChange");
    assert_eq!(package.repo, "test/repo");
    assert_eq!(package.issue_type, IssueType::Bug);

    // Should have extracted entities
    assert!(
        !package.entities.is_empty(),
        "Should extract entities from issue"
    );

    // Should have mapped entities
    assert!(
        !package.mapped.is_empty(),
        "Should map entities to CodeGraph"
    );

    // Should have generated changes
    assert!(
        !package.changes.is_empty(),
        "Should generate PreciseChanges"
    );

    // Token savings estimate should be present
    assert!(
        package.token_savings_estimate.is_some(),
        "Token savings should be estimated"
    );
    assert!(
        package.token_savings_estimate.unwrap() > 0.0,
        "Token savings should be positive"
    );
}

#[tokio::test]
async fn test_assemble_package_large_issue() {
    let temp_dir = tempdir().expect("failed to create temp dir");
    let temp_path = temp_dir.path();
    fs::create_dir_all(temp_path.join(".git")).expect("mock .git");

    let src_dir = temp_path.join("src");
    fs::create_dir_all(&src_dir).expect("src dir");

    // Create 60 test files and reference them in body
    let mut body = String::from("Refactor large issue context packager across files:\n");
    for i in 0..60 {
        let file_name = format!("file_{}.rs", i);
        let rel_path = format!("src/{}", file_name);
        fs::write(
            src_dir.join(&file_name),
            format!("pub fn func_{}() -> usize {{ {} }}\n", i, i),
        )
        .expect("write mock file");

        body.push_str(&format!("- Modifying `{}` for `func_{}`\n", rel_path, i));
    }

    let db = Arc::new(CodeGraphDB::in_memory().expect("CodeGraphDB"));
    let indexer = Arc::new(Indexer::new(Arc::clone(&db)));
    indexer.index(temp_path, true).await.expect("index repo");
    let snapshot = SnapshotManager::new(temp_path);

    let package = assemble_package(
        "100",
        "Refactor: Large scale refactoring across 60 files",
        "test/repo",
        &body,
        &db,
        &snapshot,
        temp_path,
    )
    .expect("assemble_package_large_issue");

    assert_eq!(package.issue_type, IssueType::Refactor);
    assert!(package.entities.len() >= 60, "Expected at least 60 entities");
    assert!(package.deps.len() >= 60, "Expected at least 60 deps");
}

#[tokio::test]
async fn test_assemble_package_with_no_match() {
    let (_dir, db, snapshot) = setup_test_env().await;

    let package = assemble_package(
        "99",
        "Add completely new feature",
        "test/repo",
        "This issue is about adding a brand new `nonexistent_function` that doesn't exist anywhere.",
        &db,
        &snapshot,
        _dir.path(),
    )
    .expect("assemble_package_no_match");

    // No symbols found → no changes
    assert!(
        package.changes.is_empty(),
        "No changes for nonexistent symbols"
    );
    assert!(
        package.token_savings_estimate.is_none(),
        "No token savings when no changes"
    );
}

#[tokio::test]
async fn test_assemble_package_detects_deps() {
    let (_dir, db, snapshot) = setup_test_env().await;

    let package = assemble_package(
        "50",
        "Fix init_db",
        "test/repo",
        "The `init_db` function in `src/db.rs` needs a retry mechanism.",
        &db,
        &snapshot,
        _dir.path(),
    )
    .expect("assemble_package_deps");

    // src/db.rs should be in deps
    assert!(
        package.deps.contains(&"src/db.rs".to_string()),
        "deps should include src/db.rs"
    );
}

#[tokio::test]
async fn test_assemble_package_suggests_test_files() {
    let (_dir, db, snapshot) = setup_test_env().await;

    // Create a test file that the heuristic should find
    fs::write(
        _dir.path().join("src/lib_test.rs"),
        "#[test] fn test_search_code() {}",
    )
    .expect("write test file");

    let package = assemble_package(
        "60",
        "Fix search_code",
        "test/repo",
        "The `search_code` function in `src/lib.rs` needs improvement.",
        &db,
        &snapshot,
        _dir.path(),
    )
    .expect("assemble_package_test_suggestion");

    // Should suggest lib_test.rs
    assert!(
        package
            .tests_to_fix
            .iter()
            .any(|t| t.contains("lib_test")),
        "Should suggest lib_test.rs"
    );
}
