//! Provider configuration handlers.

use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use axum::{extract::State, http::StatusCode, response::Response, Json};
use serde::{Deserialize, Serialize};
use xavier::agents::provider::ModelProviderClient;
use xavier::settings::XavierSettings;

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

/// Get providers config handler.
pub async fn get_providers_config_handler() -> Response {
    let _settings = XavierSettings::current();

    // We'll return a list of providers and their current settings from env/config
    let mut providers = Vec::new();
    let names = vec!["openai", "anthropic", "gemini", "minimax", "local"];

    for name in names {
        let client = ModelProviderClient::for_provider(name, None);
        providers.push(ProviderConfigPayload {
            provider: name.to_string(),
            model: client.config.model.clone(),
            api_key: client
                .config
                .api_key
                .as_ref()
                .map(|_| "********".to_string()),
            base_url: client.config.base_url.clone(),
        });
    }

    json_response(
        StatusCode::OK,
        serde_json::to_value(UpdateProvidersPayload { providers }).unwrap(),
    )
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChannelStatus {
    pub configured: bool,
    pub active: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MessagingStatusResponse {
    pub discord: ChannelStatus,
    pub slack: ChannelStatus,
    pub telegram: ChannelStatus,
    pub whatsapp: ChannelStatus,
    pub teams: ChannelStatus,
}

/// Get messaging channels active/configured status.
pub async fn get_messaging_status_handler() -> Response {
    let settings = XavierSettings::current();

    let telegram_configured = settings.telegram.bot_token.is_some() || settings.telegram.enabled;
    let telegram_active = settings.telegram.enabled && settings.telegram.bot_token.is_some();

    let discord_configured = settings.discord.webhook_url.is_some()
        || settings.discord.bot_token.is_some()
        || settings.discord.enabled;
    let discord_active = settings.discord.enabled
        && (settings.discord.webhook_url.is_some() || settings.discord.bot_token.is_some());

    let slack_configured =
        std::env::var("SLACK_BOT_TOKEN").is_ok() || std::env::var("SLACK_WEBHOOK_URL").is_ok();
    let slack_active = slack_configured;

    let whatsapp_configured = std::env::var("WHATSAPP_TOKEN").is_ok();
    let whatsapp_active = whatsapp_configured;

    let teams_configured = std::env::var("TEAMS_WEBHOOK_URL").is_ok();
    let teams_active = teams_configured;

    let response = MessagingStatusResponse {
        discord: ChannelStatus {
            configured: discord_configured,
            active: discord_active,
        },
        slack: ChannelStatus {
            configured: slack_configured,
            active: slack_active,
        },
        telegram: ChannelStatus {
            configured: telegram_configured,
            active: telegram_active,
        },
        whatsapp: ChannelStatus {
            configured: whatsapp_configured,
            active: whatsapp_active,
        },
        teams: ChannelStatus {
            configured: teams_configured,
            active: teams_active,
        },
    };

    json_response(StatusCode::OK, serde_json::to_value(response).unwrap())
}

/// Update providers config handler.
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

/// Test provider handler.
pub async fn test_provider_handler(
    State(_state): State<CliState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Response {
    let client = ModelProviderClient::for_provider(&name, None);

    match client
        .generate_text("You are a connectivity tester.", "Say 'ok'")
        .await
    {
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
