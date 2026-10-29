//! feat-skill-drift-audit: e2e test via HTTP router
#[path = "router_harness/mod.rs"]
mod router_harness;
use axum::body::Body;
use axum::http::{Method, Request};
use router_harness::{status, test_app, RestoreEnv, ROOT_TOKEN};
use serial_test::serial;
use std::fs;
use tower::ServiceExt;

#[tokio::test]
#[serial]
async fn audit_endpoint_returns_json_report() {
    let temp_dir = tempfile::tempdir().unwrap();
    let sources_dir = temp_dir.path().join("sources");
    let targets_dir = temp_dir.path().join("targets");
    let package_dir = sources_dir.join("demo");
    let journal_dir = temp_dir.path().join("journal");
    fs::create_dir_all(&package_dir).unwrap();
    fs::create_dir_all(&targets_dir).unwrap();

    let skill_md_path = package_dir.join("SKILL.md");
    fs::write(
        &skill_md_path,
        "---
name: demo
---
body",
    )
    .unwrap();

    let manifest_path = temp_dir.path().join("manifest.toml");
    let manifest_content = format!(
        "version = 1

[sources]
test_source = \"{}\"

[[targets]]
id = \"test_target\"
root = \"{}\"
tools = [\"codex\"]
mode = \"symlink\"

[[placements]]
source = \"test_source\"
targets = [\"test_target\"]
path = \"demo\"
name = \"demo\"
",
        sources_dir.display(),
        targets_dir.display()
    );
    fs::write(&manifest_path, &manifest_content).unwrap();

    // Journal baseline hash
    let journal = xavier::context::skill_controller::state::Journal::new(journal_dir).unwrap();
    journal
        .begin_transaction("projection", vec![], false)
        .unwrap();
    use xavier::context::skill_controller::plan::hash_package;
    let manifest =
        xavier::context::skill_controller::manifest::SkillManifest::from_toml(&manifest_content)
            .unwrap();
    let hash = hash_package(&package_dir, &manifest.placements[0])
        .unwrap()
        .hash;
    let entry = xavier::context::skill_controller::state::JournalEntry {
        action: xavier::context::skill_controller::state::Action::Symlink,
        source: package_dir.clone(),
        target: targets_dir.join("demo"),
        recorded_hash: Some(hash),
        placement: Some(manifest.placements[0].clone()),
    };
    journal.commit(&[entry]).unwrap();

    let _env = RestoreEnv::set_str(&[("XAVIER_SKILL_MANIFEST", manifest_path.to_str().unwrap())]);
    let app = test_app().await;

    // Induce drift and seed a synthetic secret
    let secret = format!("AKIA{}EXAMPLE", "IOSFODNN7");
    fs::write(
        &skill_md_path,
        format!("---\nname: demo\n---\nbody updated\nkey = \"{}\"", secret),
    )
    .unwrap();

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

    let findings = json["findings"].as_array().expect("findings");
    assert!(
        findings
            .iter()
            .any(|f| f["rule"] == "skill.projection.hash_drift"),
        "Expected hash_drift rule to fire"
    );

    // Assert secret detection and redaction
    assert!(
        findings.iter().any(|f| f["rule"] == "skill.code.secret"),
        "Expected secret detection rule to fire"
    );
    let response_text = json.to_string();
    assert!(
        !response_text.contains(&secret),
        "Secret must be redacted from response"
    );
}

#[tokio::test]
#[serial]
async fn audit_endpoint_returns_not_found_when_manifest_missing() {
    let _env = RestoreEnv::set_str(&[("XAVIER_SKILL_MANIFEST", "missing")]);
    let app = test_app().await;
    let st = status(&app.app, Method::GET, "/api/skill/audit", Some(ROOT_TOKEN)).await;
    assert_eq!(st, axum::http::StatusCode::UNPROCESSABLE_ENTITY); // 422
}
