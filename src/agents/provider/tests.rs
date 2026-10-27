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
    let _guard = env_lock().lock().expect("test assertion");
    let prev_config_path = std::env::var("XAVIER_CONFIG_PATH").ok();
    std::env::set_var(
        "XAVIER_CONFIG_PATH",
        "/tmp/nonexistent-xavier-ci-defaults.json",
    );
    let prev_zai_model = std::env::var("ZAI_MODEL").ok();
    let prev_xav_zai_model = std::env::var("XAVIER_ZAI_MODEL").ok();
    std::env::remove_var("ZAI_MODEL");
    std::env::remove_var("XAVIER_ZAI_MODEL");

    let config = ModelProviderConfig::for_provider("z.ai");
    assert_eq!(config.provider_label, "z.ai");
    assert_eq!(config.provider_mode, ProviderMode::Cloud);
    assert_eq!(config.api_flavor, ApiFlavor::OpenAICompatible);
    assert_eq!(config.base_url, Some("https://api.z.ai/v1".to_string()));
    assert_eq!(config.model, "");

    match prev_config_path {
        Some(v) => std::env::set_var("XAVIER_CONFIG_PATH", v),
        None => std::env::remove_var("XAVIER_CONFIG_PATH"),
    }
    match prev_zai_model {
        Some(v) => std::env::set_var("ZAI_MODEL", v),
        None => std::env::remove_var("ZAI_MODEL"),
    }
    match prev_xav_zai_model {
        Some(v) => std::env::set_var("XAVIER_ZAI_MODEL", v),
        None => std::env::remove_var("XAVIER_ZAI_MODEL"),
    }
}

#[test]
fn test_opencode_provider_config() {
    let _guard = env_lock().lock().expect("test assertion");

    // Clear/Mock environment variables to prevent environment leaks/dependencies
    let prev_config_path = std::env::var("XAVIER_CONFIG_PATH").ok();
    std::env::set_var(
        "XAVIER_CONFIG_PATH",
        "/tmp/nonexistent-xavier-ci-defaults.json",
    );
    let old_opencode_model = std::env::var("XAVIER_OPENCODE_MODEL").ok();
    let old_opencode_model_alt = std::env::var("OPENCODE_MODEL").ok();
    let old_opencode_key = std::env::var("OPENCODE_API_KEY").ok();

    std::env::remove_var("XAVIER_OPENCODE_MODEL");
    std::env::remove_var("OPENCODE_MODEL");
    std::env::remove_var("OPENCODE_API_KEY");

    let config = ModelProviderConfig::for_provider("opencode");
    assert_eq!(config.provider_label, "opencode");
    assert_eq!(config.provider_mode, ProviderMode::Local);
    assert_eq!(config.target, ProviderTarget::OpenCodeCLI);
    assert_eq!(config.model, "");

    // Restore environment variables
    match prev_config_path {
        Some(v) => std::env::set_var("XAVIER_CONFIG_PATH", v),
        None => std::env::remove_var("XAVIER_CONFIG_PATH"),
    }
    if let Some(val) = old_opencode_model {
        std::env::set_var("XAVIER_OPENCODE_MODEL", val);
    }
    if let Some(val) = old_opencode_model_alt {
        std::env::set_var("OPENCODE_MODEL", val);
    }
    if let Some(val) = old_opencode_key {
        std::env::set_var("OPENCODE_API_KEY", val);
    }
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_opencode_provider_generation() {
    use super::client::ModelProviderClient;

    // Lock env to prevent parallel test pollution (OPENCODE_MODEL may be set in user env)
    let _guard = env_lock().lock().expect("test assertion");

    let old_model = std::env::var("XAVIER_OPENCODE_MODEL").ok();
    let old_model_alt = std::env::var("OPENCODE_MODEL").ok();
    std::env::remove_var("XAVIER_OPENCODE_MODEL");
    std::env::remove_var("OPENCODE_MODEL");

    let client = ModelProviderClient::for_provider(
        "opencode",
        Some("opencode/deepseek-v4-flash".to_string()),
    );
    let response = client.generate_text("System", "User").await.unwrap();
    assert!(response
        .text
        .contains("Mocked response for model opencode/deepseek-v4-flash"));

    if let Some(val) = old_model {
        std::env::set_var("XAVIER_OPENCODE_MODEL", val);
    }
    if let Some(val) = old_model_alt {
        std::env::set_var("OPENCODE_MODEL", val);
    }
}

#[test]
fn test_hosted_providers_no_model_defaults_and_unconfigured_reason() {
    let _guard = env_lock().lock().expect("test assertion");

    let prev_config_path = std::env::var("XAVIER_CONFIG_PATH").ok();
    std::env::set_var(
        "XAVIER_CONFIG_PATH",
        "/tmp/nonexistent-xavier-ci-defaults.json",
    );

    // 1. DeepSeek
    let prev_deepseek_model = std::env::var("DEEPSEEK_MODEL").ok();
    let prev_llm_model = std::env::var("XAVIER_LLM_MODEL").ok();
    std::env::remove_var("DEEPSEEK_MODEL");
    std::env::remove_var("XAVIER_LLM_MODEL");

    let deepseek_cfg = ModelProviderConfig::for_provider("deepseek");
    assert_eq!(
        deepseek_cfg.model, "",
        "deepseek model must be empty when unset"
    );
    assert!(!deepseek_cfg.is_configured());
    let deepseek_reason = deepseek_cfg.unconfigured_reason().expect("deepseek reason");
    assert!(
        deepseek_reason.contains("deepseek"),
        "reason should name provider deepseek: {deepseek_reason}"
    );
    assert!(
        deepseek_reason.contains("DEEPSEEK_MODEL") || deepseek_reason.contains("XAVIER_LLM_MODEL"),
        "reason should name variable to set: {deepseek_reason}"
    );

    // 2. Gemini
    let prev_gemini_model = std::env::var("GEMINI_MODEL").ok();
    std::env::remove_var("GEMINI_MODEL");

    let gemini_cfg = ModelProviderConfig::for_provider("gemini");
    assert_eq!(
        gemini_cfg.model, "",
        "gemini model must be empty when unset"
    );
    assert!(!gemini_cfg.is_configured());
    let gemini_reason = gemini_cfg.unconfigured_reason().expect("gemini reason");
    assert!(
        gemini_reason.contains("gemini"),
        "reason should name provider gemini: {gemini_reason}"
    );
    assert!(
        gemini_reason.contains("GEMINI_MODEL") || gemini_reason.contains("XAVIER_LLM_MODEL"),
        "reason should name variable to set: {gemini_reason}"
    );

    // 3. z.ai / zai
    let prev_zai_model = std::env::var("ZAI_MODEL").ok();
    let prev_xav_zai_model = std::env::var("XAVIER_ZAI_MODEL").ok();
    std::env::remove_var("ZAI_MODEL");
    std::env::remove_var("XAVIER_ZAI_MODEL");

    let zai_cfg = ModelProviderConfig::for_provider("z.ai");
    assert_eq!(zai_cfg.model, "", "zai model must be empty when unset");
    assert!(!zai_cfg.is_configured());
    let zai_reason = zai_cfg.unconfigured_reason().expect("zai reason");
    assert!(
        zai_reason.contains("z.ai") || zai_reason.contains("zai"),
        "reason should name provider z.ai: {zai_reason}"
    );
    assert!(
        zai_reason.contains("ZAI_MODEL") || zai_reason.contains("XAVIER_ZAI_MODEL"),
        "reason should name variable to set: {zai_reason}"
    );

    // 4. OpenCode
    let prev_opencode_model = std::env::var("OPENCODE_MODEL").ok();
    let prev_xav_opencode_model = std::env::var("XAVIER_OPENCODE_MODEL").ok();
    std::env::remove_var("OPENCODE_MODEL");
    std::env::remove_var("XAVIER_OPENCODE_MODEL");

    let opencode_cfg = ModelProviderConfig::for_provider("opencode");
    assert_eq!(
        opencode_cfg.model, "",
        "opencode model must be empty when unset"
    );
    assert!(!opencode_cfg.is_configured());
    let opencode_reason = opencode_cfg.unconfigured_reason().expect("opencode reason");
    assert!(
        opencode_reason.contains("opencode"),
        "reason should name provider opencode: {opencode_reason}"
    );
    assert!(
        opencode_reason.contains("OPENCODE_MODEL")
            || opencode_reason.contains("XAVIER_OPENCODE_MODEL")
            || opencode_reason.contains("opencode_model"),
        "reason should name variable to set: {opencode_reason}"
    );

    // Restore environment
    match prev_config_path {
        Some(v) => std::env::set_var("XAVIER_CONFIG_PATH", v),
        None => std::env::remove_var("XAVIER_CONFIG_PATH"),
    }
    match prev_deepseek_model {
        Some(v) => std::env::set_var("DEEPSEEK_MODEL", v),
        None => std::env::remove_var("DEEPSEEK_MODEL"),
    }
    match prev_llm_model {
        Some(v) => std::env::set_var("XAVIER_LLM_MODEL", v),
        None => std::env::remove_var("XAVIER_LLM_MODEL"),
    }
    match prev_gemini_model {
        Some(v) => std::env::set_var("GEMINI_MODEL", v),
        None => std::env::remove_var("GEMINI_MODEL"),
    }
    match prev_zai_model {
        Some(v) => std::env::set_var("ZAI_MODEL", v),
        None => std::env::remove_var("ZAI_MODEL"),
    }
    match prev_xav_zai_model {
        Some(v) => std::env::set_var("XAVIER_ZAI_MODEL", v),
        None => std::env::remove_var("XAVIER_ZAI_MODEL"),
    }
    match prev_opencode_model {
        Some(v) => std::env::set_var("OPENCODE_MODEL", v),
        None => std::env::remove_var("OPENCODE_MODEL"),
    }
    match prev_xav_opencode_model {
        Some(v) => std::env::set_var("XAVIER_OPENCODE_MODEL", v),
        None => std::env::remove_var("XAVIER_OPENCODE_MODEL"),
    }
}

#[test]
fn test_provider_resolution_honours_xavier_model_provider() {
    let _guard = env_lock().lock().expect("test assertion");

    let prev_model_provider = std::env::var("XAVIER_MODEL_PROVIDER").ok();
    let prev_provider = std::env::var("XAVIER_PROVIDER").ok();

    std::env::set_var("XAVIER_MODEL_PROVIDER", "openai");
    std::env::remove_var("XAVIER_PROVIDER");

    let config = ModelProviderConfig::from_env();
    assert_eq!(config.provider_label, "openai");

    let alerts = crate::server::alerts::SystemAlertStore::new();
    assert_eq!(
        alerts.get_mode(),
        crate::server::alerts::OperationalMode::CloudFallback
    );

    match prev_model_provider {
        Some(v) => std::env::set_var("XAVIER_MODEL_PROVIDER", v),
        None => std::env::remove_var("XAVIER_MODEL_PROVIDER"),
    }
    match prev_provider {
        Some(v) => std::env::set_var("XAVIER_PROVIDER", v),
        None => std::env::remove_var("XAVIER_PROVIDER"),
    }
}

#[tokio::test]
async fn test_generate_text_short_circuits_when_model_unconfigured() {
    let _guard = env_lock().lock().expect("test assertion");

    let prev_config_path = std::env::var("XAVIER_CONFIG_PATH").ok();
    std::env::set_var(
        "XAVIER_CONFIG_PATH",
        "/tmp/nonexistent-xavier-ci-defaults.json",
    );
    let prev_opencode_model = std::env::var("OPENCODE_MODEL").ok();
    let prev_xav_opencode_model = std::env::var("XAVIER_OPENCODE_MODEL").ok();
    std::env::remove_var("OPENCODE_MODEL");
    std::env::remove_var("XAVIER_OPENCODE_MODEL");

    let client =
        crate::agents::provider::client::ModelProviderClient::for_provider("opencode", None);
    let res = client.generate_text("System", "User").await;
    assert!(res.is_err(), "generation must fail when model is unset");
    let err_msg = res.unwrap_err().to_string();
    assert!(
        err_msg.contains("no LLM model configured for provider 'opencode'"),
        "error must be not-configured message, got: {err_msg}"
    );

    match prev_config_path {
        Some(v) => std::env::set_var("XAVIER_CONFIG_PATH", v),
        None => std::env::remove_var("XAVIER_CONFIG_PATH"),
    }
    match prev_opencode_model {
        Some(v) => std::env::set_var("OPENCODE_MODEL", v),
        None => std::env::remove_var("OPENCODE_MODEL"),
    }
    match prev_xav_opencode_model {
        Some(v) => std::env::set_var("XAVIER_OPENCODE_MODEL", v),
        None => std::env::remove_var("XAVIER_OPENCODE_MODEL"),
    }
}
