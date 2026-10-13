xavier::isolate_test_process!();

mod router_harness;

use axum::http::{Method, StatusCode};
use router_harness::{jwt, status, test_app};
use xavier::security::auth::UserRole;

// The shared harness uses xavier::cli::server::build_app to build the router.

#[tokio::test]
async fn test_settings_models_auth_matrix() {
    let t = test_app().await;

    let endpoints = vec![
        "/api/settings/cloud-node",
        "/api/settings/discord",
        "/api/settings/discord/test",
        "/api/settings/telegram",
        "/api/settings/telegram/test",
        "/v1/config/providers",
        "/v1/offline/config",
        "/v1/ollama/pull",
        "/v1/offline/download",
    ];

    let user_token = jwt(UserRole::User);

    for path in endpoints {
        // 1. No token -> 401 Unauthorized
        assert_eq!(
            status(&t.app, Method::POST, path, None).await,
            StatusCode::UNAUTHORIZED,
            "POST {} without token should return 401 Unauthorized",
            path
        );

        // 2. User JWT -> 403 Forbidden (can_edit_config is admin-only)
        assert_eq!(
            status(&t.app, Method::POST, path, Some(&user_token)).await,
            StatusCode::FORBIDDEN,
            "POST {} with User JWT should return 403 Forbidden",
            path
        );
    }
}
