use crate::agents::provider::config::ModelProviderConfig;
use crate::agents::provider::types::{LlmResponse, ProviderMode};
use crate::domain::proxy::types::{ApiTier, ProviderKind, ProviderQuota};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use reqwest::Client;
use serde_json::json;

/// Openai chat endpoint.
pub(crate) fn openai_chat_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/chat/completions")
    } else {
        format!("{trimmed}/v1/chat/completions")
    }
}

/// Generate openai compatible.
pub(crate) async fn generate_openai_compatible(
    client: &Client,
    config: &ModelProviderConfig,
    system_prompt: &str,
    user_prompt: &str,
    use_cache: bool,
) -> Result<LlmResponse> {
    let base_url = config
        .get_resolved_base_url()
        .as_ref()
        .context("missing OpenAI-compatible base URL")?
        .clone();
    let endpoint = openai_chat_endpoint(&base_url);
    let mut request = client
        .post(endpoint)
        .header("Content-Type", "application/json");

    let api_key_to_use =
        if config.provider_mode == ProviderMode::Local || config.provider_mode == ProviderMode::ManagedLocal {
            std::env::var("OLLAMA_API_KEY")
                .ok()
                .or_else(|| std::env::var("XAVIER_LOCAL_LLM_API_KEY").ok())
                .or_else(|| {
                    config.api_key.as_ref().and_then(|k| {
                        if k == "ollama" {
                            None
                        } else {
                            Some(k.clone())
                        }
                    })
                })
        } else {
            config.api_key.clone()
        };

    if let Some(api_key) = api_key_to_use
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        use crate::domain::proxy::SecretInjectionStrategy;
        match config
            .secret_injection_strategy
            .as_ref()
            .unwrap_or(&SecretInjectionStrategy::BearerToken)
        {
            SecretInjectionStrategy::BearerToken => {
                request = request.bearer_auth(api_key);
            }
            SecretInjectionStrategy::XApiKey => {
                request = request.header("X-API-Key", api_key);
            }
            SecretInjectionStrategy::GitHubToken => {
                request = request.header("Authorization", format!("token {}", api_key));
            }
        }
    }

    let mut messages = vec![
        json!({"role": "system", "content": system_prompt}),
        json!({"role": "user", "content": user_prompt}),
    ];

    // DeepSeek prompt cache support (OpenAI compatible)
    if use_cache && config.provider_label == "deepseek" {
        if let Some(msg) = messages.get_mut(0) {
            if let Some(obj) = msg.as_object_mut() {
                obj.insert("cache_control".to_string(), json!({"type": "ephemeral"}));
            }
        }
    }

    let mut body = json!({
        "model": config.model,
        "messages": messages,
        "temperature": 0.2,
        "max_tokens": 500
    });

    if let Some(token) = &config.lease_token {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("lease_token".to_string(), json!(token));
        }
    }

    let response = request
        .json(&body)
        .send()
        .await
        .context("failed to call OpenAI-compatible API")?;

    let headers = response.headers().clone();
    let response = response
        .error_for_status()
        .context("OpenAI-compatible API returned an error")?;

    let payload: serde_json::Value = response
        .json()
        .await
        .context("failed to decode OpenAI-compatible response")?;

    let text = payload["choices"]
        .as_array()
        .and_then(|choices| choices.first())
        .and_then(|choice| choice["message"]["content"].as_str())
        .map(|text| text.to_string())
        .ok_or_else(|| anyhow!("OpenAI-compatible response did not contain text"))?;

    let mut quota = None;
    let provider_kind = ProviderKind::from_str(&config.provider_label);

    let rem_req = headers
        .get("x-ratelimit-remaining-requests")
        .or_else(|| headers.get("x-ratelimit-remaining"))
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    let rem_tok = headers
        .get("x-ratelimit-remaining-tokens")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    let limit_req = headers
        .get("x-ratelimit-limit-requests")
        .or_else(|| headers.get("x-ratelimit-limit"))
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    let limit_tok = headers
        .get("x-ratelimit-limit-tokens")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    let reset_req = headers
        .get("x-ratelimit-reset-requests")
        .or_else(|| headers.get("x-ratelimit-reset"))
        .and_then(|v| v.to_str().ok());

    if rem_req.is_some() || rem_tok.is_some() {
        let resets_at = reset_req.and_then(|r| {
            if r.contains('s') || r.contains('m') || r.contains('h') {
                // Handle duration format if some providers use it
                None
            } else {
                // Assume timestamp or seconds
                r.parse::<u64>().ok().map(|s| {
                    if s > 1_000_000_000 {
                        // Likely a timestamp
                        chrono::DateTime::from_timestamp(s as i64, 0).unwrap_or_default()
                    } else {
                        // Likely seconds until reset
                        Utc::now() + chrono::Duration::seconds(s as i64)
                    }
                })
            }
        });

        let rpm = limit_req.unwrap_or(0);
        let api_tier = ApiTier::from_rpm(rpm);

        quota = Some(ProviderQuota {
            provider: provider_kind,
            api_tier,
            requests_remaining: rem_req,
            tokens_remaining: rem_tok,
            requests_limit: limit_req,
            tokens_limit: limit_tok,
            resets_at,
            last_checked: Utc::now(),
        });
    }

    Ok(LlmResponse { text, quota })
}
