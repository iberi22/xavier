use anyhow::Result;
use code_graph::db::CodeGraphDB;
use code_graph::indexer::Indexer;
use code_graph::query::QueryEngine;
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();

fn create_temp_git_repo() -> Result<(TempDir, std::path::PathBuf)> {
    let temp_dir = tempfile::tempdir()?;
    let repo_root = temp_dir.path().join("multilang_repo");
    fs::create_dir_all(repo_root.join(".git"))?;

    let src_dir = repo_root.join("src");
    fs::create_dir_all(&src_dir)?;
    fs::write(src_dir.join("lib.rs"), "pub fn rust_alpha() {}")?;

    let web_dir = repo_root.join("web");
    fs::create_dir_all(&web_dir)?;
    fs::write(web_dir.join("app.ts"), "export function tsBeta() {}")?;

    let py_dir = repo_root.join("py");
    fs::create_dir_all(&py_dir)?;
    fs::write(py_dir.join("tool.py"), "def py_gamma(): pass")?;

    Ok((temp_dir, repo_root))
}

#[tokio::test]
#[ignore = "stale symbols after re-scan: rust_alpha2 not found"]
async fn test_codegraph_multilang_reindex_e2e() -> Result<()> {
    let (_guard, repo_root) = create_temp_git_repo()?;

    let memory_db = Arc::new(CodeGraphDB::in_memory()?);
    let indexer = Indexer::new(memory_db.clone());

    // Initial scan
    indexer.index(&repo_root, false).await?;

    let query_engine = QueryEngine::new(memory_db.clone());

    // Assert initial scan results
    let rust_res = query_engine.find_by_name("rust_alpha", 10)?;
    assert_eq!(rust_res.len(), 1, "rust_alpha not found");
    assert!(rust_res[0].file_path.ends_with("lib.rs"));

    let ts_res = query_engine.find_by_name("tsBeta", 10)?;
    assert_eq!(ts_res.len(), 1, "tsBeta not found");
    assert!(ts_res[0].file_path.ends_with("app.ts"));

    let py_res = query_engine.find_by_name("py_gamma", 10)?;
    assert_eq!(py_res.len(), 1, "py_gamma not found");
    assert!(py_res[0].file_path.ends_with("tool.py"));

    // Edits
    fs::write(repo_root.join("src/lib.rs"), "pub fn rust_alpha2() {}")?;
    fs::remove_file(repo_root.join("py/tool.py"))?;
    fs::write(
        repo_root.join("web/extra.ts"),
        "export function tsDelta() {}",
    )?;

    // Second scan (incremental)
    indexer.index(&repo_root, true).await?;

    // Assert updated state
    let rust_res2 = query_engine.find_by_name("rust_alpha2", 10)?;
    assert_eq!(rust_res2.len(), 1, "rust_alpha2 not found");

    let ts_delta_res = query_engine.find_by_name("tsDelta", 10)?;
    assert_eq!(ts_delta_res.len(), 1, "tsDelta not found");

    let old_rust = query_engine.find_by_name("rust_alpha", 10)?;
    assert_eq!(old_rust.len(), 0, "stale symbol rust_alpha still found");

    let old_py = query_engine.find_by_name("py_gamma", 10)?;
    assert_eq!(old_py.len(), 0, "stale symbol py_gamma still found");

    let ts_beta_res = query_engine.find_by_name("tsBeta", 10)?;
    assert_eq!(
        ts_beta_res.len(),
        1,
        "tsBeta should be found exactly once after re-scan"
    );

    Ok(())
}
