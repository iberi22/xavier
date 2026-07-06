use crate::agents::provider::config::ModelProviderConfig;
use crate::agents::provider::types::LlmResponse;
use crate::domain::proxy::types::{ApiTier, ProviderKind, ProviderQuota};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use reqwest::Client;
use serde_json::json;

pub(crate) fn anthropic_messages_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/messages") {
        trimmed.to_string()
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/messages")
    } else {
        format!("{trimmed}/v1/messages")
    }
}

pub(crate) async fn generate_anthropic_compatible(
    client: &Client,
    config: &ModelProviderConfig,
    system_prompt: &str,
    user_prompt: &str,
    use_cache: bool,
) -> Result<LlmResponse> {
    let base_url = config
        .base_url
        .as_ref()
        .context("missing Anthropic-compatible base URL")?;
    let api_key = config
        .api_key
        .as_ref()
        .context("missing Anthropic-compatible API key")?;
    let endpoint = anthropic_messages_endpoint(base_url);

    let mut system_json = json!([
        {
            "type": "text",
            "text": system_prompt,
        }
    ]);

    let mut builder = client
        .post(endpoint)
        .header("anthropic-version", "2023-06-01");

    use crate::domain::proxy::SecretInjectionStrategy;
    match config
        .secret_injection_strategy
        .as_ref()
        .unwrap_or(&SecretInjectionStrategy::XApiKey)
    {
        SecretInjectionStrategy::BearerToken => {
            builder = builder.bearer_auth(api_key);
        }
        SecretInjectionStrategy::XApiKey => {
            builder = builder.header("x-api-key", api_key);
        }
        SecretInjectionStrategy::GitHubToken => {
            builder = builder.header("Authorization", format!("token {}", api_key));
        }
    }

    if use_cache {
        if let Some(arr) = system_json.as_array_mut() {
            if let Some(first) = arr.get_mut(0) {
                if let Some(obj) = first.as_object_mut() {
                    obj.insert("cache_control".to_string(), json!({"type": "ephemeral"}));
                }
            }
        }
        builder = builder.header("anthropic-beta", "prompt-caching-2024-07-31");
    }

    let mut body = json!({
        "model": config.model,
        "system": system_json,
        "max_tokens": 500,
        "temperature": 0.2,
        "messages": [
            {"role": "user", "content": user_prompt}
        ]
    });

    if let Some(token) = &config.lease_token {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("lease_token".to_string(), json!(token));
        }
    }

    let response = builder
        .json(&body)
        .send()
        .await
        .context("failed to call Anthropic-compatible API")?;

    let headers = response.headers().clone();
    let response = response
        .error_for_status()
        .context("Anthropic-compatible API returned an error")?;

    let payload: serde_json::Value = response
        .json()
        .await
        .context("failed to decode Anthropic-compatible response")?;

    let text = payload["content"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["type"] == "text"))
        .and_then(|item| item["text"].as_str())
        .map(|text| text.to_string())
        .ok_or_else(|| anyhow!("Anthropic-compatible response did not contain text"))?;

    let mut quota = None;

    let rem_req = headers
        .get("anthropic-ratelimit-requests-remaining")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    let rem_tok = headers
        .get("anthropic-ratelimit-tokens-remaining")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    let limit_req = headers
        .get("anthropic-ratelimit-requests-limit")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    let limit_tok = headers
        .get("anthropic-ratelimit-tokens-limit")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    if rem_req.is_some() || rem_tok.is_some() {
        let rpm = limit_req.unwrap_or(0);
        let api_tier = ApiTier::from_rpm(rpm);

        quota = Some(ProviderQuota {
            provider: ProviderKind::Anthropic,
            api_tier,
            requests_remaining: rem_req,
            tokens_remaining: rem_tok,
            requests_limit: limit_req,
            tokens_limit: limit_tok,
            resets_at: None, // Anthropic resets are complicated to parse from headers
            last_checked: Utc::now(),
        });
    }

    Ok(LlmResponse { text, quota })
}
