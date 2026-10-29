//! <ledger id>: test_build_skill_registry_indexes_every_discovery_root
//! Test skill registry parity between REST listing and MCP context tool.

#[path = "router_harness/mod.rs"]
mod router_harness;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use router_harness::{create_test_cli_state, RestoreEnv, ROOT_TOKEN};
use serde_json::Value;
use serial_test::serial;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;
use xavier::cli::server::{build_app, AppDeps};
use xavier::workspace::{WorkspaceContext, WorkspaceState};

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

struct ParityApp {
    app: Router,
    _env: RestoreEnv,
    _temp: TempDir,
}

async fn parity_app() -> ParityApp {
    let temp = tempfile::tempdir().expect("tempdir");
    let user_store = temp.path().join("user-store");
    let workspace_skills = temp.path().join("skills");

    fs::create_dir_all(&user_store).unwrap();
    fs::create_dir_all(&workspace_skills).unwrap();

    // Distinct skills in two discovery roots
    write_skill(&user_store, "user-skill", "Skill in user store");
    write_skill(&workspace_skills, "workspace-skill", "Skill in workspace");

    let env = RestoreEnv::set_str(&[
        ("HOME", temp.path().to_str().unwrap()),
        ("XAVIER_CONFIG_PATH", temp.path().to_str().unwrap()),
        ("XAVIER_RUNTIME_STATE_PATH", temp.path().to_str().unwrap()),
        ("XAVIER_DATA_DIR", temp.path().to_str().unwrap()),
        ("XAVIER_SKILL_STORE", user_store.to_str().unwrap()),
        ("XAVIER_TOKEN", ROOT_TOKEN),
        ("XAVIER_JWT_SECRET", "router-harness-jwt-secret"),
    ]);

    let mut state = create_test_cli_state(&temp).await;
    // VERY IMPORTANT: test_app() hardcodes "test-ws" workspace_id which causes workspace paths
    // to resolve out of bounds instead of within XAVIER_DATA_DIR. We override it to the temp dir.
    state.workspace_id = temp.path().to_string_lossy().into_owned();
    state.workspace_dir = temp.path().to_path_buf();

    let workspace_state = Arc::new(
        WorkspaceState::new_minimal(
            state.workspace_id.clone(),
            state.workspace_dir.clone(),
            Arc::clone(&state.qmd_memory),
            Arc::clone(&state.store),
        )
        .await,
    );
    let deps = AppDeps {
        workspace_ctx: WorkspaceContext {
            workspace_id: state.workspace_id.clone(),
            workspace: workspace_state,
        },
        space_manager: None,
        maloca_router: Router::new(),
        event_bus_for_ws: xavier::coordination::XavierEventBus::new(10),
        no_ui: true,
    };
    ParityApp {
        app: build_app(state, deps),
        _env: env,
        _temp: temp,
    }
}

#[tokio::test]
#[serial]
#[allow(clippy::await_holding_lock)]
async fn test_build_skill_registry_indexes_every_discovery_root() {
    let _lock = crate::SUITE_ENV_LOCK.lock().await;

    let app_fixture = parity_app().await;

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

    // 2. MCP context tool listing via HTTP /mcp/tools/call
    let mcp_payload = serde_json::json!({
        "name": "xavier_skill_list",
        "arguments": {}
    });

    let req_mcp = Request::builder()
        .uri("/mcp/tools/call")
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
    let mcp_content_text = mcp_body["content"][0]["text"].as_str().expect("text");
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
