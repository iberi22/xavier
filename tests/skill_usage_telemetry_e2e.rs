//! feat-skill-usage-telemetry: e2e test via HTTP router
#[path = "router_harness/mod.rs"]
mod router_harness;
use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use router_harness::{test_app, RestoreEnv, ROOT_TOKEN};
use std::fs;
use tower::ServiceExt;

xavier::isolate_test_process!();

#[tokio::test]
async fn telemetry_dispatch_ack_and_usage_e2e() {
    let app = test_app().await;

    let temp_dir = tempfile::tempdir().unwrap();
    let skill_store = temp_dir.path().join("store");

    let skill_dir = skill_store.join("audit-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: trace-audit-e2e\ndescription: \"Audit the failed traces of a run\"\n---\nbody",
    )
    .unwrap();

    let _env = RestoreEnv::set_str(&[("XAVIER_SKILL_STORE", skill_store.to_str().unwrap())]);

    // 1. Dispatch
    let dispatch_payload = serde_json::json!({
        "task": "audit the failed traces",
        "max_tokens": 4000
    });
    let req1 = Request::builder()
        .method(Method::POST)
        .uri("/api/skill/dispatch")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", ROOT_TOKEN))
        .body(Body::from(dispatch_payload.to_string()))
        .unwrap();

    let resp1 = app.app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);

    let body_bytes1 = axum::body::to_bytes(resp1.into_body(), usize::MAX)
        .await
        .unwrap();
    let json1: serde_json::Value = serde_json::from_slice(&body_bytes1).unwrap();
    let dispatch_id = json1["data"]["dispatch_id"]
        .as_str()
        .unwrap_or("none")
        .to_string();

    // 2. Ack
    let ack_payload = serde_json::json!({
        "dispatch_id": dispatch_id,
        "skill_name": "trace-audit-e2e",
        "stage": "invoked",
        "tool": "http-test"
    });
    let req2 = Request::builder()
        .method(Method::POST)
        .uri("/api/skill/ack")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", ROOT_TOKEN))
        .body(Body::from(ack_payload.to_string()))
        .unwrap();

    let resp2 = app.app.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);

    // 3. Usage
    let req3 = Request::builder()
        .method(Method::GET)
        .uri("/api/skill/usage?workspace=test-ws")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", ROOT_TOKEN))
        .body(Body::empty())
        .unwrap();
    let resp3 = app.app.clone().oneshot(req3).await.unwrap();
    assert_eq!(resp3.status(), StatusCode::OK);
}

#[tokio::test]
async fn usage_endpoint_unauthorized() {
    let app = test_app().await;
    let req = Request::builder()
        .method(Method::GET)
        .uri("/api/skill/usage?workspace=test-ws")
        .header("Content-Type", "application/json")
        .body(Body::empty())
        .unwrap();
    let resp = app.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
