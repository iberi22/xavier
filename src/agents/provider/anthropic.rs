use crate::agents::provider::config::ModelProviderConfig;
use crate::agents::provider::types::LlmResponse;
use crate::domain::proxy::types::{ApiTier, ProviderKind, ProviderQuota};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use reqwest::Client;
use serde_json::json;

/// Anthropic messages endpoint.
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

/// Generate anthropic compatible.
pub(crate) async fn generate_anthropic_compatible(
    client: &Client,
    config: &ModelProviderConfig,
    system_prompt: &str,
    user_prompt: &str,
    use_cache: bool,
) -> Result<LlmResponse> {
    let base_url = config
        .get_resolved_base_url()
        .as_ref()
        .context("missing Anthropic-compatible base URL")?
        .clone();
    let api_key = config
        .api_key
        .as_ref()
        .context("missing Anthropic-compatible API key")?;
    let endpoint = anthropic_messages_endpoint(&base_url);

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

    let mut body = request_body(&config.model, system_json, user_prompt);

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
    let status = response.status();
    if !status.is_success() {
        let detail = response.text().await.unwrap_or_default();
        return Err(anyhow!(
            "Anthropic-compatible API returned {status}: {}",
            api_error_message(&detail)
        ));
    }

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

/// Request body for the Messages API. Claude 5 family models reject `temperature` and
/// think before answering, so they get room for thinking plus the answer.
fn request_body(model: &str, system: serde_json::Value, user_prompt: &str) -> serde_json::Value {
    let max_tokens = if accepts_temperature(model) {
        500
    } else {
        4096
    };
    let mut body = json!({
        "model": model,
        "system": system,
        "max_tokens": max_tokens,
        "messages": [
            {"role": "user", "content": user_prompt}
        ]
    });
    if accepts_temperature(model) {
        body["temperature"] = json!(0.2);
    }
    body
}

/// False for Claude models with major version 5 or later (`claude-haiku-5-5`, `claude-opus-5`).
fn accepts_temperature(model: &str) -> bool {
    let Some(rest) = model.strip_prefix("claude-") else {
        return true;
    };
    let major = rest.split('-').find_map(|part| part.parse::<u32>().ok());
    !matches!(major, Some(major) if major >= 5)
}

/// The API's `error.message`, or a short excerpt of the raw body.
fn api_error_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| body.chars().take(300).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_5_family_omits_temperature() {
        for model in [
            "claude-haiku-5-5",
            "claude-sonnet-5-5",
            "claude-opus-5",
            "claude-fable-5-1",
        ] {
            let body = request_body(model, json!([]), "hi");
            assert!(
                body.get("temperature").is_none(),
                "{model} must not send temperature"
            );
        }
    }

    #[test]
    fn claude_5_family_gets_room_for_thinking_and_answer() {
        assert_eq!(
            request_body("claude-haiku-5-5", json!([]), "hi")["max_tokens"],
            json!(4096)
        );
        assert_eq!(
            request_body("claude-sonnet-4-5", json!([]), "hi")["max_tokens"],
            json!(500)
        );
    }

    #[test]
    fn older_and_compatible_models_keep_temperature() {
        for model in [
            "claude-3-5-sonnet-latest",
            "claude-sonnet-4-5",
            "glm-4.6",
            "kimi-k2",
        ] {
            let body = request_body(model, json!([]), "hi");
            assert_eq!(body["temperature"], json!(0.2), "{model} keeps temperature");
        }
    }

    #[test]
    fn api_error_message_prefers_the_error_field() {
        let body = r#"{"type":"error","error":{"type":"invalid_request_error","message":"`temperature` is deprecated for this model."}}"#;
        assert_eq!(
            api_error_message(body),
            "`temperature` is deprecated for this model."
        );
        assert_eq!(api_error_message("plain failure"), "plain failure");
    }
}
