use std::sync::Arc;
use parking_lot::Mutex;
use std::collections::HashMap;
use xavier::agents::rate_limit::RateLimitManager;
use xavier::ports::outbound::schema_init::SchemaInitializer;
use xavier::app::proxy_use_case::ProxyUseCase;
use xavier::domain::proxy::{ProxyChatCommand, ProxyError};

#[tokio::test]
async fn test_proxy_use_case_rate_limited() {
    let temp_file = tempfile::NamedTempFile::new().unwrap();
    let db = libsql::Builder::new_local(temp_file.path().to_str().unwrap()).build().await.unwrap();
    let pool = xavier::utils::connection_pool::LibsqlConnectionPool::new(db, Default::default());
    let rate_manager = std::sync::Arc::new(RateLimitManager::new(pool));
    rate_manager.init_schema().unwrap();
    let prompt_cache = Arc::new(Mutex::new(HashMap::new()));

    // Mark all providers as rate limited
    let providers = [
        "opencode-go", "deepseek", "groq", "openrouter", "google", "openai", "anthropic",
    ];
    for p in providers {
        rate_manager.report_429(p, 10).await.unwrap();
    }

    let use_case = ProxyUseCase::new(rate_manager, prompt_cache);
    let cmd = ProxyChatCommand {
        model: "test-model".into(),
        messages: vec![serde_json::json!({"role": "user", "content": "hello"})],
        temperature: None,
        max_tokens: None,
    };

    let result = use_case.execute(cmd).await;
    assert!(matches!(result, Err(ProxyError::RateLimited)));
}
