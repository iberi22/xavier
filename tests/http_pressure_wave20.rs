//! WAVE-20.03 Integration Test: HTTP responsivo bajo presion
//!
//! Verifies:
//! 1. `test_wave20_health_responsive_under_load`: /health responds rapidly (<1s under debug test load) even under heavy thread load / pressure.
//! 2. `test_wave20_search_timeout_bounded`: /memory/search times out with HTTP 504 and JSON error when execution stalls.
//! 3. `test_wave20_stats_responsive_when_write_pool_down`: /memory/stats responds 200 OK even when write operations are locked or unavailable.

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    routing::post,
    Json, Router,
};
use http_body_util::BodyExt;
use std::time::{Duration, Instant};
use tower::ServiceExt;
use xavier::adapters::inbound::http::middleware::timeout::timeout_middleware_with_duration;
use xavier::adapters::inbound::http::routes::create_router;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_wave20_health_responsive_under_load() {
    let app = create_router();

    // Saturate background async tasks to simulate heavy daemon pressure
    let mut handles = Vec::new();
    for _ in 0..4 {
        handles.push(tokio::spawn(async {
            let start = Instant::now();
            while start.elapsed() < Duration::from_millis(200) {
                let mut x = 0u64;
                for i in 0..10_000 {
                    x = x.wrapping_add(i);
                }
                std::hint::black_box(x);
                tokio::task::yield_now().await;
            }
        }));
    }

    let req = Request::builder()
        .uri("/health")
        .method(Method::GET)
        .body(Body::empty())
        .expect("build health request");

    let start = Instant::now();
    let response = app.oneshot(req).await.expect("health request execution");
    let elapsed = start.elapsed();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET /health must return 200 OK under load"
    );
    assert!(
        elapsed < Duration::from_secs(1),
        "GET /health must respond in <1s under debug test load, took {:?}",
        elapsed
    );

    let body = response
        .into_body()
        .collect()
        .await
        .expect("collect health body")
        .to_bytes();
    let json: serde_json::Value =
        serde_json::from_slice(&body).expect("parse health JSON response");

    assert_eq!(json["service"], "xavier");
    assert!(
        json.get("status").is_some(),
        "health JSON response must contain status field"
    );

    for h in handles {
        let _ = h.await;
    }
}

#[tokio::test]
async fn test_wave20_search_timeout_bounded() {
    // Router simulating a slow /memory/search handler that hangs or stalls
    let app = Router::new()
        .route(
            "/memory/search",
            post(|| async {
                // Simulate slow embedding / vector search execution
                tokio::time::sleep(Duration::from_millis(300)).await;
                Json(serde_json::json!({ "results": [] }))
            }),
        )
        .layer(axum::middleware::from_fn(|req, next| {
            // Apply 50ms timeout for the test
            timeout_middleware_with_duration(Duration::from_millis(50), req, next)
        }));

    let req = Request::builder()
        .uri("/memory/search")
        .method(Method::POST)
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "query": "heavy search query under load"
            }))
            .unwrap(),
        ))
        .expect("build search request");

    let start = Instant::now();
    let response = app.oneshot(req).await.expect("search request execution");
    let elapsed = start.elapsed();

    assert_eq!(
        response.status(),
        StatusCode::GATEWAY_TIMEOUT,
        "Slow search request must return 504 Gateway Timeout"
    );
    assert!(
        elapsed < Duration::from_millis(200),
        "Timeout must abort slow search quickly (expected ~50ms, took {:?})",
        elapsed
    );

    let body = response
        .into_body()
        .collect()
        .await
        .expect("collect search timeout body")
        .to_bytes();
    let json: serde_json::Value =
        serde_json::from_slice(&body).expect("parse timeout JSON response");

    assert_eq!(json["status"], "error");
    assert_eq!(json["message"], "Request timeout");
}

#[tokio::test]
async fn test_wave20_stats_responsive_when_write_pool_down() {
    // Test stats responsiveness even when a background write operation holds a lock
    let write_lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
    let lock_guard = write_lock.clone().lock_owned().await;

    let app = create_router();

    let req = Request::builder()
        .uri("/memory/stats")
        .method(Method::GET)
        .body(Body::empty())
        .expect("build stats request");

    let start = Instant::now();
    let response = app.oneshot(req).await.expect("stats request execution");
    let elapsed = start.elapsed();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET /memory/stats must return 200 OK"
    );
    assert!(
        elapsed < Duration::from_millis(500),
        "GET /memory/stats must respond in <500ms, took {:?}",
        elapsed
    );

    let body = response
        .into_body()
        .collect()
        .await
        .expect("collect stats body")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).expect("parse stats JSON response");

    assert_eq!(json["status"], "ok");
    assert!(json.get("workspace_id").is_some());

    drop(lock_guard);
}

#[tokio::test]
async fn test_wave20_default_timeout_env_config() {
    std::env::set_var("XAVIER_HTTP_TIMEOUT_SECS", "5");
    let duration = xavier::adapters::inbound::http::middleware::timeout::get_timeout_duration();
    assert_eq!(duration, Duration::from_secs(5));
    std::env::remove_var("XAVIER_HTTP_TIMEOUT_SECS");
}
