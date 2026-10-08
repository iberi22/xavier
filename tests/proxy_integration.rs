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
    // The product contract: when every provider the use case knows about is
    // currently rate-limited, `execute_secured` MUST return
    // `Err(ProxyError::RateLimited)` without ever opening an outbound LLM
    // call. The use case iterates
    //   opencode-go, deepseek, groq, openrouter, google, openai, anthropic,
    //   managed-local, local, ollama
    // and falls through to `RateLimited` only when the loop exhausts them
    // all. We mark every one of those (including `managed-local`, which the
    // previous version of this test silently omitted) as rate-limited and
    // then assert the contract.
    //
    // IMPORTANT: `RateLimitManager` is built on the global `ConnectionManager`
    // keyed by `project_id`. The default `RateLimitManager::new()` uses the
    // shared `metrics` pool — that pool is a singleton, so any other test in
    // the same process that wrote to it (or any pre-existing entry left over
    // on disk) would be observable here. We use a unique `project_id` so this
    // test gets a private DB file under the worktree and is hermetic.
    // `XAVIER_DATA_DIR` is not consumed by `ConnectionManager`/`RateLimitManager`
    // (only by `notifications::db` and `storage::multi_db`), so it is not the
    // right knob to turn here.
    let rate_manager = std::sync::Arc::new(RateLimitManager::new_with_project(
        "proxy_integration_test_rate_limited",
    ));
    rate_manager.init_schema_async().await.unwrap();
    let prompt_cache = Arc::new(Mutex::new(HashMap::new()));

    // Mark every provider the use case may pick as rate limited.
    // `managed-local` was missing from earlier versions of this list, which
    // let the loop reach a real outbound LLM call and fail with
    // `ProviderError` instead of the expected `RateLimited`.
    let providers = [
        "opencode-go",
        "deepseek",
        "groq",
        "openrouter",
        "google",
        "openai",
        "anthropic",
        "managed-local",
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
    assert!(
        matches!(result, Err(ProxyError::RateLimited)),
        "Expected RateLimited, got {:?}",
        result
    );
}

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();
