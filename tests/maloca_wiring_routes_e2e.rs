//! feat-maloca-governance-wiring: Maloca Wiring test
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
async fn maloca_wiring_e2e() {
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
    // Disable an inherited XAVIER_HC_SESSIONS_DIR; RAII-restored even on panic.
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

    let json_endpoints = ["/maloca/feed/status"];

    for ep in json_endpoints {
        let req = Request::builder()
            .method(Method::GET)
            .uri(ep)
            .body(Body::empty())
            .unwrap();

        let resp = app_router.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "Endpoint {} should be 200 OK",
            ep
        );
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let _json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        if ep == "/maloca/feed/status" {
            let status_json: xavier::maloca::ws::FeedStatus =
                serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(status_json.status, "idle");
        }
    }

    let other_endpoints = [
        "/maloca/timeline",
        "/maloca/timeline/sessions",
        "/maloca/timeline/123/context",
        "/maloca/commits/graph",
        "/maloca/beliefs",
        "/maloca/beliefs/path",
    ];

    for ep in other_endpoints {
        // These reads sit behind auth_middleware; with a valid token the route
        // must be mounted (handlers may return 4xx/5xx for an empty store, but
        // never 404 or 401).
        let req = Request::builder()
            .method(Method::GET)
            .uri(ep)
            .header(
                "Authorization",
                format!("Bearer {}", router_harness::ROOT_TOKEN),
            )
            .body(Body::empty())
            .unwrap();

        let resp = app_router.clone().oneshot(req).await.unwrap();
        assert_ne!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "Endpoint {} was 404 Not Found",
            ep
        );
        assert_ne!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "Endpoint {} rejected a valid token",
            ep
        );
    }

    // WS feed route is mounted behind auth: with a valid token a plain GET
    // (no upgrade headers) reaches the WebSocketUpgrade extractor, which
    // rejects it with a 4xx that is neither 401 nor 404.
    let ws_req = Request::builder()
        .method(Method::GET)
        .uri("/maloca/ws/feed")
        .header(
            "Authorization",
            format!("Bearer {}", router_harness::ROOT_TOKEN),
        )
        .body(Body::empty())
        .unwrap();
    let ws_status = app_router.clone().oneshot(ws_req).await.unwrap().status();
    assert!(
        ws_status.is_client_error()
            && ws_status != StatusCode::NOT_FOUND
            && ws_status != StatusCode::UNAUTHORIZED,
        "WS feed must reach the upgrade extractor, got {ws_status}"
    );

    // Write request without token
    let body_json = r#"{"session_id": "test_session"}"#;
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/maloca/challenges/generate")
        .header("content-type", "application/json")
        .body(Body::from(body_json))
        .unwrap();
    let resp = app_router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "Mutating endpoint without token must return 401"
    );

    // Write request with token
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/maloca/challenges/generate")
        .header(
            "Authorization",
            format!("Bearer {}", router_harness::ROOT_TOKEN),
        )
        .header("content-type", "application/json")
        .body(Body::from(body_json))
        .unwrap();
    let resp = app_router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "Mutating endpoint with token must return OK"
    );
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let resp_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(
        resp_json["count"], 5,
        "generate returns the five default challenge types"
    );
    assert!(
        resp_json.get("challenges").is_some(),
        "Response JSON must have a 'challenges' field"
    );

    shutdown.cancel();
}
