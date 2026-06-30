use super::config::*;
use super::rate_limit::*;
use super::types::*;

use crate::domain::proxy::types::{ApiTier, ProviderKind as DomainProviderKind, ProviderQuota};
use chrono::{Duration, Utc};
use std::sync::{Mutex, OnceLock};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[tokio::test]
async fn test_rate_limit_manager_tracking() {
    let project_id = format!("test_metrics_{}", ulid::Ulid::new());
    let manager = RateLimitManager::new_with_project(&project_id);
    manager.init_schema_async().await.unwrap();

    // Track a successful request
    manager
        .track_request("openai", 100, 200, 0.01, false)
        .await
        .unwrap();

    let status = manager.get_status("openai").await.unwrap();
    assert_eq!(status.used_today, 100);
    assert!(status.rate_limited_until.is_none());

    // Track a 429 error
    manager
        .track_request("openai", 0, 429, 0.0, false)
        .await
        .unwrap();
    let status = manager.get_status("openai").await.unwrap();
    assert!(status.rate_limited_until.is_some());
    assert!(manager.check("openai").await);
}

#[tokio::test]
async fn test_rate_limit_manager_reset() {
    let project_id = format!("test_metrics_reset_{}", ulid::Ulid::new());
    let manager = RateLimitManager::new_with_project(&project_id);
    manager.init_schema_async().await.unwrap();

    manager.report_429("anthropic", 10).await.unwrap();
    assert!(manager.check("anthropic").await);

    manager.reset("anthropic").await.unwrap();
    assert!(!manager.check("anthropic").await);
}

#[tokio::test]
async fn test_quota_trackers_integration() {
    let project_id = format!("test_quota_{}", ulid::Ulid::new());
    let manager = RateLimitManager::new_with_project(&project_id);
    manager.init_schema_async().await.unwrap();

    let token_tracker = TokenQuotaTracker::new(RateLimitManager::new_with_project(&project_id));
    let quota_tracker = QuotaTracker::new(RateLimitManager::new_with_project(&project_id));

    assert!(quota_tracker.check("gemini").await);

    token_tracker.increment("gemini", 500).await.unwrap();
    let status = manager.get_status("gemini").await.unwrap();
    assert_eq!(status.used_today, 500);

    manager.report_429("gemini", 5).await.unwrap();
    assert!(!quota_tracker.check("gemini").await);
}

#[tokio::test]
async fn test_update_quota() {
    let project_id = format!("test_update_quota_{}", ulid::Ulid::new());
    let manager = RateLimitManager::new_with_project(&project_id);
    manager.init_schema_async().await.unwrap();

    let quota = ProviderQuota {
        provider: DomainProviderKind::OpenAI,
        api_tier: ApiTier::Pro,
        requests_remaining: Some(100),
        tokens_remaining: Some(10000),
        requests_limit: Some(1000),
        tokens_limit: Some(100000),
        resets_at: Some(Utc::now() + Duration::hours(1)),
        last_checked: Utc::now(),
    };

    manager.update_quota(quota).await.unwrap();
    let quotas = manager.get_all_quotas().await.unwrap();

    assert_eq!(quotas.len(), 1);
    assert_eq!(quotas[0].provider, DomainProviderKind::OpenAI);
    assert_eq!(quotas[0].requests_remaining, Some(100));
}

#[test]
fn test_config_from_label_parsing() {
    let config = ModelProviderConfig::from_label("openai");
    assert_eq!(config.provider_label, "openai");
    assert_eq!(config.provider_mode, ProviderMode::Cloud);

    let config = ModelProviderConfig::from_label("local");
    assert_eq!(config.provider_label, "local");
    assert_eq!(config.provider_mode, ProviderMode::Local);

    let config = ModelProviderConfig::from_label("nonexistent");
    // Should default to local
    assert_eq!(config.provider_label, "local");
}

#[test]
fn test_config_with_overrides() {
    let config = ModelProviderConfig::from_label("openai")
        .with_model_override(Some("gpt-3.5-turbo".to_string()))
        .with_api_key(Some("test-key".to_string()))
        .with_base_url(Some("https://custom.api/v1".to_string()));

    assert_eq!(config.model, "gpt-3.5-turbo");
    assert_eq!(config.api_key.as_deref(), Some("test-key"));
    assert_eq!(config.base_url.as_deref(), Some("https://custom.api/v1"));
}

#[test]
fn test_model_provider_config_from_env() {
    let _guard = env_lock().lock().expect("test assertion");

    std::env::set_var("XAVIER_MODEL_PROVIDER", "anthropic");
    let config = ModelProviderConfig::from_env();
    assert_eq!(config.provider_label, "anthropic");

    std::env::set_var("XAVIER_MODEL_PROVIDER", "openai");
    let config = ModelProviderConfig::from_env();
    assert_eq!(config.provider_label, "openai");

    std::env::remove_var("XAVIER_MODEL_PROVIDER");
}

#[test]
fn test_zai_provider_config() {
    let config = ModelProviderConfig::for_provider("z.ai");
    assert_eq!(config.provider_label, "z.ai");
    assert_eq!(config.provider_mode, ProviderMode::Cloud);
    assert_eq!(config.api_flavor, ApiFlavor::OpenAICompatible);
    assert_eq!(config.base_url, Some("https://api.z.ai/v1".to_string()));
    assert_eq!(config.model, "glm-5.1");
}

#[test]
fn test_opencode_provider_config() {
    let config = ModelProviderConfig::for_provider("opencode");
    assert_eq!(config.provider_label, "opencode");
    assert_eq!(config.provider_mode, ProviderMode::Local);
    assert_eq!(config.target, ProviderTarget::OpenCodeCLI);
    assert_eq!(config.model, "opencode/deepseek-v4-flash");
}
