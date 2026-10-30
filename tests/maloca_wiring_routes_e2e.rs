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

    let _env_str = router_harness::RestoreEnv::set_str(&[
        ("SWAL_WORKSPACE_DIR", workspace_path.to_str().unwrap()),
        ("XAVIER_TOKEN", router_harness::ROOT_TOKEN),
        ("XAVIER_JWT_SECRET", router_harness::JWT_SECRET),
    ]);

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

    let endpoints = [
        "/maloca/timeline",
        "/maloca/timeline/sessions",
        "/maloca/timeline/123/context",
        "/maloca/commits/graph",
        "/maloca/ws/feed",
        "/maloca/beliefs",
        "/maloca/beliefs/path",
        "/maloca/feed/status",
    ];

    for ep in endpoints {
        let req = Request::builder()
            .method(Method::GET)
            .uri(ep)
            .body(Body::empty())
            .unwrap();

        let resp = app_router.clone().oneshot(req).await.unwrap();
        // Since we hit static routes, it just shouldn't be 404 (or it might be 400/500/etc.)
        assert_ne!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "Endpoint {} was 404 Not Found",
            ep
        );
    }

    // Write request without token
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/maloca/proposals")
        .body(Body::empty())
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
        .uri("/v1/maloca/proposals")
        .header(
            "Authorization",
            format!("Bearer {}", router_harness::ROOT_TOKEN),
        )
        .body(Body::empty())
        .unwrap();
    let resp = app_router.clone().oneshot(req).await.unwrap();
    // It should not be UNAUTHORIZED (might be 400 Bad Request since body is empty).
    assert_ne!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "Mutating endpoint with token must not return 401"
    );

    shutdown.cancel();
}
