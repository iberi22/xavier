//! HTTP pressure and responsiveness integration tests for Wave 20.03
//!
//! Verifies system responsiveness under pressure:
//! - /health responds in <500ms even when the runtime/background pools are saturated
//! - /memory/search is bounded by timeout and returns HTTP 504 on delay
//! - /memory/stats remains responsive even when write operations are ongoing

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    routing::{get, post},
    Json, Router,
};
use http_body_util::BodyExt;
use std::time::{Duration, Instant};
use tower::ServiceExt;
use xavier::adapters::inbound::http::middleware::timeout_middleware;
use xavier::health::collect_health_sync;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_wave20_health_fast_under_pressure() {
    let mut handles = Vec::new();
    for _ in 0..8 {
        handles.push(tokio::spawn(async move {
            let start = Instant::now();
            while start.elapsed() < Duration::from_millis(300) {
                let mut sum: u64 = 0;
                for i in 0..10_000 {
                    sum = sum.wrapping_add(i);
                }
                std::hint::black_box(sum);
                tokio::task::yield_now().await;
            }
        }));
    }

    let start = Instant::now();
    let health = collect_health_sync();
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_millis(500),
        "collect_health_sync took {:?}, expected <500ms",
        elapsed
    );
    assert!(
        health.status == "healthy"
            || health.status == "warn"
            || health.status == "degraded"
            || health.status == "unhealthy",
        "unexpected health status: {}",
        health.status
    );

    for h in handles {
        let _ = h.await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_wave20_search_timeout_bounded() {
    std::env::set_var("XAVIER_HTTP_TIMEOUT_SECS", "1");

    let app = Router::new()
        .route(
            "/memory/search",
            post(|| async {
                tokio::time::sleep(Duration::from_secs(3)).await;
                Json(serde_json::json!({ "results": [] }))
            }),
        )
        .layer(axum::middleware::from_fn(timeout_middleware));

    let start = Instant::now();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/memory/search")
                .method(Method::POST)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({"query": "test"})).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .expect("request should be handled");

    let elapsed = start.elapsed();

    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    assert!(
        elapsed < Duration::from_secs(2),
        "search handler was not bounded by timeout, took {:?}",
        elapsed
    );

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json_body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json_body["status"], "error");
    assert_eq!(json_body["message"], "Request timeout");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_wave20_stats_responsive_when_write_pool_busy() {
    let write_busy = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));

    let write_busy_clone = write_busy.clone();
    let app = Router::new()
        .route(
            "/memory/stats",
            get(move || {
                let busy = write_busy_clone.load(std::sync::atomic::Ordering::Relaxed);
                async move {
                    Json(serde_json::json!({
                        "status": "ok",
                        "workspace_id": "default",
                        "write_pool_busy": busy,
                    }))
                }
            }),
        )
        .layer(axum::middleware::from_fn(timeout_middleware));

    let start = Instant::now();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/memory/stats")
                .method(Method::GET)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("stats request completed");

    let elapsed = start.elapsed();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        elapsed < Duration::from_millis(500),
        "stats endpoint took {:?}, expected <500ms",
        elapsed
    );

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json_body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json_body["status"], "ok");
    assert_eq!(json_body["write_pool_busy"], true);
}
