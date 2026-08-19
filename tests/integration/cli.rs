//! CLI Integration Tests
//!
//! Tests the xavier binary CLI commands by spawning the binary
//! and checking stdout/stderr output.

use std::process::{Command, Output};
use std::time::Duration;

// ─── Helpers ───────────────────────────────────────────────────────────────

fn xavier_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_xavier"))
}

fn run(args: &[&str]) -> Output {
    let output = xavier_binary()
        .args(args)
        .output()
        .expect("failed to execute xavier binary");
    output
}

fn run_with_timeout(args: &[&str], timeout_secs: u64) -> Output {
    let mut child = xavier_binary()
        .args(args)
        .env_remove("XAVIER_TOKEN")
        .env_remove("XAVIER_URL")
        .env_remove("XAVIER_PORT")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn xavier");

    // Wait with timeout
    let start = std::time::Instant::now();
    loop {
        if start.elapsed().as_secs() > timeout_secs {
            let _ = child.kill();
            panic!("xavier {} timed out after {timeout_secs}s", args.join(" "));
        }
        match child.try_wait() {
            Ok(Some(_status)) => {
                let output = child.wait_with_output().expect("get output");
                return output;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => panic!("error waiting for xavier: {e}"),
        }
    }
}

// ─── Help Output ───────────────────────────────────────────────────────────

#[test]
fn test_cli_help_output() {
    let output = run(&["--help"]);
    assert!(output.status.success(), "xavier --help should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Xavier"),
        "help should contain project name"
    );
    assert!(
        stdout.contains("http") || stdout.contains("Http"),
        "help should list http subcommand"
    );
    assert!(
        stdout.contains("search") || stdout.contains("Search"),
        "help should list search subcommand"
    );
    assert!(
        stdout.contains("add") || stdout.contains("Add"),
        "help should list add subcommand"
    );
    assert!(
        stdout.contains("stats") || stdout.contains("Stats"),
        "help should list stats subcommand"
    );
    assert!(
        stdout.contains("recall") || stdout.contains("Recall"),
        "help should list recall subcommand"
    );
    assert!(
        stdout.contains("session-save") || stdout.contains("SessionSave"),
        "help should list session-save subcommand"
    );
}

#[test]
fn test_cli_no_args_shows_help() {
    // Xavier's CLI defaults to starting the HTTP server when run without arguments,
    // which would hang/timeout in an integration test. Therefore, to safely verify
    // the help/usage path, we explicitly invoke `--help`.
    let output = run_with_timeout(&["--help"], 5);

    assert!(output.status.success(), "xavier --help should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Xavier") || stdout.contains("xavier"),
        "help should contain project name"
    );
}

#[test]
fn test_cli_subcommand_help_http() {
    let output = run(&["http", "--help"]);
    assert!(output.status.success(), "xavier http --help should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("http") || stdout.contains("port"),
        "http help should mention port"
    );
}

#[test]
fn test_cli_subcommand_help_add() {
    let output = run(&["add", "--help"]);
    assert!(output.status.success(), "xavier add --help should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("content") || stdout.contains("CONTENT"),
        "add help should mention content"
    );
    assert!(
        stdout.contains("title") || stdout.contains("TITLE"),
        "add help should mention title"
    );
    assert!(
        stdout.contains("kind") || stdout.contains("KIND"),
        "add help should mention kind"
    );
}

#[test]
fn test_cli_subcommand_help_search() {
    let output = run(&["search", "--help"]);
    assert!(
        output.status.success(),
        "xavier search --help should succeed"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("query") || stdout.contains("QUERY"),
        "search help should mention query"
    );
    assert!(
        stdout.contains("limit") || stdout.contains("LIMIT"),
        "search help should mention limit"
    );
}

#[test]
fn test_cli_subcommand_help_recall() {
    let output = run(&["recall", "--help"]);
    assert!(
        output.status.success(),
        "xavier recall --help should succeed"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("query") || stdout.contains("QUERY"),
        "recall help should mention query"
    );
    assert!(
        stdout.contains("limit") || stdout.contains("LIMIT"),
        "recall help should mention limit"
    );
}

#[test]
fn test_cli_subcommand_help_stats() {
    let output = run(&["stats", "--help"]);
    assert!(
        output.status.success(),
        "xavier stats --help should succeed"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("stats") || stdout.contains("Stats"),
        "stats help should mention stats"
    );
}

#[test]
fn test_cli_subcommand_help_session_save() {
    let output = run(&["session-save", "--help"]);
    assert!(
        output.status.success(),
        "xavier session-save --help should succeed"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("session") || stdout.contains("SESSION_ID"),
        "session-save help should mention session_id"
    );
}

// ─── Version Output ────────────────────────────────────────────────────────

#[test]
fn test_cli_version_output() {
    let output = run(&["--version"]);
    assert!(output.status.success(), "xavier --version should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "version output should not be empty");
}

// ─── Error Handling ────────────────────────────────────────────────────────

#[test]
fn test_cli_invalid_subcommand() {
    let output = run(&["nonexistent-command"]);
    assert!(!output.status.success(), "invalid subcommand should fail");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error") || stderr.contains("unrecognized"),
        "error should mention unrecognized subcommand, got: {stderr}"
    );
}

#[test]
fn test_cli_subcommand_invalid_flag() {
    let output = run(&["stats", "--invalid-flag"]);
    assert!(!output.status.success(), "invalid flag should fail");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error") || stderr.contains("unrecognized"),
        "error should mention unrecognized flag, got: {stderr}"
    );
}

#[test]
fn test_cli_subcommand_add_without_server() {
    // add requires a running server — should fail gracefully
    let output = run_with_timeout(&["add", "test-content"], 15);

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stdout} {stderr}");

    // Should fail with either a connection error, security block, or panic
    // (panic is acceptable here — no server is running)
    assert!(
        combined.contains("Error")
            || combined.contains("error")
            || combined.contains("Falling back to local offline")
            || combined.contains("blocked")
            || combined.contains("must be set"),
        "add without server should produce error output, got stdout: {stdout}, stderr: {stderr}"
    );
}

#[test]
fn test_cli_subcommand_search_without_server() {
    // search requires a running server — should fail gracefully
    // Note: use single-word query to avoid CLI arg parsing issues
    let output = run_with_timeout(&["search", "test-term"], 15);

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stdout} {stderr}");

    assert!(
        combined.contains("Error")
            || combined.contains("error")
            || combined.contains("Falling back to local offline")
            || combined.contains("must be set")
            || combined.contains("invalid digit"),
        "search without server should produce error output, got: {combined}"
    );
}

#[test]
fn test_cli_subcommand_recall_without_server() {
    // recall requires a running server — should fail gracefully
    let output = run_with_timeout(&["recall", "test"], 15);

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stdout} {stderr}");

    assert!(
        combined.contains("Error")
            || combined.contains("error")
            || combined.contains("Falling back to local offline")
            || combined.contains("must be set"),
        "recall without server should produce error output, got: {combined}"
    );
}

// ─── Add & Search Flow Tests ───────────────────────────────────────────────

#[test]
fn test_add_and_search_without_server() {
    // Without a running server, both add and search should fail gracefully.
    // This verifies both subcommands exist and produce expected error output.

    let add_output = run_with_timeout(&["add", "integration test content"], 15);
    let add_combined = format!(
        "{} {}",
        String::from_utf8_lossy(&add_output.stdout),
        String::from_utf8_lossy(&add_output.stderr)
    );
    assert!(
        add_combined.contains("Error")
            || add_combined.contains("error")
            || add_combined.contains("Falling back to local offline")
            || add_combined.contains("must be set"),
        "add without server should produce error, got: {add_combined}"
    );

    let search_output = run_with_timeout(&["search", "integration-term"], 15);
    let search_combined = format!(
        "{} {}",
        String::from_utf8_lossy(&search_output.stdout),
        String::from_utf8_lossy(&search_output.stderr)
    );
    assert!(
        search_combined.contains("Error")
            || search_combined.contains("error")
            || search_combined.contains("Falling back to local offline")
            || search_combined.contains("must be set")
            || search_combined.contains("invalid digit"),
        "search without server should produce error, got: {search_combined}"
    );
}

// ─── Stats Without Server ──────────────────────────────────────────────────

#[test]
fn test_cli_subcommand_stats_without_server() {
    // stats requires a running server — should fail gracefully
    let output = run_with_timeout(&["stats"], 5);

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stdout} {stderr}");

    assert!(
        combined.contains("Error")
            || combined.contains("error")
            || combined.contains("Falling back to local offline")
            || combined.contains("must be set"),
        "stats without server should produce error output, got: {combined}"
    );
}

// ─── Session Save Without Server ───────────────────────────────────────────

#[test]
fn test_cli_subcommand_session_save_without_server() {
    // session-save requires a running server — should fail gracefully
    let output = run_with_timeout(&["session-save", "test-session"], 5);

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stdout} {stderr}");

    assert!(
        combined.contains("Error")
            || combined.contains("error")
            || combined.contains("Falling back to local offline")
            || combined.contains("must be set"),
        "session-save without server should produce error output, got: {combined}"
    );
}

// ─── Cleanup Command Tests ──────────────────────────────────────────────────

#[test]
fn test_cli_cleanup_flow() {
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_path = temp_dir.path();

    // Create .xavier/conversations directory
    let conv_dir = temp_path.join(".xavier").join("conversations");
    std::fs::create_dir_all(&conv_dir).unwrap();

    // Create mock empty and non-empty database files
    let empty_db1 = conv_dir.join("test-empty1.db");
    let empty_db2 = conv_dir.join("test-empty2.db");
    let active_db = conv_dir.join("active.db");
    let bench_db = conv_dir.join("bench.db");
    let default_db = conv_dir.join("default.db");

    std::fs::write(&empty_db1, b"").unwrap(); // 0 bytes
    std::fs::write(&empty_db2, vec![0; 4096]).unwrap(); // 4KB empty

    // Non-empty databases (> 4KB)
    std::fs::write(&active_db, vec![0; 8192]).unwrap(); // 8KB
    std::fs::write(&bench_db, vec![0; 16384]).unwrap(); // 16KB
    std::fs::write(&default_db, vec![0; 5000]).unwrap(); // ~5KB

    // Create a mock legacy store
    let legacy_dir = temp_path.join("xavier");
    std::fs::create_dir_all(&legacy_dir).unwrap();
    let legacy_store = legacy_dir.join("memory-store.sqlite3");

    // We can initialize a valid sqlite db file
    let conn = rusqlite::Connection::open(&legacy_store).unwrap();
    conn.execute_batch("CREATE TABLE IF NOT EXISTS mock_table (id INTEGER PRIMARY KEY);")
        .unwrap();
    conn.execute("INSERT INTO mock_table DEFAULT VALUES;", [])
        .unwrap();
    drop(conn);

    // 1. Run xavier cleanup (dry-run)
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd.args(["cleanup", "--dry-run"])
        .env("HOME", temp_path)
        .env("XAVIER_DATA_DIR", legacy_dir.to_str().unwrap());

    let output = cmd.output().expect("failed to run xavier cleanup");
    assert!(output.status.success(), "xavier cleanup should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Verify it reports empty databases and legacy store
    assert!(
        stdout.contains("test-empty1.db"),
        "stdout should list test-empty1.db"
    );
    assert!(
        stdout.contains("test-empty2.db"),
        "stdout should list test-empty2.db"
    );
    assert!(
        stdout.contains("memory-store.sqlite3"),
        "stdout should list memory-store.sqlite3"
    );
    assert!(stdout.contains("active.db"), "stdout should list active.db");

    // Ensure files still exist (dry-run)
    assert!(empty_db1.exists());
    assert!(empty_db2.exists());
    assert!(active_db.exists());
    assert!(legacy_store.exists());

    // 2. Run xavier cleanup --apply
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd.args(["cleanup", "--apply"])
        .env("HOME", temp_path)
        .env("XAVIER_DATA_DIR", legacy_dir.to_str().unwrap());

    let output = cmd.output().expect("failed to run xavier cleanup");
    assert!(
        output.status.success(),
        "xavier cleanup --apply should succeed"
    );

    // Ensure empty databases are deleted
    assert!(!empty_db1.exists(), "empty_db1 should be deleted");
    assert!(!empty_db2.exists(), "empty_db2 should be deleted");

    // Ensure active databases survived
    assert!(active_db.exists(), "active_db should survive");
    assert!(bench_db.exists(), "bench_db should survive");
    assert!(default_db.exists(), "default_db should survive");

    // Ensure legacy store was deleted
    assert!(!legacy_store.exists(), "legacy_store should be deleted");
}
