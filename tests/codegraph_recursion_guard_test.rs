//! Regression test for codegraph re-entry guard and recursion depth limit.

use std::process::Command;

#[test]
fn test_codegraph_recursion_guard_aborts_immediately() {
    let codegraph_bin = xavier::codebase::codegraph_sidecar::resolve_codegraph_binary();
    if codegraph_bin.is_none() {
        eprintln!("code-graph binary not compiled yet; skipping executable test");
        return;
    }
    let bin_path = codegraph_bin.unwrap();

    // Test re-entry guard with XAVIER_CODEGRAPH_ACTIVE=1
    let output1 = Command::new(&bin_path)
        .env("XAVIER_CODEGRAPH_ACTIVE", "1")
        .env("XAVIER_CODEGRAPH_DEPTH", "1")
        .arg("--version")
        .output()
        .expect("Failed to execute codegraph binary");

    assert!(output1.status.success());
    let stderr1 = String::from_utf8_lossy(&output1.stderr);
    assert!(
        stderr1.contains("Recursive codegraph execution detected"),
        "Expected recursion detection warning in stderr, got: {}",
        stderr1
    );

    // Test max depth guard with XAVIER_CODEGRAPH_DEPTH=2
    let output2 = Command::new(&bin_path)
        .env("XAVIER_CODEGRAPH_ACTIVE", "0")
        .env("XAVIER_CODEGRAPH_DEPTH", "2")
        .arg("--version")
        .output()
        .expect("Failed to execute codegraph binary");

    assert!(output2.status.success());
    let stderr2 = String::from_utf8_lossy(&output2.stderr);
    assert!(
        stderr2.contains("Recursive codegraph execution detected"),
        "Expected recursion detection warning in stderr, got: {}",
        stderr2
    );
}
