//! Integration tests for mesh peer and workspace authorization via the production router.

xavier::isolate_test_process!();

mod router_harness;

use axum::http::{Method, StatusCode};
use router_harness::{jwt, status, test_app};
// The harness uses build_app internally
use xavier::security::auth::UserRole;

#[tokio::test]
async fn mutating_mesh_routes_without_token_return_401() {
    let t = test_app().await;

    let routes = vec![
        (Method::POST, "/v1/mesh/peers"),
        (Method::POST, "/v1/mesh/peers/pair"),
        (Method::POST, "/v1/mesh/peers/decode"),
        (Method::POST, "/v1/mesh/peers/generate-code"),
        (Method::PUT, "/v1/mesh/peers/some_node/acl"),
        (Method::DELETE, "/v1/mesh/peers/some_node"),
        (Method::POST, "/v1/mesh/workspaces/share"),
        (Method::POST, "/v1/mesh/workspaces/join"),
        (Method::POST, "/v1/mesh/workspaces/query"),
        (Method::POST, "/v1/mesh/consent/revoke"),
        (Method::POST, "/v1/mesh/bridges"),
        (Method::DELETE, "/v1/mesh/bridges/some_bridge"),
    ];

    for (method, path) in routes {
        assert_eq!(
            status(&t.app, method.clone(), path, None).await,
            StatusCode::UNAUTHORIZED,
            "Expected 401 for {} {}",
            method,
            path
        );
    }
}

#[tokio::test]
async fn mutating_mesh_peer_config_with_user_token_returns_403() {
    let t = test_app().await;
    // A regular User token (lacks `can_edit_config`)
    let user_token = jwt(UserRole::User);

    let routes = vec![
        (Method::POST, "/v1/mesh/peers"),
        (Method::PUT, "/v1/mesh/peers/some_node/acl"),
        (Method::DELETE, "/v1/mesh/peers/some_node"),
    ];

    for (method, path) in routes {
        assert_eq!(
            status(&t.app, method.clone(), path, Some(&user_token)).await,
            StatusCode::FORBIDDEN,
            "Expected 403 for {} {}",
            method,
            path
        );
    }
}
