use std::sync::Arc;
use std::thread;
use std::time::Duration;
use xavier::security::sliding_limiter::{LockFreeSlidingLimiter, SlidingLimiterConfig};

#[test]
fn test_lock_free_sliding_limiter_high_concurrency() {
    let config = SlidingLimiterConfig {
        max_requests: 50,
        window_duration: Duration::from_secs(5),
        num_buckets: 10,
    };
    let limiter = Arc::new(LockFreeSlidingLimiter::new(config));

    let mut handles = Vec::new();
    let num_threads = 100;

    for _ in 0..num_threads {
        let limiter_clone = Arc::clone(&limiter);
        let handle = thread::spawn(move || limiter_clone.check("concurrent_agent"));
        handles.push(handle);
    }

    let mut allowed_count = 0;
    let mut rejected_count = 0;

    for handle in handles {
        if handle.join().unwrap() {
            allowed_count += 1;
        } else {
            rejected_count += 1;
        }
    }

    // Exactly 50 requests should have been allowed, and 50 rejected
    assert_eq!(
        allowed_count, 50,
        "Expected exactly 50 allowed requests under concurrent load"
    );
    assert_eq!(
        rejected_count, 50,
        "Expected exactly 50 rejected requests under concurrent load"
    );
}
