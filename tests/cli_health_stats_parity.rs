//! feat-cli-health-stats-parity: exercise the actual CLI HTTP boundary.
mod bounded_cli_support;
use bounded_cli_support::{MockServer, Sandbox};
use std::time::Duration;
xavier::isolate_test_process!();

fn degraded_health() -> bounded_cli_support::Run {
    let server = MockServer::new(Some(
        serde_json::json!({
            "status": "degraded", "version": "mock-server-version",
            "degraded_reasons": ["subsystem:embedding unavailable"]
        })
        .to_string(),
    ));
    let sandbox = Sandbox::new();
    let run = sandbox.run(
        sandbox.command(&server.url).arg("health"),
        Duration::from_secs(10),
    );
    server.assert_contacted();
    assert!(
        run.stdout.contains("subsystem:embedding unavailable"),
        "{run:?}"
    );
    assert!(run.stdout.contains("mock-server-version"), "{run:?}");
    assert!(run.stdout.contains("degraded"), "{run:?}");
    run
}

#[test]
fn degraded_health_prints_server_reason_and_version() {
    degraded_health();
}

/// Documented contract (docs/OPERATIONS.md, docs/SRS/REQUIREMENTS.md): `xavier health`
/// exits non-zero only when the node is `unhealthy`; `degraded` exits 0 while
/// still surfacing the server's reasons.
#[test]
fn degraded_health_exits_zero_per_documented_contract() {
    let run = degraded_health();
    assert!(
        run.status.success(),
        "degraded health must exit 0 (only unhealthy fails): {run:?}"
    );
}

#[test]
fn unhealthy_health_exits_nonzero() {
    let server = MockServer::new(Some(
        serde_json::json!({
            "status": "unhealthy", "version": "mock-server-version",
            "degraded_reasons": ["subsystem:storage unavailable"]
        })
        .to_string(),
    ));
    let sandbox = Sandbox::new();
    let run = sandbox.run(
        sandbox.command(&server.url).arg("health"),
        Duration::from_secs(10),
    );
    server.assert_contacted();
    assert!(run.stdout.contains("unhealthy"), "{run:?}");
    assert!(
        !run.status.success(),
        "unhealthy health must exit non-zero: {run:?}"
    );
}
