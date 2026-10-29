//! feat-skill-store-config: e2e test via HTTP router
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
async fn custom_skill_store_env_is_respected() {
    let app = test_app().await;

    let temp_dir = tempfile::tempdir().unwrap();
    let skill_store = temp_dir.path().join("my-custom-store");

    let skill_dir = skill_store.join("custom-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: override-skill\ndescription: \"Override\"\n---\nbody",
    )
    .unwrap();

    let _env = RestoreEnv::set_str(&[("XAVIER_SKILL_STORE", skill_store.to_str().unwrap())]);

    let req = Request::builder()
        .method(Method::GET)
        .uri("/api/skill/list")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", ROOT_TOKEN))
        .body(Body::empty())
        .unwrap();

    let resp = app.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(json["skills"].as_array().is_some());
}

#[tokio::test]
async fn custom_skill_store_env_invalid_path() {
    let app = test_app().await;

    let temp_dir = tempfile::tempdir().unwrap();
    let skill_store = temp_dir.path().join("does-not-exist");

    let _env = RestoreEnv::set_str(&[("XAVIER_SKILL_STORE", skill_store.to_str().unwrap())]);

    let req = Request::builder()
        .method(Method::GET)
        .uri("/api/skill/list")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", ROOT_TOKEN))
        .body(Body::empty())
        .unwrap();

    let resp = app.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
