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
