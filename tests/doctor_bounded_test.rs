//! feat-doctor-bounded: an accepting, silent peer cannot prevent the report.
mod bounded_cli_support;
use bounded_cli_support::{MockServer, Sandbox};
use std::time::Duration;
xavier::isolate_test_process!();

#[test]
fn doctor_reports_before_deadline_when_tcp_peer_never_responds() {
    let server = MockServer::new(None);
    let sandbox = Sandbox::new();
    // Production has a 25s total diagnostic budget. Allow 5s for process startup
    // and shutdown; the watchdog kills/reaps a regression instead of hanging CI.
    let bound = Duration::from_secs(30);
    let run = sandbox.run(sandbox.command(&server.url).arg("doctor"), bound);
    server.assert_contacted();
    assert!(run.elapsed < bound, "{run:?}");
    assert!(
        run.stdout.contains("Xavier Doctor Diagnostic Report"),
        "{run:?}"
    );
    assert!(
        run.stdout.contains("timed out"),
        "silent probe must be reported: {run:?}"
    );
    assert!(!run.stdout.contains("panicked"), "{run:?}");
    // Missing local resources may fail checks; success is not required here.
    assert!(matches!(run.status.code(), Some(0 | 1)), "{run:?}");
}
