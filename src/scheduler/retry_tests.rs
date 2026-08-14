//! Unit tests for scheduler retry policies, exponential backoff, jitter, and circuit breaker functionality.

use super::retry::{CircuitBreaker, RetryPolicy};
use std::time::Duration;

#[test]
fn test_retry_policy_exponential_backoff_calculation() {
    let policy = RetryPolicy::new(
        Duration::from_millis(100),
        Duration::from_secs(10),
        2.0,
        0, // Disable jitter for deterministic math
    );

    assert_eq!(policy.calculate_delay(0), Duration::ZERO);
    assert_eq!(policy.calculate_delay(1), Duration::from_millis(100)); // 100 * 2^0
    assert_eq!(policy.calculate_delay(2), Duration::from_millis(200)); // 100 * 2^1
    assert_eq!(policy.calculate_delay(3), Duration::from_millis(400)); // 100 * 2^2
    assert_eq!(policy.calculate_delay(4), Duration::from_millis(800)); // 100 * 2^3
}

#[test]
fn test_retry_policy_max_delay_cap() {
    let policy = RetryPolicy::new(Duration::from_secs(1), Duration::from_secs(5), 2.0, 0);

    assert_eq!(policy.calculate_delay(1), Duration::from_secs(1));
    assert_eq!(policy.calculate_delay(2), Duration::from_secs(2));
    assert_eq!(policy.calculate_delay(3), Duration::from_secs(4));
    assert_eq!(policy.calculate_delay(4), Duration::from_secs(5)); // Capped at 5s
    assert_eq!(policy.calculate_delay(10), Duration::from_secs(5)); // Capped at 5s
}

#[test]
fn test_retry_policy_includes_jitter_range() {
    let jitter_max_ms = 50;
    let policy = RetryPolicy::new(
        Duration::from_millis(100),
        Duration::from_secs(10),
        2.0,
        jitter_max_ms,
    );

    for _ in 0..20 {
        let delay = policy.calculate_delay(1);
        let base_ms = 100;
        let delay_ms = delay.as_millis() as u64;

        assert!(
            delay_ms >= base_ms && delay_ms <= base_ms + jitter_max_ms,
            "Delay {} ms was out of expected range [{}, {}]",
            delay_ms,
            base_ms,
            base_ms + jitter_max_ms
        );
    }
}

#[test]
fn test_circuit_breaker_transitions_and_cooldown() {
    let failure_threshold = 3;
    let cooldown = Duration::from_millis(200);
    let mut cb = CircuitBreaker::new(failure_threshold, cooldown);

    assert_eq!(cb.state_name(), "closed");
    assert!(cb.can_execute());
    assert!(!cb.is_open());

    // First failure
    assert!(!cb.record_failure());
    assert_eq!(cb.state_name(), "closed");
    assert!(cb.can_execute());

    // Second failure
    assert!(!cb.record_failure());
    assert_eq!(cb.state_name(), "closed");

    // Third failure -> trips open
    let tripped = cb.record_failure();
    assert!(tripped, "Circuit breaker should return true when tripped");
    assert_eq!(cb.state_name(), "open");
    assert!(cb.is_open());
    assert!(!cb.can_execute());

    // Wait for cooldown window to expire
    std::thread::sleep(Duration::from_millis(220));

    // Circuit enters half-open / allows probe execution
    assert_eq!(cb.state_name(), "half-open");
    assert!(cb.can_execute());
    assert!(!cb.is_open());

    // Recording success resets circuit breaker back to closed
    cb.record_success();
    assert_eq!(cb.state_name(), "closed");
    assert_eq!(cb.consecutive_failures, 0);
    assert!(!cb.is_open());
}

#[test]
fn test_circuit_breaker_record_success_resets_failure_count() {
    let mut cb = CircuitBreaker::new(3, Duration::from_secs(10));

    cb.record_failure();
    cb.record_failure();
    assert_eq!(cb.consecutive_failures, 2);

    cb.record_success();
    assert_eq!(cb.consecutive_failures, 0);

    // Requires 3 full failures again to trip
    cb.record_failure();
    cb.record_failure();
    assert_eq!(cb.state_name(), "closed");

    assert!(cb.record_failure());
    assert_eq!(cb.state_name(), "open");
}
