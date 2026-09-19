//! WhatsApp Cloud API gateway for Xavier DocBot.
//!
//! Receives inbound messages via Meta's webhook (POST /webhooks/whatsapp)
//! and sends replies through the Cloud API.
//!
//! Required env vars:
//! - `DOCBOT_WHATSAPP_TOKEN`          — permanent system user token
//! - `DOCBOT_WHATSAPP_PHONE_NUMBER_ID` — sender phone number ID
//! - `DOCBOT_WHATSAPP_VERIFY_TOKEN`   — webhook verification secret

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::gateways::{
    GatewayAdapter, InboundMessage, MessageAttachment, MessageHandler, OutboundMessage,
};

const WA_API_BASE: &str = "https://graph.facebook.com/v20.0";

// ── Config ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WhatsAppConfig {
    pub token: String,
    pub phone_number_id: String,
    pub verify_token: String,
}

impl WhatsAppConfig {
    pub fn from_env() -> Result<Self> {
        let token = std::env::var("DOCBOT_WHATSAPP_TOKEN")
            .map_err(|_| anyhow::anyhow!("DOCBOT_WHATSAPP_TOKEN not set"))?;
        let phone_id = std::env::var("DOCBOT_WHATSAPP_PHONE_NUMBER_ID")
            .map_err(|_| anyhow::anyhow!("DOCBOT_WHATSAPP_PHONE_NUMBER_ID not set"))?;
        let verify = std::env::var("DOCBOT_WHATSAPP_VERIFY_TOKEN")
            .unwrap_or_else(|_| "xavier_docbot".to_string());
        Ok(Self {
            token,
            phone_number_id: phone_id,
            verify_token: verify,
        })
    }
}

// ── Webhook payload types ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct WaWebhookPayload {
    pub entry: Vec<WaEntry>,
}

#[derive(Debug, Deserialize)]
pub struct WaEntry {
    pub changes: Vec<WaChange>,
}

#[derive(Debug, Deserialize)]
pub struct WaChange {
    pub value: WaValue,
}

#[derive(Debug, Deserialize)]
pub struct WaValue {
    pub messages: Option<Vec<WaMessage>>,
    pub metadata: Option<WaMetadata>,
}

#[derive(Debug, Deserialize)]
pub struct WaMetadata {
    pub phone_number_id: String,
}

#[derive(Debug, Deserialize)]
pub struct WaMessage {
    pub id: String,
    pub from: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub text: Option<WaText>,
    pub document: Option<WaDocument>,
    pub audio: Option<WaMediaRef>,
    pub image: Option<WaMediaRef>,
}

#[derive(Debug, Deserialize)]
pub struct WaText {
    pub body: String,
}

#[derive(Debug, Deserialize)]
pub struct WaDocument {
    pub id: String,
    pub filename: Option<String>,
    pub mime_type: String,
}

#[derive(Debug, Deserialize)]
pub struct WaMediaRef {
    pub id: String,
    pub mime_type: String,
}

// ── Gateway Adapter ────────────────────────────────────────────────────────────

pub struct WhatsAppGateway {
    config: WhatsAppConfig,
    client: reqwest::Client,
}

impl WhatsAppGateway {
    pub fn new(config: WhatsAppConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    pub fn from_env() -> Option<Self> {
        WhatsAppConfig::from_env().map(Self::new).ok()
    }

    /// Parse a webhook payload into InboundMessages.
    pub fn parse_payload(payload: &WaWebhookPayload) -> Vec<InboundMessage> {
        let mut messages = Vec::new();

        for entry in &payload.entry {
            for change in &entry.changes {
                if let Some(msgs) = &change.value.messages {
                    for msg in msgs {
                        let text = match msg.kind.as_str() {
                            "text" => msg
                                .text
                                .as_ref()
                                .map(|t| t.body.clone())
                                .unwrap_or_default(),
                            "document" => "[documento adjunto]".to_string(),
                            "audio" => "[audio adjunto]".to_string(),
                            "image" => "[imagen adjunta]".to_string(),
                            _ => continue,
                        };

                        if text.is_empty() {
                            continue;
                        }

                        messages.push(InboundMessage {
                            message_id: msg.id.clone(),
                            gateway: "whatsapp".to_string(),
                            channel_id: msg.from.clone(),
                            sender_id: msg.from.clone(),
                            sender_name: None,
                            text,
                            attachment: None, // populated by media download if needed
                        });
                    }
                }
            }
        }

        messages
    }

    /// Send a text message via WhatsApp Cloud API.
    async fn send_text(&self, to: &str, text: &str) -> Result<()> {
        let url = format!("{}/{}/messages", WA_API_BASE, self.config.phone_number_id);

        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "to": to,
            "type": "text",
            "text": { "body": text }
        });

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.config.token)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("WhatsApp send failed {}: {}", status, body);
        }

        Ok(())
    }

    /// Download media by ID from the Cloud API (for document ingestion).
    pub async fn download_media(&self, media_id: &str) -> Result<Vec<u8>> {
        // Step 1: get download URL
        let info_url = format!("{}/{}", WA_API_BASE, media_id);
        let info: serde_json::Value = self
            .client
            .get(&info_url)
            .bearer_auth(&self.config.token)
            .send()
            .await?
            .json()
            .await?;

        let download_url = info["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("no URL in media info"))?;

        // Step 2: download bytes
        let bytes = self
            .client
            .get(download_url)
            .bearer_auth(&self.config.token)
            .send()
            .await?
            .bytes()
            .await?;

        Ok(bytes.to_vec())
    }
}

#[async_trait]
impl GatewayAdapter for WhatsAppGateway {
    fn name(&self) -> &str {
        "whatsapp"
    }

    /// WhatsApp uses webhooks — this method registers the webhook endpoint
    /// with Meta's Cloud API and then waits (the actual message handling is
    /// done via the Axum route `/webhooks/whatsapp`).
    async fn start(&self, _handler: Arc<dyn MessageHandler>) -> Result<()> {
        info!(
            phone_number_id = %self.config.phone_number_id,
            "WhatsApp gateway ready — waiting for webhook events at /webhooks/whatsapp"
        );
        // Webhook routing is handled by the Axum server; nothing to block here.
        Ok(())
    }

    async fn send(&self, msg: OutboundMessage) -> Result<()> {
        self.send_text(&msg.channel_id, &msg.text).await
    }

    async fn health(&self) -> bool {
        // Probe the Cloud API with a lightweight GET
        let url = format!("{}/{}", WA_API_BASE, self.config.phone_number_id);
        self.client
            .get(&url)
            .bearer_auth(&self.config.token)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_payload() -> WaWebhookPayload {
        serde_json::from_str(
            r#"
        {
            "entry": [{
                "changes": [{
                    "value": {
                        "messages": [{
                            "id": "msg-1",
                            "from": "521234567890",
                            "type": "text",
                            "text": { "body": "¿Cuál es el artículo 5?" }
                        }]
                    }
                }]
            }]
        }
        "#,
        )
        .unwrap()
    }

    #[test]
    fn test_parse_text_message() {
        let payload = sample_payload();
        let msgs = WhatsAppGateway::parse_payload(&payload);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "¿Cuál es el artículo 5?");
        assert_eq!(msgs[0].gateway, "whatsapp");
        assert_eq!(msgs[0].sender_id, "521234567890");
    }

    #[test]
    fn test_parse_empty_messages() {
        let payload: WaWebhookPayload = serde_json::from_str(
            r#"
        {"entry": [{"changes": [{"value": {"messages": null}}]}]}
        "#,
        )
        .unwrap();
        let msgs = WhatsAppGateway::parse_payload(&payload);
        assert!(msgs.is_empty());
    }
}
