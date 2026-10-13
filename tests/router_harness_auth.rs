//! Self-test of the shared router harness (also the usage example for
//! `tests/router_*_auth.rs`).

xavier::isolate_test_process!();

mod router_harness;

use axum::http::{Method, StatusCode};
use router_harness::{jwt, status, test_app, ROOT_TOKEN};
use xavier::security::auth::UserRole;

#[tokio::test]
async fn harness_builds_the_production_router_with_auth() {
    let t = test_app().await;
    assert_eq!(
        status(&t.app, Method::GET, "/health", None).await,
        StatusCode::OK
    );
    assert_eq!(
        status(&t.app, Method::POST, "/v1/memories/consolidate", None).await,
        StatusCode::UNAUTHORIZED
    );
    let readonly = jwt(UserRole::Readonly);
    assert_eq!(
        status(
            &t.app,
            Method::POST,
            "/v1/memories/consolidate",
            Some(&readonly)
        )
        .await,
        StatusCode::FORBIDDEN
    );
    assert_ne!(
        status(
            &t.app,
            Method::POST,
            "/v1/memories/consolidate",
            Some(ROOT_TOKEN)
        )
        .await,
        StatusCode::UNAUTHORIZED
    );
}
