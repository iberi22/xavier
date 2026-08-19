//! E2E CLI Integration Tests for the `verify` subcommand.
//!
//! Tests the xavier verify command and its subcommands by spawning the binary
//! and checking stdout/stderr outputs and exit statuses.

use std::process::{Command, Output};

// ─── Helpers ───────────────────────────────────────────────────────────────

fn xavier_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_xavier"))
}

fn run(args: &[&str]) -> Output {
    xavier_binary()
        .args(args)
        .output()
        .expect("failed to execute xavier binary")
}

// ─── Verify Scan Command Tests ─────────────────────────────────────────────

#[test]
fn test_cli_verify_scan_default() {
    let output = run(&["verify", "scan"]);
    assert!(output.status.success(), "xavier verify scan should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("SYSTEM SCAN RESULTS"),
        "verify scan should contain system scan headers. got: {stdout}"
    );
}

#[test]
fn test_cli_verify_scan_json() {
    let output = run(&["verify", "scan", "--format", "json"]);
    assert!(
        output.status.success(),
        "xavier verify scan --format json should succeed"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_start = stdout.find('{').expect("JSON start brace not found");
    let json_str = &stdout[json_start..];
    let parsed: serde_json::Value =
        serde_json::from_str(json_str).expect("verify scan output should be valid JSON");

    assert!(
        parsed.get("system_info").is_some(),
        "JSON should contain system_info key"
    );
    assert!(
        parsed.get("docker").is_some(),
        "JSON should contain docker key"
    );
    assert!(
        parsed.get("env_vars").is_some(),
        "JSON should contain env_vars key"
    );
}

#[test]
fn test_cli_verify_scan_markdown() {
    let output = run(&["verify", "scan", "--format", "markdown"]);
    assert!(
        output.status.success(),
        "xavier verify scan --format markdown should succeed"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("System Scan Results")
            || stdout.contains("SYSTEM SCAN RESULTS")
            || stdout.contains("##"),
        "verify scan --format markdown should produce markdown headings"
    );
}

// ─── Scan CLI Command Tests ───────────────────────────────────────────────

#[test]
fn test_cli_scan_system_table() {
    let output = run(&["scan", "system"]);
    assert!(output.status.success(), "xavier scan system should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("SYSTEM SCAN RESULTS"),
        "scan system should contain scan headers"
    );
}

#[test]
fn test_cli_scan_system_json() {
    let output = run(&["scan", "system", "-f", "json"]);
    assert!(
        output.status.success(),
        "xavier scan system -f json should succeed"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_start = stdout.find('{').expect("JSON start brace not found");
    let json_str = &stdout[json_start..];
    let parsed: serde_json::Value =
        serde_json::from_str(json_str).expect("scan system output should be valid JSON");

    assert!(
        parsed.get("ollama").is_some(),
        "JSON should contain ollama key"
    );
    assert!(
        parsed.get("docker").is_some(),
        "JSON should contain docker key"
    );
}

#[test]
fn test_cli_scan_system_markdown() {
    let output = run(&["scan", "system", "-f", "markdown"]);
    assert!(
        output.status.success(),
        "xavier scan system -f markdown should succeed"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("System Scan Results") || stdout.contains("##"),
        "scan system -f markdown should contain markdown headings"
    );
}

#[test]
fn test_cli_scan_security_table() {
    let output = run(&["scan", "security"]);
    assert!(
        output.status.success(),
        "xavier scan security should succeed"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("SECURITY SCAN RESULTS") || stdout.contains("Critical File Permissions"),
        "scan security should contain security headers"
    );
}

#[test]
fn test_cli_scan_security_json() {
    let output = run(&["scan", "security", "-f", "json"]);
    assert!(
        output.status.success(),
        "xavier scan security -f json should succeed"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_start = stdout.find('{').expect("JSON start brace not found");
    let json_str = &stdout[json_start..];
    let parsed: serde_json::Value =
        serde_json::from_str(json_str).expect("scan security output should be valid JSON");

    assert!(
        parsed.get("permissions").is_some(),
        "JSON should contain permissions key"
    );
    assert!(
        parsed.get("tokens").is_some(),
        "JSON should contain tokens key"
    );
}

// ─── Maturity CLI Command Tests ─────────────────────────────────────────────

#[test]
fn test_cli_maturity_scan_positional() {
    let output = run(&["maturity", "scan", ".", "-j"]);
    assert!(
        output.status.success(),
        "xavier maturity scan . -j should succeed"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_start = stdout.find('{').expect("JSON start brace not found");
    let json_str = &stdout[json_start..];
    let parsed: serde_json::Value =
        serde_json::from_str(json_str).expect("maturity scan output should be valid JSON");

    assert!(
        parsed.get("features").is_some(),
        "JSON should contain features key"
    );
    assert!(
        parsed.get("summary").is_some(),
        "JSON should contain summary key"
    );
}
