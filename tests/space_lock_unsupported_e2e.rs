#![allow(unused_imports)]
#[path = "bounded_cli_support/mod.rs"]
mod bounded_cli_support;
use bounded_cli_support::Sandbox;
use std::time::Duration;

#[test]
fn test_space_lock_unsupported_stderr() {
    xavier::isolate_test_process!();
    let sandbox = Sandbox::new();
    let run = sandbox.run(
        sandbox
            .command("http://127.0.0.1:0")
            .arg("space")
            .arg("lock")
            .arg("test-id"),
        Duration::from_secs(10),
    );

    assert!(!run.status.success());
    assert_eq!(
        run.status.code(),
        Some(5),
        "unsupported must exit 5, not the generic failure code 1"
    );
    assert!(run
        .stderr
        .contains("Error: lock is not implemented on this version."));
    assert!(run.stdout.is_empty(), "stdout must be empty");
}

#[test]
fn test_space_lock_unsupported_json() {
    xavier::isolate_test_process!();
    let sandbox = Sandbox::new();
    let run = sandbox.run(
        sandbox
            .command("http://127.0.0.1:0")
            .arg("space")
            .arg("lock")
            .arg("--json")
            .arg("test-id"),
        Duration::from_secs(10),
    );

    assert!(!run.status.success());
    assert_eq!(
        run.status.code(),
        Some(5),
        "unsupported must exit 5, not the generic failure code 1"
    );

    let parsed: serde_json::Value =
        serde_json::from_str(&run.stdout).expect("stdout must be valid JSON");
    let expected = serde_json::json!({
        "command": "space lock",
        "error": "unsupported",
        "ok": false,
        "reason": "spaces runtime lock not available in this version"
    });
    assert_eq!(parsed, expected, "JSON output must match exactly");
}
