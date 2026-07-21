// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Discord messaging integration for Xavier

use crate::middleware::token_bucket::RateLimiter;
use crate::secrets::vault::HardwareVault;
use anyhow::{Context, Result};
use serde::Serialize;
use std::sync::Arc;
use tracing::{debug, error, info};

/// Discord client for sending messages via webhook
#[derive(Clone)]
pub struct DiscordClient {
    webhook_url: String,
    limiter: Arc<RateLimiter>,
}

#[derive(Serialize)]
struct DiscordEmbed {
    title: Option<String>,
    description: String,
    color: Option<u32>,
    timestamp: Option<String>,
}

#[derive(Serialize)]
struct DiscordWebhookPayload {
    content: Option<String>,
    embeds: Vec<DiscordEmbed>,
    username: String,
    avatar_url: Option<String>,
}

impl DiscordClient {
    /// Create a new Discord client with the given webhook URL and rate limit
    pub fn new(webhook_url_opt: Option<String>, rate_limit_per_min: u32) -> Self {
        let webhook_url = webhook_url_opt.unwrap_or_else(|| {
            let vault = HardwareVault::new("xavier-discord");
            vault.get_secret("webhook_url").unwrap_or_default()
        });

        let fill_rate = rate_limit_per_min as f64 / 60.0;
        let limiter = Arc::new(RateLimiter::new(rate_limit_per_min as f64, fill_rate));

        Self {
            webhook_url,
            limiter,
        }
    }

    /// Send a message to Discord via webhook in embed format
    pub async fn send_embed(
        &self,
        title: Option<String>,
        description: String,
        color: Option<u32>,
    ) -> Result<()> {
        // Check rate limit
        if !self.limiter.try_consume(1.0).await {
            let wait = self.limiter.retry_after(1.0).await;
            error!("Discord rate limit exceeded, wait {}ms", wait.as_millis());
            return Err(anyhow::anyhow!("Rate limit exceeded"));
        }

        let payload = DiscordWebhookPayload {
            content: None,
            embeds: vec![DiscordEmbed {
                title,
                description,
                color,
                timestamp: Some(chrono::Utc::now().to_rfc3339()),
            }],
            username: "Xavier".to_string(),
            avatar_url: None, // Could be configurable later
        };

        let client = reqwest::Client::new();
        let response = client
            .post(&self.webhook_url)
            .json(&payload)
            .send()
            .await
            .context("Failed to send Discord webhook request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("Discord webhook error: {} - {}", status, body);
            return Err(anyhow::anyhow!("Discord API error: {} - {}", status, body));
        }

        debug!("Discord message sent successfully");
        Ok(())
    }

    /// Test the connection to the Discord webhook
    pub async fn test_connection(&self) -> Result<()> {
        info!("Testing Discord webhook connection...");
        self.send_embed(
            Some("🔌 Connection Test".to_string()),
            "Xavier Discord integration is active and connected.".to_string(),
            Some(0x39ff14), // Xavier Green
        )
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_discord_payload_serialization() {
        let payload = DiscordWebhookPayload {
            content: None,
            embeds: vec![DiscordEmbed {
                title: Some("Test".into()),
                description: "Desc".into(),
                color: Some(0xFFFFFF),
                timestamp: Some("2025-01-01T00:00:00Z".into()),
            }],
            username: "Xavier".into(),
            avatar_url: None,
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"title\":\"Test\""));
        assert!(json.contains("\"description\":\"Desc\""));
    }

    #[tokio::test]
    async fn test_discord_client_new() {
        let client = DiscordClient::new(Some("http://mock.url".into()), 30);
        assert_eq!(client.webhook_url, "http://mock.url");
    }
}
