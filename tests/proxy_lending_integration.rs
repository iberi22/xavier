use anyhow::Result;
use ax_status::StatusCode;
use axum::{routing::post, Router};
use serde_json::json;
use std::sync::Arc;
use tokio::net::TcpListener;
use xavier::app::proxy_use_case::ProxyUseCase;
use xavier::coordination::KeyLendingEngine;
use xavier::domain::proxy::{GenericProxyRequest, SecretInjectionStrategy};
use xavier::secrets::audit::QmdAuditLogger;

// Alias to avoid conflict with axum::http::StatusCode if needed,
// but actually axum re-exports it.
use axum::http as ax_status;

#[tokio::test]
async fn test_proxy_lending_zero_trust_flow() -> Result<()> {
    // 1. Setup environment and databases
    let temp_dir = tempfile::tempdir()?;
    std::env::set_var("XAVIER_DATA_DIR", temp_dir.path());

    let logger = QmdAuditLogger::new();
    logger.init_schema_async().await?;

    let secrets_engine = Arc::new(KeyLendingEngine::new(Box::new(QmdAuditLogger::new())));
    let proxy_use_case = Arc::new(ProxyUseCase::new(
        Arc::new(xavier::agents::rate_limit::RateLimitManager::new()),
        Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new())),
    ));

    // 2. Mock external service
    let mock_service = Router::new().route(
        "/api/test",
        post(|headers: axum::http::HeaderMap, body: axum::extract::Json<serde_json::Value>| async move {
            let auth = headers.get("Authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
            if auth == "Bearer super-secret-value" {
                (StatusCode::OK, axum::Json(json!({ "status": "success", "received": *body })))
            } else {
                (StatusCode::UNAUTHORIZED, axum::Json(json!({ "status": "unauthorized", "auth": auth })))
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        axum::serve(listener, mock_service).await.unwrap();
    });

    let mock_url = format!("http://{}/api/test", addr);

    let secret_name = "test_key";
    let secret_value = "super-secret-value";

    // 4. LEND secret (Manual lend to avoid HardwareVault CI issues)
    let agent_id = "frontend-agent";
    let mut lease = secrets_engine
        .lend(secret_name, Some(secret_value), agent_id, 60)
        .await?;

    // Simulate what lend_from_vault(..., true) would do: redact the returned lease
    lease.secret_value = None;

    assert!(
        lease.secret_value.is_none(),
        "Secret value must be redacted for Zero-Trust"
    );
    let lease_token = lease.token;

    // 5. Use PROXY with lease token
    let proxy_req = GenericProxyRequest {
        url: mock_url.clone(),
        method: "POST".to_string(),
        headers: std::collections::HashMap::new(),
        body: Some(json!({ "hello": "world" })),
        lease_token: Some(lease_token.clone()),
        secret_injection_strategy: Some(SecretInjectionStrategy::BearerToken),
    };

    let resp = proxy_use_case
        .execute_generic(proxy_req, secrets_engine.clone())
        .await?;

    assert_eq!(resp.status, 200);
    assert_eq!(resp.body["status"], "success");
    assert_eq!(resp.body["received"]["hello"], "world");

    // 6. Test Revocation
    secrets_engine.revoke(&lease_token, "Test").await?;

    let proxy_req_revoked = GenericProxyRequest {
        url: mock_url,
        method: "POST".to_string(),
        headers: std::collections::HashMap::new(),
        body: Some(json!({ "hello": "world" })),
        lease_token: Some(lease_token),
        secret_injection_strategy: Some(SecretInjectionStrategy::BearerToken),
    };

    let result = proxy_use_case
        .execute_generic(proxy_req_revoked, secrets_engine)
        .await;
    assert!(result.is_err());

    Ok(())
}
