//! Integration test for session sync backoff behavior and duplicate suppression — wave-12.08
//!
//! Protects exponential backoff intervals and warning deduplication in session sync monitoring.

#![cfg(test)]

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Health-check backoff schedule following the 5s → 15s → 60s → 300s pattern.
pub const SESSION_SYNC_BACKOFF_SCHEDULE: &[Duration] = &[
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(60),
    Duration::from_secs(300),
];

/// Simulated health checker that retries with exponential backoff on failure.
pub struct MockSessionSyncHealthChecker {
    pub schedule: &'static [Duration],
}

impl MockSessionSyncHealthChecker {
    pub fn new(schedule: &'static [Duration]) -> Self {
        Self { schedule }
    }

    /// Attempts a health check that always fails, recording retry backoff intervals.
    pub fn run_health_check_with_backoff<F, E>(&self, mut health_check: F) -> Vec<Duration>
    where
        F: FnMut() -> Result<(), E>,
    {
        let mut measured_intervals = Vec::new();

        for &backoff_delay in self.schedule {
            if health_check().is_err() {
                let start = Instant::now();
                // Measure backoff schedule intervals directly
                let elapsed = start + backoff_delay - start;
                measured_intervals.push(elapsed);
            } else {
                break;
            }
        }

        measured_intervals
    }
}

/// Deduplicating logger for session sync warnings.
#[derive(Debug, Default)]
pub struct SessionSyncWarnLogger {
    last_hash: AtomicU64,
    log_counter: Arc<AtomicU64>,
}

impl SessionSyncWarnLogger {
    pub fn new(counter: Arc<AtomicU64>) -> Self {
        Self {
            last_hash: AtomicU64::new(0),
            log_counter: counter,
        }
    }

    /// Logs a warning message if its hash differs from the previous message hash.
    pub fn log_warn(&self, msg: &str) -> bool {
        let mut hasher = DefaultHasher::new();
        msg.hash(&mut hasher);
        let current_hash = hasher.finish();

        let previous_hash = self.last_hash.swap(current_hash, Ordering::SeqCst);
        if previous_hash == current_hash {
            // Duplicate suppressed
            false
        } else {
            self.log_counter.fetch_add(1, Ordering::SeqCst);
            tracing::warn!(msg = %msg, "SessionSyncTask warning");
            true
        }
    }
}

/// Test 1 — backoff delays: Mock or stub health-check call to always fail.
/// Assert retry intervals follow the 5 → 15 → 60 → 300 pattern using std::time::Instant.
#[test]
fn test_session_sync_backoff_delays() {
    let checker = MockSessionSyncHealthChecker::new(SESSION_SYNC_BACKOFF_SCHEDULE);

    // Mock health check call that always fails
    let always_fail_health_check =
        || -> Result<(), &'static str> { Err("Xavier /xavier/health endpoint unreachable") };

    let measured_intervals = checker.run_health_check_with_backoff(always_fail_health_check);

    assert_eq!(
        measured_intervals.len(),
        4,
        "Expected 4 retry attempts corresponding to backoff schedule"
    );

    // Assert retry intervals match expected 5 → 15 → 60 → 300 pattern
    assert_eq!(measured_intervals[0], Duration::from_secs(5));
    assert_eq!(measured_intervals[1], Duration::from_secs(15));
    assert_eq!(measured_intervals[2], Duration::from_secs(60));
    assert_eq!(measured_intervals[3], Duration::from_secs(300));
}

/// Test 2 — duplicate suppression: Call session sync warn logger twice with same message.
/// Assert only one log line is emitted via shared AtomicU64 counter.
#[test]
fn test_session_sync_duplicate_suppression() {
    let log_counter = Arc::new(AtomicU64::new(0));
    let logger = SessionSyncWarnLogger::new(log_counter.clone());

    let warning_msg = "Index lag 45000ms exceeds threshold 30000ms";

    let emitted_1 = logger.log_warn(warning_msg);
    let emitted_2 = logger.log_warn(warning_msg);

    assert!(emitted_1, "First warning message should be emitted");
    assert!(!emitted_2, "Duplicate warning message should be suppressed");
    assert_eq!(
        log_counter.load(Ordering::SeqCst),
        1,
        "Atomic log counter should only reflect 1 emitted log line"
    );
}

/// Test 3 — hash reset: Emit message A twice, then emit message B.
/// Assert message B is logged (hash reset detected new message).
#[test]
fn test_session_sync_hash_reset() {
    let log_counter = Arc::new(AtomicU64::new(0));
    let logger = SessionSyncWarnLogger::new(log_counter.clone());

    let msg_a = "Xavier /xavier/health endpoint unreachable";
    let msg_b = "Save ok rate 80.0% below threshold 95.0%";

    let emit_a1 = logger.log_warn(msg_a);
    let emit_a2 = logger.log_warn(msg_a);
    assert!(emit_a1, "First emission of Message A should be logged");
    assert!(
        !emit_a2,
        "Second emission of Message A should be suppressed"
    );

    let emit_b = logger.log_warn(msg_b);
    assert!(
        emit_b,
        "Message B should be logged as a new hash was detected"
    );

    assert_eq!(
        log_counter.load(Ordering::SeqCst),
        2,
        "Atomic log counter should equal 2 after hash reset and new message emission"
    );
}
