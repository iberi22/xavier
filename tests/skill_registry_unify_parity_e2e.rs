//! <ledger id>: test_build_skill_registry_indexes_every_discovery_root
//! Test skill registry parity between REST listing and MCP context tool.

#[path = "router_harness/mod.rs"]
mod router_harness;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use router_harness::{test_app, RestoreEnv, ROOT_TOKEN};
use serde_json::Value;
use serial_test::serial;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use tower::ServiceExt;

xavier::isolate_test_process!();

fn write_skill(root: &Path, name: &str, description: &str) {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!(
            "---\nname: {name}\ndescription: \"{description}\"\nvisibility: public\n---\n\nbody\n"
        ),
    )
    .unwrap();
}

#[tokio::test]
#[serial]
#[allow(clippy::await_holding_lock)]
async fn test_build_skill_registry_indexes_every_discovery_root() {
    let _lock = crate::SUITE_ENV_LOCK.lock().await;

    let temp_dir = tempfile::tempdir().unwrap();
    let user_store = temp_dir.path().join("user-store");

    // Create skills directory inside XAVIER_DATA_DIR (which acts as workspace dir in harness)
    let workspace_skills = temp_dir.path().join("skills");

    fs::create_dir_all(&user_store).unwrap();
    fs::create_dir_all(&workspace_skills).unwrap();

    // Distinct skills in two discovery roots
    write_skill(&user_store, "user-skill", "Skill in user store");
    write_skill(&workspace_skills, "workspace-skill", "Skill in workspace");

    // The router_harness sets HOME and XAVIER_DATA_DIR to its own temp dir.
    // We append XAVIER_SKILL_STORE via RestoreEnv over the top.
    let _env = RestoreEnv::set_str(&[
        ("XAVIER_SKILL_STORE", user_store.to_str().unwrap()),
        ("XAVIER_DATA_DIR", temp_dir.path().to_str().unwrap()),
    ]);

    let app_fixture = test_app().await;

    // Helper to get names
    fn extract_names(skills: &[Value]) -> BTreeSet<String> {
        skills
            .iter()
            .map(|s| s["name"].as_str().expect("skill name").to_string())
            .collect()
    }

    // 1. REST listing
    let req_rest = Request::builder()
        .uri("/api/skill/list")
        .method(Method::GET)
        .header("Authorization", format!("Bearer {}", ROOT_TOKEN))
        .body(Body::empty())
        .unwrap();

    let resp_rest = app_fixture.app.clone().oneshot(req_rest).await.unwrap();
    assert_eq!(resp_rest.status(), StatusCode::OK);

    let bytes_rest = axum::body::to_bytes(resp_rest.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let rest_body: Value = serde_json::from_slice(&bytes_rest).unwrap();
    let rest_skills = rest_body["skills"].as_array().expect("skills array");
    let rest_names = extract_names(rest_skills);

    // 2. MCP context tool listing via HTTP /mcp
    let mcp_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": "xavier_skill_list",
            "arguments": {}
        },
        "id": 1
    });

    let req_mcp = Request::builder()
        .uri("/mcp")
        .method(Method::POST)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", ROOT_TOKEN))
        .body(Body::from(mcp_payload.to_string()))
        .unwrap();

    let resp_mcp = app_fixture.app.clone().oneshot(req_mcp).await.unwrap();
    assert_eq!(resp_mcp.status(), StatusCode::OK);

    let bytes_mcp = axum::body::to_bytes(resp_mcp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let mcp_body: Value = serde_json::from_slice(&bytes_mcp).unwrap();

    // Extract the json array from the text of the first content block
    let mcp_content_text = mcp_body["result"]["content"][0]["text"]
        .as_str()
        .expect("text");
    let mcp_parsed: Value = serde_json::from_str(mcp_content_text).expect("mcp json");
    let mcp_skills = mcp_parsed["skills"].as_array().expect("mcp skills array");
    let mcp_names = extract_names(mcp_skills);

    // 3. Assertions
    assert_eq!(
        rest_names, mcp_names,
        "REST and MCP skill listings must match exactly"
    );

    assert!(
        rest_names.contains("user-skill"),
        "Must include user store skill"
    );
    assert!(
        rest_names.contains("workspace-skill"),
        "Must include workspace skill"
    );
}
