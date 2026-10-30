//! feat-codegraph-walk-hardening: E2E code-graph walk hardening test.

use anyhow::Result;
use code_graph::db::CodeGraphDB;
use code_graph::indexer::Indexer;
use code_graph::query::QueryEngine;
use std::fs::{self, File};
use std::sync::Arc;
use tempfile::TempDir;

xavier::isolate_test_process!();

#[tokio::test]
async fn test_codegraph_walk_hardening_e2e() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let repo_root = temp_dir.path().canonicalize()?;

    // Create .git directory to mark it as a git repo
    fs::create_dir_all(repo_root.join(".git"))?;

    // 1. Valid .rs file
    let valid_file = repo_root.join("valid.rs");
    fs::write(&valid_file, "pub fn valid_target() {}\n")?;

    // 2. A .png with source-looking text (binary extension)
    let fake_png = repo_root.join("fake.png");
    fs::write(&fake_png, "pub fn ignored_png() {}\n")?;

    // 3. A .rs file containing a NUL byte in the first 8 KiB
    let nul_rs = repo_root.join("nul.rs");
    let mut nul_content = vec![b'a'; 4000];
    nul_content.push(0);
    nul_content.extend_from_slice(b"pub fn ignored_nul() {}\n");
    fs::write(&nul_rs, &nul_content)?;

    // 4. A sparse file over the size cap (64 MiB)
    let large_rs = repo_root.join("large.rs");
    let f = File::create(&large_rs)?;
    f.set_len(65 * 1024 * 1024)?; // 65 MiB
    drop(f);
    // Write something at the beginning to look like source? The cap check should fail it based on metadata anyway.

    let memory_db = Arc::new(CodeGraphDB::in_memory()?);
    let indexer = Indexer::new(memory_db.clone());

    // Check collect_files directly to get skip stats since they might not be on IndexStats
    let (_files, skips) = indexer.collect_files(&repo_root)?;

    assert_eq!(
        skips.skipped_binary_ext, 1,
        "Expected 1 binary extension skip (.png)"
    );
    assert_eq!(
        skips.skipped_binary_content, 1,
        "Expected 1 binary content skip (NUL byte)"
    );
    assert_eq!(
        skips.skipped_too_large, 1,
        "Expected 1 too-large skip (65 MiB)"
    );

    // Run the scan
    let stats = indexer.index(&repo_root, false).await?;
    assert_eq!(
        stats.total_files, 1,
        "Only the valid file should be indexed"
    );

    // Query
    let query_engine = QueryEngine::new(memory_db.clone());

    // Check valid target is found
    let res_valid = query_engine.find_by_name("valid_target", 10)?;
    assert_eq!(res_valid.len(), 1, "valid_target should be found");

    // Check other symbols are NOT found
    let res_png = query_engine.find_by_name("ignored_png", 10)?;
    assert!(res_png.is_empty(), "fake.png should not be indexed");

    let res_nul = query_engine.find_by_name("ignored_nul", 10)?;
    assert!(res_nul.is_empty(), "nul.rs should not be indexed");

    Ok(())
}
