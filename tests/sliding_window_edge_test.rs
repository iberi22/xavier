use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use xavier::security::rate_limiter::{RateLimitConfig, RateLimitResult, SlidingWindowLimiter};

#[test]
fn test_default_config_and_creation() {
    let limiter = SlidingWindowLimiter::with_defaults();
    let cfg = limiter.get_config("tenant_1");
    assert_eq!(cfg.max_requests, 100);
    assert_eq!(cfg.window_duration, Duration::from_secs(60));
    assert_eq!(cfg.burst_capacity, 20);
    assert_eq!(cfg.effective_capacity(), 120);
}

#[test]
fn test_exact_burst_and_capacity_exhaustion() {
    let cfg = RateLimitConfig::new(5, Duration::from_secs(10), 3);
    let limiter = SlidingWindowLimiter::new(cfg);

    // Total effective capacity = 5 + 3 = 8
    for i in 1..=8 {
        let res = limiter.check("user_a");
        assert!(res.allowed, "Request {} should be allowed", i);
        assert_eq!(res.remaining, 8 - i);
        assert_eq!(res.limit, 8);
        assert_eq!(res.retry_after, Duration::ZERO);
    }

    // 9th request must be denied
    let denied = limiter.check("user_a");
    assert!(!denied.allowed);
    assert_eq!(denied.remaining, 0);
    assert_eq!(denied.limit, 8);
    assert!(denied.retry_after > Duration::ZERO);
}

#[test]
fn test_check_n_bulk_requests() {
    let cfg = RateLimitConfig::new(10, Duration::from_secs(10), 0);
    let limiter = SlidingWindowLimiter::new(cfg);

    // Consume 0 requests (probe remaining)
    let res0 = limiter.check_n("user_b", 0);
    assert!(res0.allowed);
    assert_eq!(res0.remaining, 10);

    // Consume 7 requests at once
    let res7 = limiter.check_n("user_b", 7);
    assert!(res7.allowed);
    assert_eq!(res7.remaining, 3);

    // Attempting 4 requests should fail since only 3 are available
    let res4 = limiter.check_n("user_b", 4);
    assert!(!res4.allowed);
    assert_eq!(res4.limit, 10);
    assert!(res4.retry_after > Duration::ZERO);

    // Consuming remaining 3 requests should succeed
    let res3 = limiter.check_n("user_b", 3);
    assert!(res3.allowed);
    assert_eq!(res3.remaining, 0);
}

#[test]
fn test_sliding_window_time_progression() {
    let cfg = RateLimitConfig::new(3, Duration::from_secs(5), 0);
    let limiter = SlidingWindowLimiter::new(cfg);
    let start = Instant::now();

    // Fill capacity at start
    assert!(limiter.check_n_at("user_c", 3, start).allowed);
    assert!(!limiter.check_n_at("user_c", 1, start).allowed);

    // 2 seconds later (within window), should still be blocked
    let t2 = start + Duration::from_secs(2);
    assert!(!limiter.check_n_at("user_c", 1, t2).allowed);

    // 6 seconds later (window expired), all 3 tokens recovered
    let t6 = start + Duration::from_secs(6);
    let res = limiter.check_n_at("user_c", 3, t6);
    assert!(res.allowed);
    assert_eq!(res.remaining, 0);
}

#[test]
fn test_partial_sliding_window_recovery() {
    let cfg = RateLimitConfig::new(3, Duration::from_secs(10), 0);
    let limiter = SlidingWindowLimiter::new(cfg);
    let t0 = Instant::now();

    // Request 1 at t0
    assert!(limiter.check_n_at("user_d", 1, t0).allowed);

    // Request 2 at t0 + 2s
    let t2 = t0 + Duration::from_secs(2);
    assert!(limiter.check_n_at("user_d", 1, t2).allowed);

    // Request 3 at t0 + 4s -> Bucket now full (3/3)
    let t4 = t0 + Duration::from_secs(4);
    assert!(limiter.check_n_at("user_d", 1, t4).allowed);

    // Request 4 at t0 + 5s -> Denied
    let t5 = t0 + Duration::from_secs(5);
    let res_denied = limiter.check_n_at("user_d", 1, t5);
    assert!(!res_denied.allowed);
    // Earliest timestamp t0 expires at t0 + 10s. Difference from t5 is 5s.
    assert_eq!(res_denied.retry_after, Duration::from_secs(5));

    // At t0 + 11s, Request 1 has expired, 1 slot open
    let t11 = t0 + Duration::from_secs(11);
    assert_eq!(limiter.get_usage_at("user_d", t11), 2);
    assert!(limiter.check_n_at("user_d", 1, t11).allowed);
}

#[test]
fn test_zero_capacity_and_zero_window_edge_cases() {
    // Zero capacity
    let zero_cfg = RateLimitConfig::new(0, Duration::from_secs(10), 0);
    let limiter_zero = SlidingWindowLimiter::new(zero_cfg);

    assert!(limiter_zero.check_n("key_zero", 0).allowed);
    let res = limiter_zero.check("key_zero");
    assert!(!res.allowed);
    assert_eq!(res.retry_after, Duration::from_secs(10));

    // Zero window duration
    let zero_win_cfg = RateLimitConfig::new(5, Duration::ZERO, 0);
    let limiter_win_zero = SlidingWindowLimiter::new(zero_win_cfg);

    for _ in 0..10 {
        assert!(limiter_win_zero.check("key_win_zero").allowed);
    }
}

#[test]
fn test_custom_configs_and_multi_key_isolation() {
    let limiter = SlidingWindowLimiter::with_defaults();

    // Custom config for tenant_a
    limiter.set_config(
        "tenant_a",
        RateLimitConfig::new(2, Duration::from_secs(10), 0),
    );

    assert!(limiter.check("tenant_a").allowed);
    assert!(limiter.check("tenant_a").allowed);
    assert!(!limiter.check("tenant_a").allowed);

    // tenant_b uses default config (100 max, 20 burst = 120 total)
    assert!(limiter.check("tenant_b").allowed);
    assert_eq!(limiter.get_usage("tenant_b"), 1);
    assert_eq!(limiter.get_usage("tenant_a"), 2);
}

#[test]
fn test_reset_clear_and_cleanup() {
    let cfg = RateLimitConfig::new(2, Duration::from_secs(10), 0);
    let limiter = SlidingWindowLimiter::new(cfg);
    let now = Instant::now();

    assert!(limiter.check_n_at("k1", 2, now).allowed);
    assert!(limiter.check_n_at("k2", 2, now).allowed);
    assert_eq!(limiter.active_keys_count(), 2);

    // Reset single key
    limiter.reset("k1");
    assert_eq!(limiter.get_usage_at("k1", now), 0);
    assert!(limiter.check_at("k1", now).allowed);

    // Cleanup active keys after expiration
    let future = now + Duration::from_secs(15);
    limiter.cleanup_at(future);
    assert_eq!(limiter.active_keys_count(), 0);

    // Clear all
    limiter.check_at("k3", now);
    assert_eq!(limiter.active_keys_count(), 1);
    limiter.clear();
    assert_eq!(limiter.active_keys_count(), 0);
}

#[test]
fn test_high_concurrency_flood() {
    let cfg = RateLimitConfig::new(500, Duration::from_secs(10), 100); // 600 total
    let limiter = Arc::new(SlidingWindowLimiter::new(cfg));

    let num_threads = 20;
    let requests_per_thread = 50; // Total 1000 requests attempted
    let allowed_count = Arc::new(AtomicUsize::new(0));
    let denied_count = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::new();
    for _ in 0..num_threads {
        let lim = Arc::clone(&limiter);
        let allowed = Arc::clone(&allowed_count);
        let denied = Arc::clone(&denied_count);

        let handle = std::thread::spawn(move || {
            for _ in 0..requests_per_thread {
                let res = lim.check("flood_key");
                if res.allowed {
                    allowed.fetch_add(1, Ordering::Relaxed);
                } else {
                    denied.fetch_add(1, Ordering::Relaxed);
                }
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let total_allowed = allowed_count.load(Ordering::SeqCst);
    let total_denied = denied_count.load(Ordering::SeqCst);

    assert_eq!(total_allowed, 600, "Exact capacity must be granted");
    assert_eq!(total_denied, 400, "Overflow requests must be denied");
    assert_eq!(limiter.get_usage("flood_key"), 600);
}

#[tokio::test]
async fn test_tokio_async_concurrency_flood() {
    let cfg = RateLimitConfig::new(200, Duration::from_secs(5), 50); // 250 capacity
    let limiter = Arc::new(SlidingWindowLimiter::new(cfg));

    let mut tasks = Vec::new();
    let allowed_count = Arc::new(AtomicUsize::new(0));

    for _ in 0..50 {
        let lim = Arc::clone(&limiter);
        let allowed = Arc::clone(&allowed_count);
        tasks.push(tokio::spawn(async move {
            for _ in 0..10 {
                let res = lim.check("async_key");
                if res.allowed {
                    allowed.fetch_add(1, Ordering::Relaxed);
                }
            }
        }));
    }

    for task in tasks {
        task.await.unwrap();
    }

    assert_eq!(
        allowed_count.load(Ordering::SeqCst),
        250,
        "Async tasks must respect exact capacity limit"
    );
}

#[test]
fn test_rate_limit_result_helpers() {
    let allowed = RateLimitResult::allowed(5, 10);
    assert!(allowed.allowed);
    assert_eq!(allowed.remaining, 5);
    assert_eq!(allowed.limit, 10);
    assert_eq!(allowed.retry_after, Duration::ZERO);

    let denied = RateLimitResult::denied(10, Duration::from_secs(3));
    assert!(!denied.allowed);
    assert_eq!(denied.remaining, 0);
    assert_eq!(denied.limit, 10);
    assert_eq!(denied.retry_after, Duration::from_secs(3));
}
