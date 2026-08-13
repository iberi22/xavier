//! Integration tests for F12 Issue Context Packager.
//!
//! Validates the automatic issue-to- PreciseChange packaging workflow,
//! including parsing, CodeGraphDB mapping, PreciseChange snippet slicing,
//! and endpoint response conformity.

use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

use code_graph::db::CodeGraphDB;
use xavier::codebase::snapshot::SnapshotManager;
use xavier::codebase::issue_context::{
    parse_issue_entities, map_entities_to_codegraph, generate_changes, assemble_package,
    ExtractedEntity, MappedEntity, IssueContextPackage,
};
use xavier::server::f12_routes::{self, F12State};

use axum::{body::Body, http::{Request, StatusCode}};
use http_body_util::BodyExt;
use tower::ServiceExt;

// Helper to set up a temporary workspace with an initialized CodeGraphDB.
fn setup_temp_workspace() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp_dir = tempdir().expect("failed to create temp dir");
    let repo_root = temp_dir.path().join("my-test-repo");
    fs::create_dir_all(&repo_root).unwrap();

    // Create a mock .git folder
    fs::create_dir_all(repo_root.join(".git")).unwrap();

    // Create the expected database folder
    let xavier_dir = repo_root.join(".xavier");
    fs::create_dir_all(&xavier_dir).unwrap();

    let db_path = xavier_dir.join("code_graph.db");

    (temp_dir, repo_root, db_path)
}

#[test]
fn test_parse_issue_entities_basic_coverage() {
    let title = "[BUG] Fix `resolve_active_peers` function";
    let body = r#"
We are facing an issue with `resolve_active_peers` in `src/mesh/peers.rs`.
Please ensure that the feature `feat-issue-context-packager` is also correctly referenced.
Also check `tests/integration/issue_context_test.rs`.
    "#;

    let entities = parse_issue_entities(title, body);

    // Verify file extraction
    let files: Vec<&str> = entities
        .iter()
        .filter(|e| e.kind == "file")
        .map(|e| e.value.as_str())
        .collect();
    assert!(files.contains(&"src/mesh/peers.rs"));
    assert!(files.contains(&"tests/integration/issue_context_test.rs"));

    // Verify symbol extraction
    let symbols: Vec<&str> = entities
        .iter()
        .filter(|e| e.kind == "symbol")
        .map(|e| e.value.as_str())
        .collect();
    assert!(symbols.contains(&"resolve_active_peers"));

    // Verify feature extraction
    let features: Vec<&str> = entities
        .iter()
        .filter(|e| e.kind == "feature")
        .map(|e| e.value.as_str())
        .collect();
    assert!(features.contains(&"feat-issue-context-packager"));
}

#[test]
fn test_assemble_empty_or_basic() {
    let temp_dir = tempdir().unwrap();
    let data_dir = temp_dir.path().join("data");
    fs::create_dir_all(&data_dir).unwrap();

    let repo_root = temp_dir.path().join("repo");
    fs::create_dir_all(&repo_root).unwrap();

    let db_path = repo_root.join("test.db");
    let code_graph_db = CodeGraphDB::new(&db_path).unwrap();
    let snapshot_manager = SnapshotManager::new(&data_dir);

    let package = assemble_package(
        "123",
        "Just a placeholder",
        "my-placeholder-repo",
        "There is nothing here",
        &code_graph_db,
        &snapshot_manager,
        &repo_root,
    )
    .unwrap();

    assert_eq!(package.issue_id, "123");
    assert_eq!(package.title, "Just a placeholder");
    assert_eq!(package.repo, "my-placeholder-repo");
    assert!(package.entities.is_empty());
    assert!(package.mapped.is_empty());
    assert!(package.changes.is_empty());
}

#[test]
fn test_map_entities_to_codegraph() {
    let (_temp_dir, repo_root, db_path) = setup_temp_workspace();

    // Initialize CodeGraphDB and populate with mock symbols
    let db = CodeGraphDB::new(&db_path).expect("failed to open database");

    let sym = code_graph::types::Symbol {
        id: None,
        stable_id: Some("id-peer-resolve".to_string()),
        name: "resolve_active_peers".to_string(),
        kind: code_graph::types::SymbolKind::Function,
        lang: code_graph::types::Language::Rust,
        file_path: "src/mesh/peers.rs".to_string(),
        start_line: 10,
        end_line: 25,
        start_col: 0,
        end_col: 0,
        signature: Some("pub fn resolve_active_peers()".to_string()),
        parent: None,
        complexity: Some(1.5),
    };
    db.insert_symbol(&sym).unwrap();

    // Create the mock file on disk
    let file_path = repo_root.join("src/mesh/peers.rs");
    fs::create_dir_all(file_path.parent().unwrap()).unwrap();
    fs::write(&file_path, "pub fn resolve_active_peers() {\n    // code\n}\n").unwrap();

    // Prepare extracted entities
    let entities = vec![
        ExtractedEntity {
            kind: "symbol".to_string(),
            value: "resolve_active_peers".to_string(),
            offset: 0,
        },
        ExtractedEntity {
            kind: "file".to_string(),
            value: "src/mesh/peers.rs".to_string(),
            offset: 50,
        },
        ExtractedEntity {
            kind: "feature".to_string(),
            value: "feat-issue-context-packager".to_string(),
            offset: 100,
        },
    ];

    let data_dir = repo_root.join("data");
    let snapshot_manager = SnapshotManager::new(&data_dir);

    let mapped = map_entities_to_codegraph(
        &entities,
        &db,
        &snapshot_manager,
        "my-test-repo",
        &repo_root,
    )
    .unwrap();

    assert_eq!(mapped.len(), 3);

    let mapped_symbol = mapped.iter().find(|m| m.entity.kind == "symbol").unwrap();
    assert!(mapped_symbol.found);
    assert_eq!(mapped_symbol.symbol_name.as_deref(), Some("resolve_active_peers"));
    assert_eq!(mapped_symbol.file.as_deref(), Some("src/mesh/peers.rs"));
    assert_eq!(mapped_symbol.start_line, Some(10));
    assert_eq!(mapped_symbol.end_line, Some(25));

    let mapped_file = mapped.iter().find(|m| m.entity.kind == "file").unwrap();
    assert!(mapped_file.found);
    assert_eq!(mapped_file.file.as_deref(), Some("src/mesh/peers.rs"));

    let mapped_feature = mapped.iter().find(|m| m.entity.kind == "feature").unwrap();
    assert!(mapped_feature.found);
}

#[test]
fn test_generate_changes_from_mapped() {
    let (_temp_dir, repo_root, _db_path) = setup_temp_workspace();

    // Write source file content
    let file_path = repo_root.join("src/mesh/peers.rs");
    fs::create_dir_all(file_path.parent().unwrap()).unwrap();
    fs::write(
        &file_path,
        "line 1\nline 2\nline 3\nline 4\npub fn resolve_active_peers() {\n    // actual body\n}\nline 7\n",
    )
    .unwrap();

    let mapped = vec![
        MappedEntity {
            entity: ExtractedEntity {
                kind: "symbol".to_string(),
                value: "resolve_active_peers".to_string(),
                offset: 0,
            },
            found: true,
            symbol_name: Some("resolve_active_peers".to_string()),
            file: Some("src/mesh/peers.rs".to_string()),
            start_line: Some(5),
            end_line: Some(7),
        },
    ];

    let data_dir = repo_root.join("data");
    let snapshot_manager = SnapshotManager::new(&data_dir);

    let changes = generate_changes(
        &mapped,
        &snapshot_manager,
        "my-test-repo",
        &repo_root,
    )
    .unwrap();

    assert_eq!(changes.len(), 1);
    let change = &changes[0];
    assert_eq!(change.symbol, "resolve_active_peers");
    assert_eq!(change.file, "src/mesh/peers.rs");
    assert_eq!(change.start_line, 5);
    assert_eq!(change.end_line, 7);
    assert_eq!(
        change.before_snippet,
        "pub fn resolve_active_peers() {\n    // actual body\n}"
    );
}

#[tokio::test]
async fn test_issue_context_http_endpoint_e2e() {
    let temp_dir = tempdir().unwrap();
    let data_dir = temp_dir.path().join("data_dir");
    fs::create_dir_all(&data_dir).unwrap();

    // F12State looks up repos under state.data_dir.join("repos") in issue_context handler
    let state_repos_dir = data_dir.join("repos");
    let state_repo_root = state_repos_dir.join("my-test-repo");
    fs::create_dir_all(&state_repo_root).unwrap();

    // Setup .xavier and DB directly in the state_repo_root
    let xavier_dir = state_repo_root.join(".xavier");
    fs::create_dir_all(&xavier_dir).unwrap();
    let db_path = xavier_dir.join("code_graph.db");

    // Populate CodeGraph DB with mock symbols directly in the target location
    let db = CodeGraphDB::new(&db_path).unwrap();
    let sym = code_graph::types::Symbol {
        id: None,
        stable_id: Some("id-peer-resolve".to_string()),
        name: "resolve_active_peers".to_string(),
        kind: code_graph::types::SymbolKind::Function,
        lang: code_graph::types::Language::Rust,
        file_path: "src/mesh/peers.rs".to_string(),
        start_line: 2,
        end_line: 4,
        start_col: 0,
        end_col: 0,
        signature: Some("pub fn resolve_active_peers()".to_string()),
        parent: None,
        complexity: Some(1.0),
    };
    db.insert_symbol(&sym).unwrap();
    drop(db); // Flush and close DB connection so it is fully persisted on disk

    // Write file to target location on disk
    let file_path = state_repo_root.join("src/mesh/peers.rs");
    fs::create_dir_all(file_path.parent().unwrap()).unwrap();
    fs::write(&file_path, "line 1\npub fn resolve_active_peers() {\n    println!(\"ok\");\n}\nline 5").unwrap();

    // Create F12State & App Router
    let f12_state = F12State::new(data_dir.clone());
    let app = f12_routes::router(f12_state);

    // Make an issue_context request payload
    let req_body = serde_json::to_string(&serde_json::json!({
        "issue_id": "42",
        "title": "Test peer function resolution",
        "repo": "my-test-repo",
        "body": "Let's fix `resolve_active_peers` in `src/mesh/peers.rs`.",
        "repo_root": null,
    }))
    .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/f12/issue-context")
                .header("content-type", "application/json")
                .body(Body::from(req_body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let package: IssueContextPackage = serde_json::from_slice(&body).unwrap();

    assert_eq!(package.issue_id, "42");
    assert_eq!(package.repo, "my-test-repo");
    assert_eq!(package.entities.len(), 2); // symbol + file

    // Verify mapped elements
    assert_eq!(package.mapped.len(), 2);
    let sym_mapped = package.mapped.iter().find(|m| m.entity.kind == "symbol").unwrap();
    assert!(sym_mapped.found);
    assert_eq!(sym_mapped.symbol_name.as_deref(), Some("resolve_active_peers"));

    // Verify generated changes
    assert_eq!(package.changes.len(), 1);
    let change = &package.changes[0];
    assert_eq!(change.symbol, "resolve_active_peers");
    assert_eq!(change.file, "src/mesh/peers.rs");
    assert_eq!(change.start_line, 2);
    assert_eq!(change.end_line, 4);
    assert_eq!(
        change.before_snippet,
        "pub fn resolve_active_peers() {\n    println!(\"ok\");\n}"
    );

    // Clean up
    fs::remove_dir_all(&data_dir).ok();
}
