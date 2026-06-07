//! MiniMax (Hailuo AI) API provider integration.
//!
//! Implements the LLM provider interface for MiniMax models,
//! supporting both text generation and function calling.

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde_json::json;
use crate::agents::provider::config::ModelProviderConfig;

pub(crate) async fn generate_minimax_legacy(
    client: &Client,
    config: &ModelProviderConfig,
    system_prompt: &str,
    user_prompt: &str,
    _use_cache: bool,
) -> Result<String> {
    let api_key = config
        .api_key
        .as_ref()
        .context("missing MiniMax API key")?;
    let base_url = config
        .base_url
        .as_ref()
        .context("missing MiniMax base URL")?
        .trim_end_matches('/');
    let endpoint = format!("{}/text/chatcompletion_pro", base_url);
    let response = client
        .post(endpoint)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&json!({
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt}
            ],
            "model": config.model,
            "temperature": 0.2,
            "max_tokens": 500
        }))
        .send()
        .await
        .context("failed to call MiniMax API")?
        .error_for_status()
        .context("MiniMax API returned an error")?;
    let payload: serde_json::Value = response
        .json()
        .await
        .context("failed to decode MiniMax response")?;
    payload["choices"]
        .as_array()
        .and_then(|choices| choices.first())
        .and_then(|choice| choice["message"]["content"].as_str())
        .map(|text| text.to_string())
        .ok_or_else(|| anyhow!("MiniMax response did not contain text"))
}
