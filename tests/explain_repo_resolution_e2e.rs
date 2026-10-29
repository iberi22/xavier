//! fix-explain-repo-resolution: e2e tests

xavier::isolate_test_process!();

mod router_harness;

use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    http::{Method, Request, StatusCode},
    Router,
};
use router_harness::{create_test_cli_state, RestoreEnv, JWT_SECRET, ROOT_TOKEN};
use serde_json::Value;
use tempfile::TempDir;
use tower::ServiceExt;
use xavier::cli::server::{build_app, AppDeps};
use xavier::workspace::{WorkspaceContext, WorkspaceState};

struct App {
    app: Router,
    _envs: Vec<RestoreEnv>,
}

async fn explain_app_with_nested_repo(temp: &TempDir) -> App {
    let repo_dir = temp.path().join("nested").join("repo");
    std::fs::create_dir_all(repo_dir.join("src")).unwrap();
    std::fs::write(repo_dir.join("src/lib.rs"), "fn nested() {}\n").unwrap();

    let mut cmd = std::process::Command::new("git");
    xavier::explain::history::clear_git_env(&mut cmd);
    let status = cmd
        .current_dir(&repo_dir)
        .args(["init", "-q"])
        .status()
        .unwrap();
    assert!(status.success());

    let envs = vec![
        RestoreEnv::set(&[
            ("HOME", temp.path()),
            ("XAVIER_DATA_DIR", temp.path()),
            ("XAVIER_CONFIG_PATH", temp.path()),
            ("XAVIER_RUNTIME_STATE_PATH", temp.path()),
        ]),
        RestoreEnv::set_str(&[
            ("XAVIER_TOKEN", ROOT_TOKEN),
            ("XAVIER_JWT_SECRET", JWT_SECRET),
            ("XAVIER_MODEL_PROVIDER", "disabled"),
        ]),
    ];

    let state = create_test_cli_state(temp).await;
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
    App {
        app: build_app(state, deps),
        _envs: envs,
    }
}

async fn post_explain(app: &Router, bearer: Option<&str>, body: &str) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/v1/explain")
        .header("Content-Type", "application/json");
    if let Some(token) = bearer {
        req = req.header("Authorization", format!("Bearer {token}"));
    }
    let resp = app
        .clone()
        .oneshot(req.body(Body::from(body.to_string())).expect("request"))
        .await
        .expect("response");
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("response body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn repo_resolution_finds_nested_repo() {
    let temp = tempfile::tempdir().expect("tempdir");
    let t = explain_app_with_nested_repo(&temp).await;

    let body = r#"{"file":"nested/repo/src/lib.rs","start_line":1,"end_line":1}"#;
    let (status, json) = post_explain(&t.app, Some(ROOT_TOKEN), body).await;

    // It successfully resolved the repo, built evidence, but failed on the disabled provider model.
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{json}");
}

#[tokio::test]
async fn repo_resolution_fails_for_missing_file_in_nested_repo() {
    let temp = tempfile::tempdir().expect("tempdir");
    let t = explain_app_with_nested_repo(&temp).await;

    // file doesn't exist in nested repo
    let body = r#"{"file":"nested/repo/src/missing.rs","start_line":1,"end_line":1}"#;
    let (status, json) = post_explain(&t.app, Some(ROOT_TOKEN), body).await;

    // Should return 400 Bad Request because it resolves to a missing file.
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
}
