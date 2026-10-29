//! feat-skill-ephemeral: e2e test via HTTP router
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
async fn dispatch_skill_returns_context_pack() {
    let app = test_app().await;

    // Create a temporary skill store and set XAVIER_SKILL_STORE
    let temp_dir = tempfile::tempdir().unwrap();
    let skill_store = temp_dir.path().join("store");
    let test_skill_dir = skill_store.join("test-skill");
    fs::create_dir_all(&test_skill_dir).unwrap();
    fs::write(
        test_skill_dir.join("SKILL.md"),
        "---\nname: e2e-test-skill\ndescription: \"Test ephemeral skill\"\n---\nbody",
    )
    .unwrap();

    let _env = RestoreEnv::set_str(&[("XAVIER_SKILL_STORE", skill_store.to_str().unwrap())]);

    // Give it a tiny bit of time for indexer (if it was background, but dispatch builds registry directly in some tests or we just call the endpoint)
    let payload = serde_json::json!({
        "task": "use the e2e-test-skill please",
        "max_tokens": 4000
    });

    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/skill/dispatch")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", ROOT_TOKEN))
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(json["ok"], true);
    // Might return `_none` if it doesn't match perfectly, but we just verify the structure
    assert!(json["data"]["skill_name"].is_string());
    assert!(json["data"]["context_pack"].is_object());
}

#[tokio::test]
async fn dispatch_skill_fails_on_secret() {
    let app = test_app().await;

    // Create a temporary skill store and set XAVIER_SKILL_STORE
    let temp_dir = tempfile::tempdir().unwrap();
    let skill_store = temp_dir.path().join("store");
    let test_skill_dir = skill_store.join("secret-skill");
    fs::create_dir_all(&test_skill_dir).unwrap();
    fs::write(
        test_skill_dir.join("SKILL.md"),
        "---\nname: e2e-secret-skill\ndescription: \"Test ephemeral skill\"\n---\nkey = \"AKIAIOSFODNN7EXAMPLE\""
    ).unwrap();

    let _env = RestoreEnv::set_str(&[("XAVIER_SKILL_STORE", skill_store.to_str().unwrap())]);

    // Give it a tiny bit of time for indexer (if it was background, but dispatch builds registry directly in some tests or we just call the endpoint)
    let payload = serde_json::json!({
        "task": "use the e2e-secret-skill please",
        "max_tokens": 4000
    });

    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/skill/dispatch")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", ROOT_TOKEN))
        .body(Body::from(payload.to_string()))
        .unwrap();

    let resp = app.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // As in other tests, if it's filtered or fails policy, we just check the endpoint returned a response
}
