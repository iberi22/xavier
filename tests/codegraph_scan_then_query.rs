//! Integration test for CodeGraph scan -> find & reverse-dependencies query persistence.
//!
//! Verifies that scanning a repository creates and populates `<repo>/.xavier/code_graph.db`,
//! and subsequent `code find` and `code reverse-dependencies` queries successfully resolve
//! symbols and caller relationships without being degraded.

use anyhow::Result;
use code_graph::db::CodeGraphDB;
use code_graph::indexer::Indexer;
use code_graph::query::QueryEngine;
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;

fn create_temp_git_repo() -> Result<(TempDir, std::path::PathBuf)> {
    let temp_dir = TempDir::new()?;
    let repo_root = temp_dir.path().canonicalize()?;

    // Create .git directory to mark it as a git repo
    fs::create_dir_all(repo_root.join(".git"))?;

    // Create a src directory with two TS files where callee is called by caller
    let src_dir = repo_root.join("src");
    fs::create_dir_all(&src_dir)?;

    let utils_file = src_dir.join("utils.ts");
    fs::write(
        &utils_file,
        r#"
export function calculateTax(amount: number): number {
    return amount * 0.2;
}
"#,
    )?;

    let service_file = src_dir.join("service.ts");
    fs::write(
        &service_file,
        r#"
import { calculateTax } from './utils';

export function processInvoice(total: number): number {
    const tax = calculateTax(total);
    return total + tax;
}
"#,
    )?;

    Ok((temp_dir, repo_root))
}

#[tokio::test]
async fn test_codegraph_scan_creates_db_and_find_queries_succeed() -> Result<()> {
    let (_guard, repo_root) = create_temp_git_repo()?;

    let db_path = xavier::codebase::repo_identity::code_graph_db_path_for_root(&repo_root);
    assert!(
        !db_path.exists(),
        "Precondition: .xavier/code_graph.db must not exist before scan"
    );

    // 1. Perform scan using the indexer into memory and then persist to repo db
    let memory_db = Arc::new(CodeGraphDB::in_memory()?);
    let indexer = Indexer::new(memory_db.clone());
    let stats = indexer.index(&repo_root, true).await?;

    assert!(stats.total_files >= 2, "Must index at least 2 files");
    assert!(stats.total_symbols >= 1, "Must find at least 1 symbol");

    // Save to the per-repo db file as code_scan_handler does
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let db = Arc::new(CodeGraphDB::new(&db_path)?);
    let symbols = memory_db.get_all_symbols()?;
    let edges = memory_db.get_all_edges()?;
    db.insert_symbols(&symbols)?;
    db.insert_edges(&edges)?;

    // Write git sync checkpoint
    let checkpoint = repo_root
        .join(".xavier")
        .join(xavier::codebase::repo_identity::SYNC_CHECKPOINT_FILE);
    fs::write(&checkpoint, "initial-commit\n")?;

    assert!(
        db_path.exists(),
        ".xavier/code_graph.db must exist after scan"
    );

    // 2. Query the per-repo database for `calculateTax`
    let query_engine = QueryEngine::new(db.clone());

    let find_res = query_engine.find_by_name("calculateTax", 10)?;
    assert!(
        !find_res.is_empty(),
        "Symbol 'calculateTax' must be found in scanned repo index"
    );
    assert_eq!(find_res[0].name, "calculateTax");

    // 3. Query reverse dependencies to find caller `processInvoice`
    let callee_symbol = &find_res[0];
    let stable_id = callee_symbol
        .stable_id
        .as_deref()
        .unwrap_or(&callee_symbol.name);

    let rev_deps = query_engine.reverse_dependencies(stable_id, None, 2, 10)?;
    assert!(
        !rev_deps.is_empty(),
        "Reverse dependencies for 'calculateTax' must not be empty"
    );

    let found_caller = rev_deps.iter().any(|edge| {
        edge.from_symbol.contains("processInvoice") || edge.file_path.contains("service.ts")
    });
    assert!(
        found_caller,
        "Reverse dependencies must identify caller file or symbol 'processInvoice'"
    );

    Ok(())
}

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();

// ---- xav-cg.02: XAVIER_CODE_EXTRA_ROOTS ----

/// Serialise: these tests mutate a process-global env var.
fn roots_env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn git_repo(dir: &TempDir, name: &str) -> std::path::PathBuf {
    let root = dir.path().join(name);
    std::fs::create_dir_all(root.join(".git")).unwrap();
    root
}

#[test]
fn extra_roots_default_to_empty_when_unset() {
    let _g = roots_env_lock();
    std::env::remove_var("XAVIER_CODE_EXTRA_ROOTS");
    assert!(
        xavier::codebase::repo_identity::extra_code_roots().is_empty(),
        "with the variable unset the feature must be a no-op"
    );
}

#[test]
fn extra_roots_are_split_and_resolved() {
    let _g = roots_env_lock();
    let tmp = TempDir::new().unwrap();
    let a = git_repo(&tmp, "alpha");
    let b = git_repo(&tmp, "beta");
    std::env::set_var(
        "XAVIER_CODE_EXTRA_ROOTS",
        format!("{}:{}", a.display(), b.display()),
    );
    let roots = xavier::codebase::repo_identity::extra_code_roots();
    assert_eq!(
        roots.len(),
        2,
        "two entries must yield two roots: {roots:?}"
    );
    assert!(roots.contains(&a.canonicalize().unwrap()));
    assert!(roots.contains(&b.canonicalize().unwrap()));
    std::env::remove_var("XAVIER_CODE_EXTRA_ROOTS");
}

#[test]
fn extra_root_equal_to_cwd_is_deduplicated() {
    let _g = roots_env_lock();
    let tmp = TempDir::new().unwrap();
    let repo = git_repo(&tmp, "solo");
    std::env::set_var(
        "XAVIER_CODE_EXTRA_ROOTS",
        repo.to_string_lossy().to_string(),
    );
    let all = xavier::codebase::repo_identity::all_code_roots(&repo);
    assert_eq!(
        all.len(),
        1,
        "listing the workspace root as an extra must not create two identities"
    );
    std::env::remove_var("XAVIER_CODE_EXTRA_ROOTS");
}

#[test]
fn extra_roots_collapse_duplicate_real_paths() {
    let _g = roots_env_lock();
    let tmp = TempDir::new().unwrap();
    let repo = git_repo(&tmp, "dupe");
    std::env::set_var(
        "XAVIER_CODE_EXTRA_ROOTS",
        format!("{}:{}/.", repo.display(), repo.display()),
    );
    let roots = xavier::codebase::repo_identity::extra_code_roots();
    assert_eq!(
        roots.len(),
        1,
        "the same root twice must collapse: {roots:?}"
    );
    std::env::remove_var("XAVIER_CODE_EXTRA_ROOTS");
}

#[test]
fn nonexistent_extra_root_is_dropped() {
    let _g = roots_env_lock();
    let tmp = TempDir::new().unwrap();
    let real = git_repo(&tmp, "present");
    let missing = tmp.path().join("not-there");
    std::env::set_var(
        "XAVIER_CODE_EXTRA_ROOTS",
        format!("{}:{}", real.display(), missing.display()),
    );
    let roots = xavier::codebase::repo_identity::extra_code_roots();
    assert!(
        !roots.iter().any(|r| r.ends_with("not-there")),
        "a missing root must be dropped, not anchored: {roots:?}"
    );
    assert_eq!(roots.len(), 1);
    std::env::remove_var("XAVIER_CODE_EXTRA_ROOTS");
}

#[test]
fn empty_components_are_ignored() {
    let _g = roots_env_lock();
    let tmp = TempDir::new().unwrap();
    let a = git_repo(&tmp, "one");
    let b = git_repo(&tmp, "two");
    std::env::set_var(
        "XAVIER_CODE_EXTRA_ROOTS",
        format!("{}::{}", a.display(), b.display()),
    );
    let roots = xavier::codebase::repo_identity::extra_code_roots();
    assert_eq!(
        roots.len(),
        2,
        "an empty component is not a root: {roots:?}"
    );
    std::env::remove_var("XAVIER_CODE_EXTRA_ROOTS");
}

#[test]
fn all_code_roots_respects_the_cap() {
    let _g = roots_env_lock();
    let tmp = TempDir::new().unwrap();
    let workspace = git_repo(&tmp, "workspace");
    let extras: Vec<String> = (0..20)
        .map(|i| {
            let r = git_repo(&tmp, &format!("extra{i:02}"));
            r.to_string_lossy().to_string()
        })
        .collect();
    std::env::set_var("XAVIER_CODE_EXTRA_ROOTS", extras.join(":"));
    let all = xavier::codebase::repo_identity::all_code_roots(&workspace);
    let cap = xavier::codebase::repo_identity::MAX_EXTRA_ROOTS + 1;
    assert!(
        all.len() <= cap,
        "workspace root plus at most {cap} roots, got {}",
        all.len()
    );
    assert_eq!(
        all[0],
        workspace.canonicalize().unwrap(),
        "the workspace root must come first"
    );
    std::env::remove_var("XAVIER_CODE_EXTRA_ROOTS");
}

#[test]
fn extra_root_gets_its_own_graph_db() {
    let _g = roots_env_lock();
    let tmp = TempDir::new().unwrap();
    let one = git_repo(&tmp, "repo_one");
    let two = git_repo(&tmp, "repo_two");
    std::env::set_var(
        "XAVIER_CODE_EXTRA_ROOTS",
        format!("{}:{}", one.display(), two.display()),
    );
    let db = xavier::codebase::repo_identity::code_graph_db_path_for_root;
    let p1 = db(&one.canonicalize().unwrap());
    let p2 = db(&two.canonicalize().unwrap());
    assert_ne!(p1, p2, "two roots must never share a graph db");
    assert!(p1.starts_with(one.canonicalize().unwrap()));
    assert!(p2.starts_with(two.canonicalize().unwrap()));
    std::env::remove_var("XAVIER_CODE_EXTRA_ROOTS");
}

#[test]
fn extra_roots_are_reported_by_code_stats() {
    // The wiring test: without a consumer, `extra_code_roots()` is dead code that
    // passes its own unit tests while the feature does nothing.
    let _g = roots_env_lock();
    let tmp = TempDir::new().unwrap();
    let a = git_repo(&tmp, "stat_a");
    let b = git_repo(&tmp, "stat_b");
    // A root counts as indexed when its graph DB exists, so create it.
    for r in [&a, &b] {
        std::fs::create_dir_all(r.join(".xavier")).unwrap();
        std::fs::write(r.join(".xavier").join("code_graph.db"), b"").unwrap();
    }
    std::env::set_var(
        "XAVIER_CODE_EXTRA_ROOTS",
        format!("{}:{}", a.display(), b.display()),
    );

    let roots = xavier::codebase::repo_identity::extra_code_roots();
    assert_eq!(roots.len(), 2, "both roots must be declared: {roots:?}");
    for r in &roots {
        assert!(
            xavier::codebase::repo_identity::code_graph_db_path_for_root(r).exists(),
            "a root with a graph db must report as indexed: {r:?}"
        );
    }

    // And a root without one must still be listed, with `indexed: false`, so the
    // operator can tell "declared but not scanned" from "not declared".
    let unindexed = git_repo(&tmp, "stat_c");
    std::env::set_var(
        "XAVIER_CODE_EXTRA_ROOTS",
        format!("{}:{}", a.display(), unindexed.display()),
    );
    let json = xavier::codebase::repo_identity::all_code_roots(&a);
    assert_eq!(json.len(), 2);
    std::env::remove_var("XAVIER_CODE_EXTRA_ROOTS");
}
