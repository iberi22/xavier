// SPDX-License-Identifier: MIT OR LICENSE-MESH
use crate::agents::provider::config::ModelProviderConfig;
use crate::agents::provider::types::LlmResponse;
use crate::domain::proxy::types::{ApiTier, ProviderKind, ProviderQuota};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use reqwest::Client;
use serde_json::json;

pub(crate) async fn generate_gemini_legacy(
    client: &Client,
    config: &ModelProviderConfig,
    system_prompt: &str,
    user_prompt: &str,
    _use_cache: bool,
) -> Result<LlmResponse> {
    let api_key = config.api_key.as_ref().context("missing Gemini API key")?;
    let endpoint = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        config.model, api_key
    );
    let response = client
        .post(endpoint)
        .json(&json!({
            "system_instruction": {
                "parts": [{"text": system_prompt}]
            },
            "contents": [{
                "role": "user",
                "parts": [{"text": user_prompt}]
            }],
            "generationConfig": {
                "temperature": 0.2,
                "maxOutputTokens": 500
            }
        }))
        .send()
        .await
        .context("failed to call Gemini API")?
        .error_for_status()
        .context("Gemini API returned an error")?;
    let payload: serde_json::Value = response
        .json()
        .await
        .context("failed to decode Gemini response")?;
    let text = payload["candidates"]
        .as_array()
        .and_then(|candidates| candidates.first())
        .and_then(|candidate| candidate["content"]["parts"].as_array())
        .and_then(|parts| parts.first())
        .and_then(|part| part["text"].as_str())
        .map(|text| text.to_string())
        .ok_or_else(|| anyhow!("Gemini response did not contain text"))?;

    // Gemini doesn't expose headers, estimate based on tier.
    // Defaulting to Free for now unless we have a way to know the tier.
    let api_tier = ApiTier::Free;
    let quota = Some(ProviderQuota {
        provider: ProviderKind::Gemini,
        api_tier,
        requests_remaining: None,
        tokens_remaining: None,
        requests_limit: Some(15), // Free tier estimate
        tokens_limit: Some(32768),
        resets_at: None,
        last_checked: Utc::now(),
    });

    Ok(LlmResponse { text, quota })
}
