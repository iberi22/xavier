//! feat-explain-isolated-llm: End-to-end integration tests for isolated LLM logic.

xavier::isolate_test_process!();

mod router_harness;

use axum::{
    body::to_bytes,
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
use router_harness::{create_test_cli_state, RestoreEnv, JWT_SECRET, ROOT_TOKEN};
use serde_json::Value;
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;
use xavier::cli::server::{build_app, AppDeps};
use xavier::workspace::{WorkspaceContext, WorkspaceState};

const EXPLAIN_BODY: &str = r#"{"file":"src/lib.rs","start_line":1,"end_line":2}"#;

#[cfg(unix)]
fn write_key_file(dir: &std::path::Path, contents: &str, mode: u32) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("explain.key");
    std::fs::write(&path, contents).expect("write key file");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
        .expect("set key file mode");
    path
}

/// Builds the app with the specific isolated explain environment variables.
async fn isolated_explain_app(
    temp: &TempDir,
    provider: Option<&str>,
    key_file: Option<&std::path::Path>,
) -> (Router, Vec<RestoreEnv>) {
    std::fs::create_dir_all(temp.path().join("src")).unwrap();
    std::fs::write(temp.path().join("src/lib.rs"), "fn one() {}\nfn two() {}\n").unwrap();

    let mut envs = vec![
        RestoreEnv::set(&[("HOME", temp.path()), ("XAVIER_DATA_DIR", temp.path())]),
        RestoreEnv::set_str(&[
            ("XAVIER_TOKEN", ROOT_TOKEN),
            ("XAVIER_JWT_SECRET", JWT_SECRET),
            ("XAVIER_MODEL_PROVIDER", "disabled"), // Global provider disabled
        ]),
    ];

    if let Some(p) = provider {
        envs.push(RestoreEnv::set_str(&[("XAVIER_EXPLAIN_PROVIDER", p)]));
    }
    if let Some(kf) = key_file {
        envs.push(RestoreEnv::set(&[("XAVIER_EXPLAIN_API_KEY_FILE", kf)]));
    }

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

    (build_app(state, deps), envs)
}

async fn post_explain(app: &Router) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/explain")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {ROOT_TOKEN}"))
        .body(Body::from(EXPLAIN_BODY))
        .expect("request");

    let resp = app.clone().oneshot(req).await.expect("response");
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("response body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[cfg(unix)]
#[tokio::test]
async fn explain_uses_isolated_llm_settings_and_fails_on_network() {
    let temp = tempfile::tempdir().expect("tempdir");
    let key_file = write_key_file(temp.path(), "fake-api-key", 0o600);

    // We configure Anthropic. Since we don't mock the endpoint here, it will try to call the real
    // anthropic or error out. What we want to check is that it attempts the provider request
    // rather than falling back to the disabled model immediately.
    let (app, _envs) = isolated_explain_app(&temp, Some("anthropic"), Some(&key_file)).await;

    let (status, json) = post_explain(&app).await;

    // The request should fail (BAD_GATEWAY) because the anthropic client cannot make a real request
    // (no network in sandbox, or DNS error), but it SHOULD indicate an Anthropic error, not a
    // global 'disabled' error.
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    let error_msg = json["error"].as_str().unwrap_or("");
    assert!(
        error_msg.contains("anthropic explanation request failed"),
        "Expected anthropic error, got: {}",
        error_msg
    );
    // Ensure the key doesn't leak in the error message
    assert!(!error_msg.contains("fake-api-key"));
}

#[tokio::test]
async fn explain_falls_back_to_global_when_isolated_not_configured() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (app, _envs) = isolated_explain_app(&temp, None, None).await;

    let (status, json) = post_explain(&app).await;

    // Global provider is 'disabled', so it should fail with the disabled provider error.
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    let error_msg = json["error"].as_str().unwrap_or("");
    assert!(
        error_msg.contains("disabled explanation request failed")
            || error_msg.contains("unconfigured"),
        "Expected disabled/unconfigured error, got: {}",
        error_msg
    );
}
