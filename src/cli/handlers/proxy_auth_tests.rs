// SPDX-License-Identifier: MIT OR LICENSE-MESH
use serde_json::json;
use std::sync::Arc;
use xavier_lib::agents::rate_limit::RateLimitManager;
use xavier_lib::app::proxy_use_case::ProxyUseCase;
use xavier_lib::coordination::secrets::KeyLendingEngine;
use xavier_lib::secrets::audit::QmdAuditLogger;

#[tokio::test]
async fn test_proxy_auth_and_rate_limit() {
    let rate_manager = Arc::new(RateLimitManager::new());
    rate_manager.init_schema_async().await.unwrap();

    let audit_logger = Box::new(QmdAuditLogger::new());
    let secrets_engine = Arc::new(KeyLendingEngine::new(audit_logger, None));

    let prompt_cache = Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new()));
    let proxy = ProxyUseCase::new(rate_manager.clone(), prompt_cache);

    let agent_id = "test-agent";
    let lease = secrets_engine
        .lend("test-key", Some("secret-value"), agent_id, 3600)
        .await
        .unwrap();
    let token = lease.token;

    // 1. Test successful proxy use (should log and track)
    let cmd = xavier_lib::domain::proxy::ProxyChatCommand {
        model: "gpt-4".to_string(),
        messages: vec![json!({"role": "user", "content": "hello"})],
        temperature: None,
        max_tokens: None,
        lease_token: Some(token.clone()),
    };

    // We expect it to fail with ProviderError because we don't have real API keys/providers set up,
    // but it should pass the rate limit and lease checks.
    let result = proxy
        .execute_secured(
            cmd.clone(),
            true,
            secrets_engine.clone(),
            xavier_lib::coordination::XavierEventBus::new(1),
        )
        .await;

    match result {
        Err(xavier_lib::domain::proxy::ProxyError::ProviderError(_)) => {} // Expected: reached provider but failed
        other => panic!(
            "Expected ProviderError (passed rate limit), got {:?}",
            other
        ),
    }

    // 2. Test Rate Limit (100 req/min)
    // Manually insert 100 requests into the DB for this lease
    for _ in 0..100 {
        rate_manager
            .track_request(&format!("lease:{}", token), 0, 200, 0.0, false)
            .await
            .unwrap();
    }

    let result = proxy
        .execute_secured(
            cmd,
            true,
            secrets_engine.clone(),
            xavier_lib::coordination::XavierEventBus::new(1),
        )
        .await;
    assert!(matches!(
        result,
        Err(xavier_lib::domain::proxy::ProxyError::RateLimited)
    ));

    // 3. Test Expired Lease
    let expired_lease = secrets_engine
        .lend("expired-key", Some("secret-value"), agent_id, 0)
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(10)).await; // Ensure it's expired

    let cmd_expired = xavier_lib::domain::proxy::ProxyChatCommand {
        model: "gpt-4".to_string(),
        messages: vec![json!({"role": "user", "content": "hello"})],
        temperature: None,
        max_tokens: None,
        lease_token: Some(expired_lease.token),
    };

    let result = proxy
        .execute_secured(
            cmd_expired,
            true,
            secrets_engine.clone(),
            xavier_lib::coordination::XavierEventBus::new(1),
        )
        .await;
    assert!(
        matches!(result, Err(xavier_lib::domain::proxy::ProxyError::SecretError(msg)) if msg.contains("expired"))
    );
}
