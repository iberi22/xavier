#[cfg(test)]
mod tests {
    use crate::app::proxy_use_case::ProxyUseCase;
    use crate::coordination::{KeyLendingEngine, XavierEventBus};
    use crate::domain::proxy::ProxyChatCommand;
    use crate::secrets::audit::QmdAuditLogger;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_proxy_lease_lifecycle() {
        let rate_manager = Arc::new(crate::agents::rate_limit::RateLimitManager::new());
        let prompt_cache = Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new()));
        let _use_case = ProxyUseCase::new(rate_manager, prompt_cache);

        let audit_logger = Box::new(QmdAuditLogger::new());
        let event_bus = XavierEventBus::new(10);
        let secrets_engine = Arc::new(KeyLendingEngine::new(audit_logger, Some(event_bus.clone())));

        // Create a lease
        let lease = secrets_engine
            .lend("test-secret", Some("test-value"), "agent-1", 60)
            .await
            .unwrap();
        let token = lease.token.clone();

        let _cmd = ProxyChatCommand {
            model: "gpt-4o".to_string(),
            messages: vec![serde_json::json!({"role": "user", "content": "hello"})],
            temperature: None,
            max_tokens: None,
            lease_token: Some(token.clone()),
        };

        // We expect this to fail in test environment because providers are not configured,
        // but we want to see if it tries to resolve the lease.
        // To properly test renewal/backoff we'd need to mock the provider client,
        // which is hard-coded in ProxyUseCase.
        // For now, let's verify KeyLendingEngine's new methods directly.

        secrets_engine.renew(&token, 3600).await.unwrap();
        let updated = secrets_engine.get_lease(&token).await.unwrap();
        assert!(updated.expires_at > chrono::Utc::now() + chrono::Duration::minutes(59));

        secrets_engine.backoff(&token, 30).await.unwrap();
        let backed_off = secrets_engine.get_lease(&token).await.unwrap();
        assert!(backed_off.expires_at > updated.expires_at);
    }

    #[tokio::test]
    async fn test_proxy_use_case_provider_fallback() {
        use crate::agents::provider::router::{ProviderKind, ProviderRouter};
        use tokio::sync::RwLock;

        let rate_manager = Arc::new(crate::agents::rate_limit::RateLimitManager::new());
        let prompt_cache = Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new()));

        let mut router = ProviderRouter::new(ProviderKind::OpenAI);
        router.set_fallback_chain(vec![
            ProviderKind::OpenAI,
            ProviderKind::Anthropic,
            ProviderKind::Local,
        ]);
        let router_shared = Arc::new(RwLock::new(router));

        let use_case = ProxyUseCase::new(rate_manager, prompt_cache)
            .with_provider_router(router_shared.clone());

        let mut fallback_attempted = false;
        let result = use_case
            .handle_provider_fallback("openai", "gpt-4o", &mut fallback_attempted)
            .await;

        assert!(result.is_some());
        let (next_name, config) = result.unwrap();
        assert_eq!(next_name, "anthropic");
        assert_eq!(config.model, "gpt-4o");
        assert!(fallback_attempted);

        // A second fallback attempt should fail because fallback_attempted is now true (preventing recursion)
        let result_second = use_case
            .handle_provider_fallback("anthropic", "gpt-4o", &mut fallback_attempted)
            .await;
        assert!(result_second.is_none());
    }
}
