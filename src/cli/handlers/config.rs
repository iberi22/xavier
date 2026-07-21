// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Provider configuration handlers.

use axum::{extract::State, http::StatusCode, response::Response, Json};
use serde::{Deserialize, Serialize};
use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use xavier::settings::XavierSettings;
use xavier::agents::provider::ModelProviderClient;

#[derive(Debug, Serialize, Deserialize)]
pub struct ProviderConfigPayload {
    pub provider: String,
    pub model: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateProvidersPayload {
    pub providers: Vec<ProviderConfigPayload>,
}

pub async fn get_providers_config_handler() -> Response {
    let settings = XavierSettings::current();

    // We'll return a list of providers and their current settings from env/config
    let mut providers = Vec::new();
    let names = vec!["openai", "anthropic", "gemini", "minimax", "local"];

    for name in names {
        let client = ModelProviderClient::for_provider(name, None);
        providers.push(ProviderConfigPayload {
            provider: name.to_string(),
            model: client.config.model.clone(),
            api_key: client.config.api_key.as_ref().map(|_| "********".to_string()),
            base_url: client.config.base_url.clone(),
        });
    }

    json_response(
        StatusCode::OK,
        serde_json::to_value(UpdateProvidersPayload { providers }).unwrap(),
    )
}

pub async fn update_providers_config_handler(
    State(_state): State<CliState>,
    Json(payload): Json<UpdateProvidersPayload>,
) -> Response {
    // In a real implementation, we would save these to xavier.config.json
    // For now, we'll simulate success. XavierSettings currently loads from env/file.
    // Saving settings at runtime requires implementing a Save method for XavierSettings.

    for p in payload.providers {
        let prefix = p.provider.to_uppercase();
        if let Some(key) = p.api_key {
            if !key.contains("********") {
                std::env::set_var(format!("{}_API_KEY", prefix), key);
            }
        }
        std::env::set_var(format!("XAVIER_{}_MODEL", prefix), p.model);
        if let Some(url) = p.base_url {
            std::env::set_var(format!("XAVIER_{}_URL", prefix), url);
        }
    }

    json_response(
        StatusCode::OK,
        serde_json::json!({ "status": "ok", "message": "Providers updated in memory" }),
    )
}

pub async fn test_provider_handler(
    State(_state): State<CliState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Response {
    let client = ModelProviderClient::for_provider(&name, None);

    match client.generate_text("You are a connectivity tester.", "Say 'ok'").await {
        Ok(_) => json_response(
            StatusCode::OK,
            serde_json::json!({ "status": "ok", "message": format!("Connection to {} successful", name) }),
        ),
        Err(e) => json_response(
            StatusCode::BAD_GATEWAY,
            serde_json::json!({ "status": "error", "message": e.to_string() }),
        ),
    }
}
