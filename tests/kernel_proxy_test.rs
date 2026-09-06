//! Integration and verification tests for Xavier RTK Kernel Proxy

use xavier::kernel::filters::{filter_cargo, filter_git, filter_grep, strip_ansi};
use xavier::kernel::runner::execute_proxy_command;

#[test]
fn test_strip_ansi_sequences() {
    let colored = "\x1B[32mSuccess\x1B[0m: compilation \x1B[1;31mfinished\x1B[0m";
    assert_eq!(strip_ansi(colored), "Success: compilation finished");
}

#[test]
fn test_filter_cargo_condenses_ok_tests() {
    let raw = "\
test tests::test_alpha ... ok
test tests::test_beta ... ok
test tests::test_gamma ... ok
test tests::test_delta ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
";
    let filtered = filter_cargo(raw);
    assert!(filtered.contains("all 4 tests passed"));
    assert!(!filtered.contains("test tests::test_alpha ... ok"));
}

#[test]
fn test_filter_cargo_preserves_failures() {
    let raw = "\
test tests::test_alpha ... ok
test tests::test_beta ... FAILED
failures:
---- tests::test_beta stdout ----
thread 'tests::test_beta' panicked at 'explicit panic', src/lib.rs:42:9
failures:
    tests::test_beta
test result: FAILED. 1 failed; 1 passed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s
";
    let filtered = filter_cargo(raw);
    assert!(filtered.contains("=== FAILURES & ERRORS ==="));
    assert!(filtered.contains("tests::test_beta"));
    assert!(filtered.contains("explicit panic"));
    assert!(filtered.contains("passed: 1, failed: 1"));
}

#[test]
fn test_filter_git_removes_noise() {
    let raw = "\
On branch main
Your branch is up to date with 'origin/main'.

Changes not staged for commit:
  (use \"git add <file>...\" to update what will be committed)
  (use \"git restore <file>...\" to discard changes in working directory)
	modified:   src/kernel/mod.rs

no changes added to commit (use \"git add\" to track)
";
    let filtered = filter_git(raw);
    assert!(!filtered.contains("use \"git add"));
    assert!(!filtered.contains("use \"git restore"));
    assert!(filtered.contains("modified:   src/kernel/mod.rs"));
}

#[tokio::test]
async fn test_execute_proxy_command_savings_accounting() {
    // Run echo with repeated test patterns
    let cmd = "echo 'test a ... ok\ntest b ... ok\ntest result: ok. 2 passed; 0 failed;'";
    let res = execute_proxy_command(cmd, None, Some("test_integration_session")).await.unwrap();

    assert_eq!(res.exit_code, 0);
    assert!(res.estimated_raw_tokens > 0);
    assert_eq!(res.command, cmd);
}

#[test]
fn test_filter_grep_truncation() {
    let mut large_grep = String::new();
    for i in 0..120 {
        large_grep.push_str(&format!("src/file_{}.rs:10:fn calculate_{}() {{}}\n", i, i));
    }
    let filtered = filter_grep(&large_grep);
    assert!(filtered.contains("matches truncated"));
}
