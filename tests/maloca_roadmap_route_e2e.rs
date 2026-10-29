//! feat-maloca-roadmap-route: E2E test for the Maloca roadmap route.
#[path = "router_harness/mod.rs"]
mod router_harness;

use axum::body::{to_bytes, Body};
use axum::http::{Method, Request, StatusCode};
use std::fs::{create_dir_all, write};
use tower::util::ServiceExt;
use xavier::server::maloca::roadmap_route::RoadmapResponse;

xavier::isolate_test_process!();

#[tokio::test]
#[serial_test::serial]
async fn roadmap_endpoint_reads_gitcore_ledgers_e2e() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let workspace_path = workspace_dir.path();

    let gitcore = workspace_path.join("shelf").join(".gitcore");
    create_dir_all(&gitcore).unwrap();
    write(
        gitcore.join("features.json"),
        r#"{
            "metadata": { "project": "shelf" },
            "features": {
                "feat-w2": { "id": "feat-w2", "name": "Wave 2", "status": "planned", "progress_pct": 0.0, "wave": 2 },
                "feat-w1": { "id": "feat-w1", "name": "Wave 1", "status": "stable", "progress_pct": 100.0, "wave": 1 },
                "feat-none": { "id": "feat-none", "name": "Loose", "status": "planned", "progress_pct": 10.0 }
            }
        }"#,
    )
    .unwrap();

    std::env::set_var("SWAL_WORKSPACE_DIR", workspace_path);

    // Build the app with maloca_router mounted using existing harness helpers
    let _env_str = router_harness::RestoreEnv::set_str(&[
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
    let deps = xavier::cli::server::AppDeps {
        workspace_ctx: xavier::workspace::WorkspaceContext {
            workspace_id: state.workspace_id.clone(),
            workspace: workspace_state,
        },
        space_manager: None,
        maloca_router: xavier::server::maloca::v1_maloca_router_with_maloca_store(
            None,
            Some(state.workspace_dir.clone()),
            Some(state.maloca.clone()),
        ),
        event_bus_for_ws: xavier::coordination::XavierEventBus::new(10),
        no_ui: true,
    };
    let app_router = xavier::cli::server::build_app(state, deps);

    let req = Request::builder()
        .method(Method::GET)
        .uri("/v1/maloca/roadmap")
        .header(
            "Authorization",
            format!("Bearer {}", router_harness::ROOT_TOKEN),
        )
        .body(Body::empty())
        .unwrap();

    let resp = app_router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let roadmap: RoadmapResponse = serde_json::from_slice(&body).unwrap();

    let waves: Vec<u32> = roadmap.waves.iter().map(|w| w.wave).collect();
    assert_eq!(waves, vec![1, 2]);
    assert_eq!(roadmap.waves[0].summary.stable, 1);
    assert_eq!(roadmap.waves[1].summary.planned, 1);

    // The test MalocaStore seeds 'maloca-ui' and 'maloca-pwa' automatically into the backlog,
    // plus the 'feat-none' we wrote to features.json
    assert_eq!(roadmap.unscheduled.total, 3);
    assert!(roadmap
        .unscheduled
        .features
        .iter()
        .any(|f| f.id == "feat-none"));
}

#[tokio::test]
#[serial_test::serial]
async fn roadmap_endpoint_returns_empty_when_no_projects() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let workspace_path = workspace_dir.path();

    // Empty workspace dir
    std::env::set_var("SWAL_WORKSPACE_DIR", workspace_path);

    // Build the app with maloca_router mounted using existing harness helpers
    let _env_str = router_harness::RestoreEnv::set_str(&[
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
    let deps = xavier::cli::server::AppDeps {
        workspace_ctx: xavier::workspace::WorkspaceContext {
            workspace_id: state.workspace_id.clone(),
            workspace: workspace_state,
        },
        space_manager: None,
        maloca_router: xavier::server::maloca::v1_maloca_router_with_maloca_store(
            None,
            Some(state.workspace_dir.clone()),
            Some(state.maloca.clone()),
        ),
        event_bus_for_ws: xavier::coordination::XavierEventBus::new(10),
        no_ui: true,
    };
    let app_router = xavier::cli::server::build_app(state, deps);

    let req = Request::builder()
        .method(Method::GET)
        .uri("/v1/maloca/roadmap")
        .header(
            "Authorization",
            format!("Bearer {}", router_harness::ROOT_TOKEN),
        )
        .body(Body::empty())
        .unwrap();

    let resp = app_router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let roadmap: RoadmapResponse = serde_json::from_slice(&body).unwrap();

    // No waves
    assert!(roadmap.waves.is_empty());

    // Still returns the defaults inserted by maloca-store
    assert_eq!(roadmap.unscheduled.total, 2);
}
