//! feat-skill-drift-audit: e2e test via HTTP router
#[path = "router_harness/mod.rs"]
mod router_harness;
use axum::body::Body;
use axum::http::{Method, Request};
use router_harness::{status, test_app, RestoreEnv, ROOT_TOKEN};
use serial_test::serial;
use std::fs;
use tower::ServiceExt;

xavier::isolate_test_process!();

#[tokio::test]
#[serial]
async fn audit_endpoint_returns_json_report() {
    let app = test_app().await;
    let temp_dir = tempfile::tempdir().unwrap();
    let sources_dir = temp_dir.path().join("sources");
    let targets_dir = temp_dir.path().join("targets");
    let package_dir = sources_dir.join("demo");
    fs::create_dir_all(&package_dir).unwrap();
    fs::create_dir_all(&targets_dir).unwrap();

    let skill_md_path = package_dir.join("SKILL.md");
    fs::write(&skill_md_path, "---\nname: demo\n---\nbody").unwrap();

    let manifest_path = temp_dir.path().join("manifest.toml");
    let manifest_content = format!(
        "version = 1\n\n[sources]\ntest_source = \"{}\"\n\n[[targets]]\nid = \"test_target\"\nroot = \"{}\"\ntools = [\"codex\"]\nmode = \"symlink\"\n\n[[placements]]\nsource = \"test_source\"\ntargets = [\"test_target\"]\npath = \"demo\"\nname = \"demo\"\n",
        sources_dir.display(),
        targets_dir.display()
    );
    fs::write(&manifest_path, &manifest_content).unwrap();

    let _env = RestoreEnv::set_str(&[("XAVIER_SKILL_MANIFEST", manifest_path.to_str().unwrap())]);

    // Induce drift
    fs::write(&skill_md_path, "---\nname: demo\n---\nbody updated").unwrap();

    let req = Request::builder()
        .method(Method::GET)
        .uri("/api/skill/audit")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", ROOT_TOKEN))
        .body(Body::empty())
        .unwrap();

    let resp = app.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::OK);

    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(json["findings"].as_array().is_some());
    // Check that drift or missing description is reported
    assert!(!json["findings"].as_array().unwrap().is_empty());
}

#[tokio::test]
#[serial]
async fn audit_endpoint_returns_not_found_when_manifest_missing() {
    let app = test_app().await;
    let _env = RestoreEnv::set_str(&[("XAVIER_SKILL_MANIFEST", "missing")]);
    let st = status(&app.app, Method::GET, "/api/skill/audit", Some(ROOT_TOKEN)).await;
    assert_eq!(st, axum::http::StatusCode::UNPROCESSABLE_ENTITY); // 422
}
