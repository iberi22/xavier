//! Auth and wiring tests for `POST /v1/explain` on the production router.

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

const EXPLAIN_BODY: &str = r#"{"file":"src/lib.rs","start_line":1,"end_line":2}"#;

/// The production router plus the env guards that must outlive it.
struct App {
    app: Router,
    _envs: Vec<RestoreEnv>,
}

/// `build_app` over `temp`, with `src/lib.rs` in the workspace.
async fn explain_app(temp: &TempDir) -> App {
    std::fs::create_dir_all(temp.path().join("src")).unwrap();
    std::fs::write(temp.path().join("src/lib.rs"), "fn one() {}\nfn two() {}\n").unwrap();

    let envs = vec![
        RestoreEnv::set(&[("HOME", temp.path()), ("XAVIER_DATA_DIR", temp.path())]),
        RestoreEnv::set_str(&[
            ("XAVIER_TOKEN", ROOT_TOKEN),
            ("XAVIER_JWT_SECRET", JWT_SECRET),
            // No provider call leaves the process: the model reports "unconfigured".
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

/// `POST /v1/explain` with a valid body and an optional bearer token.
async fn post_explain(app: &Router, bearer: Option<&str>) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/v1/explain")
        .header("Content-Type", "application/json");
    if let Some(token) = bearer {
        req = req.header("Authorization", format!("Bearer {token}"));
    }
    let resp = app
        .clone()
        .oneshot(req.body(Body::from(EXPLAIN_BODY)).expect("request"))
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
async fn explain_rejects_missing_or_invalid_token_like_other_v1_routes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let t = explain_app(&temp).await;

    let (status, _) = post_explain(&t.app, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = post_explain(&t.app, Some("not-a-real-token")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Same answer as an existing protected /v1 route without a token.
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/memories/consolidate")
        .header("Content-Type", "application/json")
        .body(Body::from("{}"))
        .expect("request");
    let resp = t.app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn explain_with_root_token_reaches_the_model_and_returns_502() {
    let temp = tempfile::tempdir().expect("tempdir");
    let t = explain_app(&temp).await;

    let (status, json) = post_explain(&t.app, Some(ROOT_TOKEN)).await;

    // The evidence pass runs on the real workspace; the disabled provider then
    // fails and the route reports it as a structured 502.
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{json}");
    assert!(
        json["error"].as_str().is_some_and(|e| !e.is_empty()),
        "{json}"
    );
}
