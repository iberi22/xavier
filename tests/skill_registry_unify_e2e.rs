//! feat-skill-registry-unify: e2e test via HTTP router
#[path = "router_harness/mod.rs"]
mod router_harness;
use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use router_harness::{test_app, RestoreEnv, ROOT_TOKEN};
use serial_test::serial;
use std::fs;
use tower::ServiceExt;

#[tokio::test]
#[serial]
#[allow(clippy::await_holding_lock)]
async fn list_skills_returns_unified_registry_results() {
    let _lock = crate::SUITE_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let temp_dir = tempfile::tempdir().unwrap();
    let skill_store = temp_dir.path().join("store");

    // Create skill A
    let skill_a_dir = skill_store.join("skill-a");
    fs::create_dir_all(&skill_a_dir).unwrap();
    fs::write(
        skill_a_dir.join("SKILL.md"),
        "---\nname: alpha-unify-e2e\ndescription: \"Alpha\"\n---\nbody",
    )
    .unwrap();

    let _env = RestoreEnv::set_str(&[("XAVIER_SKILL_STORE", skill_store.to_str().unwrap())]);

    let app = test_app().await;

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

    let skills = json["skills"].as_array().expect("skills array");
    let has_alpha = skills.iter().any(|s| s["name"] == "alpha-unify-e2e");
    // Since skills are indexed lazily or synchronously depending on the route logic,
    // we just check that the endpoint returns the list and hasn't failed.
    // In our hermetic test, if the store is read, it should contain the skill.
    assert!(
        has_alpha,
        "skill 'alpha-unify-e2e' not found in registry: {:?}",
        skills
    );
}

#[tokio::test]
#[serial]
#[allow(clippy::await_holding_lock)]
async fn list_skills_returns_unauthorized_without_token() {
    let _lock = crate::SUITE_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let app = test_app().await;

    let req = Request::builder()
        .method(Method::GET)
        .uri("/api/skill/list")
        .header("Content-Type", "application/json")
        .body(Body::empty())
        .unwrap();

    let resp = app.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
