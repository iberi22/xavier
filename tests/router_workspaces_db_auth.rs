//! Workspace database routes on the production router: creating a database
//! needs the same role as deleting one.

xavier::isolate_test_process!();

mod router_harness;

use axum::http::{Method, StatusCode};
use router_harness::{jwt, status, test_app};
use xavier::security::auth::UserRole;

#[tokio::test]
async fn workspace_db_create_requires_the_admin_role() {
    let t = test_app().await;
    assert_eq!(
        status(&t.app, Method::POST, "/v1/workspaces/db", None).await,
        StatusCode::UNAUTHORIZED
    );
    for role in [UserRole::Readonly, UserRole::User] {
        let token = jwt(role);
        assert_eq!(
            status(&t.app, Method::POST, "/v1/workspaces/db", Some(&token)).await,
            StatusCode::FORBIDDEN,
            "create must be refused for {role:?}"
        );
    }
    let admin = jwt(UserRole::Admin);
    assert_ne!(
        status(&t.app, Method::POST, "/v1/workspaces/db", Some(&admin)).await,
        StatusCode::FORBIDDEN
    );
}
