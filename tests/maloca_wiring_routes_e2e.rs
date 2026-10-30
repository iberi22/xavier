//! feat-maloca-governance-wiring: Maloca Wiring test
#[path = "router_harness/mod.rs"]
mod router_harness;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tokio_util::sync::CancellationToken;
use tower::util::ServiceExt;

xavier::isolate_test_process!();

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
    ]);
    // Explicitly disable inherited XAVIER_HC_SESSIONS_DIR
    let _old_sessions = std::env::var_os("XAVIER_HC_SESSIONS_DIR");
    std::env::remove_var("XAVIER_HC_SESSIONS_DIR");

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

    // WS feed route is mounted (a plain GET without upgrade headers is rejected
    // by the extractor, but must not be a 404).
    let ws_req = Request::builder()
        .method(Method::GET)
        .uri("/maloca/ws/feed")
        .body(Body::empty())
        .unwrap();
    let ws_resp = app_router.clone().oneshot(ws_req).await.unwrap();
    assert_ne!(
        ws_resp.status(),
        StatusCode::NOT_FOUND,
        "WS feed route must be mounted"
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

    if let Some(v) = _old_sessions {
        std::env::set_var("XAVIER_HC_SESSIONS_DIR", v);
    }
}
