//! Cloud Node settings handlers

use crate::messaging::DiscordClient;
use crate::secrets::vault::HardwareVault;
use crate::settings::XavierSettings;
use axum::{response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct CloudNodeConfig {
    pub url: Option<String>,
    pub token: Option<String>,
    pub instance_id: Option<String>,
}

pub async fn get_cloud_node() -> impl IntoResponse {
    let settings = XavierSettings::current();
    let config = CloudNodeConfig {
        url: settings.pgheart.url,
        token: settings
            .pgheart
            .token
            .as_ref()
            .map(|_| "********".to_string()),
        instance_id: settings.pgheart.instance_id,
    };
    Json(serde_json::json!({ "status": "ok", "data": config }))
}

pub async fn update_cloud_node(Json(payload): Json<CloudNodeConfig>) -> impl IntoResponse {
    let mut settings = XavierSettings::current();

    if let Some(url) = payload.url {
        settings.pgheart.url = Some(url);
    }

    if let Some(token) = payload.token {
        if !token.contains("********") {
            settings.pgheart.token = Some(token);
        }
    }

    if let Some(instance_id) = payload.instance_id {
        settings.pgheart.instance_id = Some(instance_id);
    }

    match settings.save().await {
        Ok(_) => {
            Json(serde_json::json!({ "status": "ok", "message": "Cloud node settings updated" }))
        }
        Err(e) => Json(serde_json::json!({ "status": "error", "message": e.to_string() })),
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DiscordConfigPayload {
    pub enabled: Option<bool>,
    pub webhook_url: Option<String>,
    pub bot_token: Option<String>,
    pub rate_limit_per_min: Option<u32>,
}

pub async fn get_discord_settings() -> impl IntoResponse {
    let settings = XavierSettings::current();
    let config = DiscordConfigPayload {
        enabled: Some(settings.discord.enabled),
        webhook_url: settings
            .discord
            .webhook_url
            .as_ref()
            .map(|_| "********".to_string()),
        bot_token: settings
            .discord
            .bot_token
            .as_ref()
            .map(|_| "********".to_string()),
        rate_limit_per_min: Some(settings.discord.rate_limit_per_min),
    };
    Json(serde_json::json!({ "status": "ok", "data": config }))
}

pub async fn update_discord_settings(
    Json(payload): Json<DiscordConfigPayload>,
) -> impl IntoResponse {
    let mut settings = XavierSettings::current();

    if let Some(enabled) = payload.enabled {
        settings.discord.enabled = enabled;
    }

    if let Some(url) = payload.webhook_url {
        if !url.contains("********") {
            // Save to hardware vault for extra security
            let vault = HardwareVault::new("xavier-discord");
            let _ = vault.store_secret("webhook_url", &url);
            settings.discord.webhook_url = Some(url);
        }
    }

    if let Some(token) = payload.bot_token {
        if !token.contains("********") {
            let vault = HardwareVault::new("xavier-discord");
            let _ = vault.store_secret("bot_token", &token);
            settings.discord.bot_token = Some(token);
        }
    }

    if let Some(limit) = payload.rate_limit_per_min {
        settings.discord.rate_limit_per_min = limit;
    }

    match settings.save().await {
        Ok(_) => Json(serde_json::json!({ "status": "ok", "message": "Discord settings updated" })),
        Err(e) => Json(serde_json::json!({ "status": "error", "message": e.to_string() })),
    }
}

pub async fn test_discord_connection() -> impl IntoResponse {
    let settings = XavierSettings::current();

    let webhook_url = settings.discord.webhook_url.clone();
    let rate_limit = settings.discord.rate_limit_per_min;

    let client = DiscordClient::new(webhook_url, rate_limit);
    match client.test_connection().await {
        Ok(_) => {
            Json(serde_json::json!({ "status": "ok", "message": "Discord connection successful" }))
        }
        Err(e) => Json(
            serde_json::json!({ "status": "error", "message": format!("Discord test failed: {}", e) }),
        ),
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TelegramConfigPayload {
    pub enabled: Option<bool>,
    pub bot_token: Option<String>,
    pub webhook_url: Option<String>,
    pub webhook_port: Option<u16>,
    pub admin_ids: Option<Vec<u64>>,
    pub notification_chat_id: Option<String>,
}

pub async fn get_telegram_settings() -> impl IntoResponse {
    let settings = XavierSettings::current();
    let config = TelegramConfigPayload {
        enabled: Some(settings.telegram.enabled),
        bot_token: settings
            .telegram
            .bot_token
            .as_ref()
            .map(|_| "********".to_string()),
        webhook_url: settings
            .telegram
            .webhook_url
            .as_ref()
            .map(|_| "********".to_string()),
        webhook_port: Some(settings.telegram.webhook_port),
        admin_ids: Some(settings.telegram.admin_ids.clone()),
        notification_chat_id: settings
            .telegram
            .notification_chat_id
            .as_ref()
            .map(|_| "********".to_string()),
    };
    Json(serde_json::json!({ "status": "ok", "data": config }))
}

pub async fn update_telegram_settings(
    Json(payload): Json<TelegramConfigPayload>,
) -> impl IntoResponse {
    let mut settings = XavierSettings::current();

    if let Some(enabled) = payload.enabled {
        settings.telegram.enabled = enabled;
    }

    if let Some(token) = payload.bot_token {
        if !token.contains("********") {
            let vault = HardwareVault::new("xavier-telegram");
            let _ = vault.store_secret("bot_token", &token);
            settings.telegram.bot_token = Some(token);
        }
    }

    if let Some(url) = payload.webhook_url {
        if !url.contains("********") {
            let vault = HardwareVault::new("xavier-telegram");
            let _ = vault.store_secret("webhook_url", &url);
            settings.telegram.webhook_url = Some(url);
        }
    }

    if let Some(port) = payload.webhook_port {
        settings.telegram.webhook_port = port;
    }

    if let Some(ids) = payload.admin_ids {
        settings.telegram.admin_ids = ids;
    }

    if let Some(chat_id) = payload.notification_chat_id {
        if !chat_id.contains("********") {
            let vault = HardwareVault::new("xavier-telegram");
            let _ = vault.store_secret("notification_chat_id", &chat_id);
            settings.telegram.notification_chat_id = Some(chat_id);
        }
    }

    match settings.save().await {
        Ok(_) => {
            Json(serde_json::json!({ "status": "ok", "message": "Telegram settings updated" }))
        }
        Err(e) => Json(serde_json::json!({ "status": "error", "message": e.to_string() })),
    }
}

pub async fn test_telegram_connection() -> impl IntoResponse {
    #[cfg(feature = "telegram")]
    {
        let settings = XavierSettings::current();
        let token = match settings.telegram.bot_token {
            Some(t) => t,
            None => {
                return Json(
                    serde_json::json!({ "status": "error", "message": "Bot token not set" }),
                )
            }
        };
        use teloxide::prelude::*;
        let bot = Bot::new(token);
        match bot.get_me().await {
            Ok(me) => Json(
                serde_json::json!({ "status": "ok", "message": format!("Telegram connection successful: @{}", me.username()) }),
            ),
            Err(e) => Json(
                serde_json::json!({ "status": "error", "message": format!("Telegram test failed: {}", e) }),
            ),
        }
    }
    #[cfg(not(feature = "telegram"))]
    {
        Json(
            serde_json::json!({ "status": "error", "message": "Telegram feature not enabled in build" }),
        )
    }
}
