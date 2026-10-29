//! feat-health-truth: Health report does not fabricate a healthy state when subsystems are unmeasured.
#![allow(unused_imports)]

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use serde_json::Value;
use std::fs;
use tower::ServiceExt;

#[path = "router_harness/mod.rs"]
mod router_harness;

xavier::isolate_test_process!();

#[tokio::test]
async fn unmeasured_subsystems_report_unknown_or_degraded_not_healthy() {
    // router_harness creates a clean TempDir for data, meaning no embeddings or stores are measured yet.
    let app = router_harness::test_app().await;

    let req = Request::builder()
        .method(Method::GET)
        .uri("/health")
        .body(Body::empty())
        .unwrap();

    let response = app.router.oneshot(req).await.unwrap();
    assert!(
        response.status().is_success(),
        "GET /health should succeed even if degraded/unknown"
    );

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let health: Value = serde_json::from_slice(&bytes).expect("parse health JSON");

    let status = health["status"].as_str().unwrap_or("missing");
    assert!(
        status == "unknown" || status == "degraded",
        "health status in unmeasured environment must be unknown or degraded, but was: {}",
        status
    );

    // Verify degraded_reasons contains reasons and is not empty
    let reasons = health["degraded_reasons"]
        .as_array()
        .expect("degraded_reasons must be an array");
    assert!(
        !reasons.is_empty(),
        "degraded_reasons must not be empty when not healthy"
    );

    // HealthResponse.degraded_reasons tags host:<check> vs subsystem:<check>
    let has_subsystem_or_host_tags = reasons.iter().any(|r| {
        let text = r.as_str().unwrap_or("");
        text.contains("subsystem:") || text.contains("host:")
    });
    assert!(
        has_subsystem_or_host_tags,
        "degraded_reasons must tag host:<check> vs subsystem:<check>"
    );
}
