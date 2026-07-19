use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use xavier::agents::rate_limit::RateLimitManager;
use xavier::app::proxy_use_case::ProxyUseCase;
use xavier::coordination::{KeyLendingEngine, XavierEventBus};
use xavier::domain::proxy::{ProxyChatCommand, ProxyError};
use xavier::secrets::audit::QmdAuditLogger;

#[tokio::test]
async fn test_proxy_use_case_rate_limited() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::env::set_var("XAVIER_DATA_DIR", temp_dir.path());
    let rate_manager = std::sync::Arc::new(RateLimitManager::new());
    rate_manager.init_schema_async().await.unwrap();
    let prompt_cache = Arc::new(Mutex::new(HashMap::new()));

    // Mark all providers as rate limited
    let providers = [
        "opencode-go",
        "deepseek",
        "groq",
        "openrouter",
        "google",
        "openai",
        "anthropic",
        "local",
        "ollama",
    ];
    for p in providers {
        rate_manager.report_429(p, 10).await.unwrap();
    }

    let use_case = ProxyUseCase::new(rate_manager, prompt_cache);
    let audit_logger = Box::new(QmdAuditLogger::new());
    let event_bus = XavierEventBus::new(10);
    let secrets_engine = Arc::new(KeyLendingEngine::new(audit_logger, Some(event_bus.clone())));

    let cmd = ProxyChatCommand {
        model: "test-model".into(),
        messages: vec![serde_json::json!({"role": "user", "content": "hello"})],
        temperature: None,
        max_tokens: None,
        lease_token: None,
    };

    let result = use_case
        .execute_secured(cmd, false, secrets_engine, event_bus)
        .await;
    assert!(matches!(result, Err(ProxyError::RateLimited)), "Expected RateLimited, got {:?}", result);
}
