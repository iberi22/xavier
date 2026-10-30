//! feat-maloca: Maloca Model Router Unsupported E2E
#[path = "router_harness/mod.rs"]
mod router_harness;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tokio_util::sync::CancellationToken;
use tower::util::ServiceExt;

xavier::isolate_test_process!();

/// Removes an env var for the test's lifetime and restores it on drop.
struct RemoveEnvGuard {
    key: &'static str,
    prev: Option<std::ffi::OsString>,
}

impl RemoveEnvGuard {
    fn new(key: &'static str) -> Self {
        let prev = std::env::var_os(key);
        std::env::remove_var(key);
        Self { key, prev }
    }
}

impl Drop for RemoveEnvGuard {
    fn drop(&mut self) {
        match self.prev.take() {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

#[tokio::test]
#[serial_test::serial]
async fn maloca_models_unsupported_e2e() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let workspace_path = workspace_dir.path();

    let config_dir = workspace_path.join("config");
    let config_file = config_dir.join("config.json");
    let state_dir = workspace_path.join("state");
    let runtime_file = state_dir.join("runtime.json");
    let data_dir = workspace_path.join("data");

    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&state_dir).unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();

    let _env_str = router_harness::RestoreEnv::set_str(&[
        ("SWAL_WORKSPACE_DIR", workspace_path.to_str().unwrap()),
        ("XAVIER_TOKEN", router_harness::ROOT_TOKEN),
        ("XAVIER_JWT_SECRET", router_harness::JWT_SECRET),
        ("HOME", workspace_path.to_str().unwrap()),
        ("XAVIER_CONFIG_PATH", config_file.to_str().unwrap()),
        ("XAVIER_RUNTIME_STATE_PATH", runtime_file.to_str().unwrap()),
        ("XAVIER_DATA_DIR", data_dir.to_str().unwrap()),
        // The isolation macro installs a process-wide XAVIER_STATE_DIR that
        // takes precedence over XAVIER_DATA_DIR; pin it to this test's tempdir.
        ("XAVIER_STATE_DIR", state_dir.to_str().unwrap()),
    ]);
    let _hc_sessions = RemoveEnvGuard::new("XAVIER_HC_SESSIONS_DIR");

    let state = router_harness::create_test_cli_state(&workspace_dir).await;
    let workspace_state = std::sync::Arc::new(
        xavier::workspace::WorkspaceState::new_minimal(
            state.workspace_id.clone(),
            state.workspace_dir.clone(),
            std::sync::Arc::clone(&state.qmd_memory),
            std::sync::Arc::clone(&state.store),
        )
        .await,
    );

    let shutdown = CancellationToken::new();
    let maloca_wiring = xavier::cli::maloca_wiring::build_maloca_router(&state, shutdown.clone());

    let deps = xavier::cli::server::AppDeps {
        workspace_ctx: xavier::workspace::WorkspaceContext {
            workspace_id: state.workspace_id.clone(),
            workspace: workspace_state,
        },
        space_manager: None,
        maloca_router: maloca_wiring.router,
        event_bus_for_ws: xavier::coordination::XavierEventBus::new(10),
        no_ui: true,
    };
    let app_router = xavier::cli::server::build_app(state.clone(), deps);

    let endpoints = vec![
        (Method::POST, "/v1/maloca/models/infer"),
        (Method::GET, "/v1/maloca/models/list"),
        (Method::GET, "/v1/maloca/models/health"),
    ];

    for (method, path) in endpoints {
        let req = Request::builder()
            .method(method)
            .uri(path)
            .body(Body::empty())
            .unwrap();

        let resp = app_router.clone().oneshot(req).await.unwrap();

        assert_eq!(
            resp.status(),
            StatusCode::NOT_IMPLEMENTED,
            "Endpoint {} should be 501 NOT_IMPLEMENTED",
            path
        );

        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();

        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(json["status"], "unsupported");
        assert_eq!(json["use_instead"], "/v1/providers");
        assert_eq!(json["feature"], "maloca-model-router");
        assert_eq!(json["route"], path);
        assert_eq!(json["docs"], "docs/FEATURE_STATUS.md#maloca-model-router");
    }

    shutdown.cancel();
}
