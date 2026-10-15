//! feat-improve-bounded: a real stalled embedding probe truncates the benchmark.
mod bounded_cli_support;
use bounded_cli_support::{MockServer, Sandbox};
use std::fs;
use std::time::Duration;
use xavier::auto_improvement::cycle::{AutoImprovementEngine, CycleStatus, EXIT_TIMEOUT};
xavier::isolate_test_process!();

#[test]
fn improve_times_out_and_durably_records_partial_progress() {
    let server = MockServer::new(None);
    let sandbox = Sandbox::new();
    let mut command = sandbox.command(&server.url);
    command
        .args(["improve", "run", "--json"])
        .env("XAVIER_IMPROVE_STAGE_TIMEOUT_SECS", "1")
        .env("XAVIER_IMPROVE_TOTAL_TIMEOUT_SECS", "3");
    // The health probe normally allows 2s; a silent endpoint exceeds this
    // stage's 1s deadline. Include startup outside the cycle in the watchdog.
    let run = sandbox.run(&mut command, Duration::from_secs(10));
    server.assert_contacted();
    assert_eq!(run.status.code(), Some(EXIT_TIMEOUT), "{run:?}");
    let cycle: serde_json::Value = serde_json::from_str(&run.stdout).expect("JSON cycle report");
    let status: CycleStatus = serde_json::from_value(cycle["status"].clone()).unwrap();
    assert_eq!(
        status,
        CycleStatus::Truncated {
            stage: "benchmark".into()
        }
    );
    assert_eq!(cycle["completed_stages"], serde_json::json!([]));
    assert!(run.stderr.contains("partial progress persisted"), "{run:?}");

    let history = sandbox.0.path().join(".xavier/improvement-history.json");
    let partial = AutoImprovementEngine::last_partial_progress_from(&history)
        .expect("durable history must parse")
        .expect("truncated progress must exist");
    assert_eq!(partial.truncated_stage, "benchmark");
    assert!(partial.completed_stages.is_empty());
    assert!(partial.gaps.is_empty());
    assert!(partial.experiments.is_empty());
    let entries: serde_json::Value = serde_json::from_slice(&fs::read(history).unwrap()).unwrap();
    assert_eq!(entries.as_array().unwrap().len(), 1);
    assert_eq!(entries[0]["cycle_id"], cycle["cycle_id"]);
    assert_eq!(
        entries[0]["partial"]["completed_stages"],
        cycle["completed_stages"]
    );
}
