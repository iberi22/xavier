use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde_json::json;
use crate::agents::provider::config::ModelProviderConfig;

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
) -> Result<String> {
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
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01");

    if use_cache {
        if let Some(arr) = system_json.as_array_mut() {
            if let Some(first) = arr.get_mut(0) {
                if let Some(obj) = first.as_object_mut() {
                    obj.insert(
                        "cache_control".to_string(),
                        json!({"type": "ephemeral"}),
                    );
                }
            }
        }
        builder = builder.header("anthropic-beta", "prompt-caching-2024-07-31");
    }

    let response = builder
        .json(&json!({
            "model": config.model,
            "system": system_json,
            "max_tokens": 500,
            "temperature": 0.2,
            "messages": [
                {"role": "user", "content": user_prompt}
            ]
        }))
        .send()
        .await
        .context("failed to call Anthropic-compatible API")?
        .error_for_status()
        .context("Anthropic-compatible API returned an error")?;

    let payload: serde_json::Value = response
        .json()
        .await
        .context("failed to decode Anthropic-compatible response")?;
    payload["content"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["type"] == "text"))
        .and_then(|item| item["text"].as_str())
        .map(|text| text.to_string())
        .ok_or_else(|| anyhow!("Anthropic-compatible response did not contain text"))
}
