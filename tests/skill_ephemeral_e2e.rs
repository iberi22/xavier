//! feat-skill-ephemeral: e2e test via HTTP router
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

xavier::isolate_test_process!();

#[tokio::test]
#[serial]
async fn dispatch_skill_returns_context_pack() {
    let app = test_app().await;

    // Create a temporary skill store and set XAVIER_SKILL_STORE
    let temp_dir = tempfile::tempdir().unwrap();
    let skill_store = temp_dir.path().join("store");
    let test_skill_dir = skill_store.join("test-skill");
    fs::create_dir_all(&test_skill_dir).unwrap();
    fs::write(
        test_skill_dir.join("SKILL.md"),
        "---\nname: e2e-test-skill\ndescription: \"Test ephemeral skill\"\n---\nbody is composed",
    )
    .unwrap();

    let _env = RestoreEnv::set_str(&[("XAVIER_SKILL_STORE", skill_store.to_str().unwrap())]);

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
    let data = &json["data"];
    assert!(data["skill_name"].is_string());
    // In our hermetic test, the dispatch algorithm should pick the skill or at least
    // return a valid context pack if there are no matches. If matched, the body should
    // be present in system_instructions.
    assert_eq!(
        data["skill_name"].as_str(),
        Some("e2e-test-skill"),
        "Failed to match composed skill"
    );
    let instructions = data["context_pack"]["system_instructions"]
        .as_str()
        .unwrap_or("");
    assert!(
        instructions.contains("body is composed"),
        "skill body missing from context pack"
    );
}

#[tokio::test]
#[serial]
async fn dispatch_skill_fails_on_secret() {
    let app = test_app().await;

    // Create a temporary skill store and set XAVIER_SKILL_STORE
    let temp_dir = tempfile::tempdir().unwrap();
    let skill_store = temp_dir.path().join("store");
    let test_skill_dir = skill_store.join("secret-skill");
    fs::create_dir_all(&test_skill_dir).unwrap();
    let secret_key = format!("AKIA{}EXAMPLE", "IOSFODNN7");
    fs::write(
        test_skill_dir.join("SKILL.md"),
        format!(
            "---\nname: e2e-secret-skill\ndescription: \"Test ephemeral skill\"\n---\nkey = \"{}\"",
            secret_key
        ),
    )
    .unwrap();

    let _env = RestoreEnv::set_str(&[("XAVIER_SKILL_STORE", skill_store.to_str().unwrap())]);

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

    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    // The policy check inside dispatch/compose should block the skill if selected,
    // reflecting an error in the response json.
    assert_eq!(
        json["ok"].as_bool(),
        Some(false),
        "Secret skill was not blocked"
    );
    assert!(json["error"].is_string());
    assert!(
        json["error"].as_str().unwrap().contains("policy")
            || json["error"].as_str().unwrap().contains("secret")
    );
}
