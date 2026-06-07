//! OpenAI API provider integration.
//!
//! Implements the LLM provider interface for OpenAI models (GPT-4, GPT-3.5),
//! handling API communication, streaming, and function calling.

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde_json::json;
use crate::agents::provider::config::ModelProviderConfig;

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

pub(crate) async fn generate_openai_compatible(
    client: &Client,
    config: &ModelProviderConfig,
    system_prompt: &str,
    user_prompt: &str,
    use_cache: bool,
) -> Result<String> {
    let base_url = config
        .base_url
        .as_ref()
        .context("missing OpenAI-compatible base URL")?;
    let endpoint = openai_chat_endpoint(base_url);
    let mut request = client
        .post(endpoint)
        .header("Content-Type", "application/json");

    if let Some(api_key) = config
        .api_key
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        request = request.bearer_auth(api_key);
    }

    let mut messages = vec![
        json!({"role": "system", "content": system_prompt}),
        json!({"role": "user", "content": user_prompt}),
    ];

    // DeepSeek prompt cache support (OpenAI compatible)
    if use_cache && config.provider_label == "deepseek" {
        if let Some(msg) = messages.get_mut(0) {
            if let Some(obj) = msg.as_object_mut() {
                obj.insert(
                    "cache_control".to_string(),
                    json!({"type": "ephemeral"}),
                );
            }
        }
    }

    let response = request
        .json(&json!({
            "model": config.model,
            "messages": messages,
            "temperature": 0.2,
            "max_tokens": 500
        }))
        .send()
        .await
        .context("failed to call OpenAI-compatible API")?
        .error_for_status()
        .context("OpenAI-compatible API returned an error")?;

    let payload: serde_json::Value = response
        .json()
        .await
        .context("failed to decode OpenAI-compatible response")?;
    payload["choices"]
        .as_array()
        .and_then(|choices| choices.first())
        .and_then(|choice| choice["message"]["content"].as_str())
        .map(|text| text.to_string())
        .ok_or_else(|| anyhow!("OpenAI-compatible response did not contain text"))
}
