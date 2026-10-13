//! Auth tests for proxy chat, agent spawn and mini-experts on the production router.

xavier::isolate_test_process!();

mod router_harness;

use axum::http::{Method, StatusCode};
use router_harness::{jwt, status, test_app};
use xavier::security::auth::UserRole;
use xavier::security::tokens::TokenStore;

#[tokio::test]
async fn proxy_agents_auth_matrix() {
    let t = test_app().await;

    // Use build_app test harness

    let token_store = TokenStore::new();
    token_store.init_schema_async().await.unwrap();
    let (read_only_token, _) = token_store
        .create_token("ro".into(), vec!["read_only".into()], None)
        .await
        .unwrap();

    let endpoints = vec![
        "/v1/proxy/chat/completions",
        "/v1/proxy/chat/completions/batch",
        "/v1/proxy/request",
        "/v1/agents/spawn",
        "/v1/agents/mini-experts/invoke",
    ];

    let readonly_jwt = jwt(UserRole::Readonly);

    for path in endpoints {
        // 1. No token -> 401 on all
        assert_eq!(
            status(&t.app, Method::POST, path, None).await,
            StatusCode::UNAUTHORIZED,
            "Expected 401 for missing token on POST {}",
            path
        );

        // 2. Read-only scoped token -> 403 on proxy POSTs, 403 on root-only proxy/request, etc.
        assert_eq!(
            status(&t.app, Method::POST, path, Some(&read_only_token)).await,
            StatusCode::FORBIDDEN,
            "Expected 403 for read-only scoped token on POST {}",
            path
        );
    }

    // 3. Readonly JWT -> 403 on mini-experts invoke (gated by can_add_memory)
    assert_eq!(
        status(
            &t.app,
            Method::POST,
            "/v1/agents/mini-experts/invoke",
            Some(&readonly_jwt)
        )
        .await,
        StatusCode::FORBIDDEN,
        "Expected 403 for Readonly JWT on POST /v1/agents/mini-experts/invoke"
    );
}
