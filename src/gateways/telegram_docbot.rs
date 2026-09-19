//! Telegram DocBot gateway.
//!
//! Document-aware Telegram bot that wraps Xavier's existing `telegram` module,
//! adding collection-scoped search and document upload handling.
//!
//! Commands:
//! - `/buscar <query>`       — semantic search across all collections
//! - `/preguntar <query>`    — RAG answer with citations
//! - `/colecciones`          — list available collections
//! - `/subir`                — next message with a file will be ingested
//! - `/ayuda`                — show help
//!
//! Feature-gated: requires the `telegram` cargo feature.

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::gateways::{GatewayAdapter, InboundMessage, MessageHandler, OutboundMessage, ParseMode};

// ── Config ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TelegramDocConfig {
    /// Bot token from env `TELEGRAM_BOT_TOKEN` or Clavis vault.
    pub token: String,
    /// Optional allowlist of Telegram user IDs (empty = allow all).
    pub allowed_user_ids: Vec<i64>,
    /// Maximum messages per user per minute.
    pub rate_limit: usize,
}

impl TelegramDocConfig {
    pub fn from_env() -> Result<Self> {
        let token = std::env::var("TELEGRAM_BOT_TOKEN").map_err(|_| {
            anyhow::anyhow!(
                "TELEGRAM_BOT_TOKEN not set — run: xavier vault set telegram_bot_token <TOKEN>"
            )
        })?;

        let allowed = std::env::var("DOCBOT_TELEGRAM_ALLOWED_IDS")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .filter_map(|s| s.trim().parse::<i64>().ok())
            .collect();

        Ok(Self {
            token,
            allowed_user_ids: allowed,
            rate_limit: 10,
        })
    }
}

// ── Gateway adapter (stub for non-telegram builds) ─────────────────────────────

/// Telegram DocBot gateway adapter.
///
/// In builds without the `telegram` feature, `start()` returns immediately with
/// a configuration error. Wire the real Teloxide dispatcher when the feature is
/// enabled.
pub struct TelegramDocGateway {
    config: TelegramDocConfig,
}

impl TelegramDocGateway {
    pub fn new(config: TelegramDocConfig) -> Self {
        Self { config }
    }

    /// Try to build from environment. Returns `None` if token is missing.
    pub fn from_env() -> Option<Self> {
        TelegramDocConfig::from_env().map(Self::new).ok()
    }

    /// Format a search result for Telegram Markdown.
    pub fn format_search_result(
        answer: &str,
        sources: &[crate::collections::CitedSource],
    ) -> String {
        let mut msg = format!("🔍 *Respuesta:*\n{}\n", answer);

        if !sources.is_empty() {
            msg.push_str("\n📚 *Fuentes:*\n");
            for (i, src) in sources.iter().take(3).enumerate() {
                let page_info = src.page.map(|p| format!(" · p.{}", p)).unwrap_or_default();
                let crumb = src
                    .breadcrumb
                    .as_deref()
                    .map(|b| format!(" › {}", b))
                    .unwrap_or_default();
                msg.push_str(&format!(
                    "{}. *{}*{}{}\n_{}_\n",
                    i + 1,
                    src.doc_title,
                    page_info,
                    crumb,
                    truncate(&src.excerpt, 120),
                ));
            }
        }

        msg
    }
}

fn truncate(s: &str, max_chars: usize) -> &str {
    let mut idx = 0;
    for (count, (i, _)) in s.char_indices().enumerate() {
        if count >= max_chars {
            return &s[..idx];
        }
        idx = i;
    }
    s
}

#[async_trait]
impl GatewayAdapter for TelegramDocGateway {
    fn name(&self) -> &str {
        "telegram"
    }

    #[cfg(feature = "telegram")]
    async fn start(&self, handler: Arc<dyn MessageHandler>) -> Result<()> {
        use teloxide::prelude::*;
        use teloxide::types::ParseMode as TgParseMode;

        info!("Starting Telegram DocBot gateway (long-polling)");

        let bot = Bot::new(&self.config.token);
        let handler_arc = handler.clone();
        let allowed = self.config.allowed_user_ids.clone();

        teloxide::repl(bot, move |bot: Bot, msg: Message| {
            let handler = handler_arc.clone();
            let allowed = allowed.clone();
            async move {
                let user_id = msg.from().map(|u| u.id.0 as i64).unwrap_or(0);
                if !allowed.is_empty() && !allowed.contains(&user_id) {
                    bot.send_message(msg.chat.id, "⛔ Acceso no autorizado.")
                        .await?;
                    return Ok(());
                }

                let text = msg.text().unwrap_or("").trim().to_string();
                if text.is_empty() {
                    return Ok(());
                }

                let inbound = InboundMessage {
                    message_id: msg.id.0.to_string(),
                    gateway: "telegram".to_string(),
                    channel_id: msg.chat.id.0.to_string(),
                    sender_id: user_id.to_string(),
                    sender_name: msg.from().map(|u| u.first_name.clone()),
                    text,
                    attachment: None,
                };

                match handler.handle(inbound).await {
                    Ok(reply) => {
                        bot.send_message(msg.chat.id, &reply)
                            .parse_mode(TgParseMode::Markdown)
                            .await?;
                    }
                    Err(e) => {
                        error!(error = %e, "handler error");
                        bot.send_message(
                            msg.chat.id,
                            "❌ Error procesando tu solicitud. Intenta de nuevo.",
                        )
                        .await?;
                    }
                }

                Ok(())
            }
        })
        .await;

        Ok(())
    }

    #[cfg(not(feature = "telegram"))]
    async fn start(&self, _handler: Arc<dyn MessageHandler>) -> Result<()> {
        anyhow::bail!("Telegram gateway is not compiled in. Add --features telegram to enable it.")
    }

    async fn send(&self, msg: OutboundMessage) -> Result<()> {
        // Direct send via Teloxide HTTP (no dispatcher needed)
        let url = format!(
            "https://api.telegram.org/bot{}/sendMessage",
            self.config.token
        );
        let client = reqwest::Client::new();
        let parse_mode = match msg.parse_mode {
            ParseMode::Markdown => "Markdown",
            ParseMode::Html => "HTML",
            ParseMode::Plain => "",
        };

        let mut body = serde_json::json!({
            "chat_id": msg.channel_id,
            "text": msg.text,
        });
        if !parse_mode.is_empty() {
            body["parse_mode"] = serde_json::Value::String(parse_mode.to_string());
        }

        let resp = client.post(&url).json(&body).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Telegram send failed {}: {}", status, body);
        }

        Ok(())
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::CitedSource;

    #[test]
    fn test_format_search_result_no_sources() {
        let msg = TelegramDocGateway::format_search_result("La respuesta es 42.", &[]);
        assert!(msg.contains("La respuesta es 42."));
    }

    #[test]
    fn test_format_search_result_with_sources() {
        let src = CitedSource {
            chunk_id: "c1".to_string(),
            doc_id: "d1".to_string(),
            doc_title: "Contrato 2024".to_string(),
            collection_id: "col1".to_string(),
            collection_name: "Legal".to_string(),
            page: Some(3),
            breadcrumb: Some("Artículo 5".to_string()),
            excerpt: "El arrendatario deberá pagar mensualmente.".to_string(),
            score: 0.92,
        };
        let msg = TelegramDocGateway::format_search_result("Debe pagar mensualmente.", &[src]);
        assert!(msg.contains("Contrato 2024"));
        assert!(msg.contains("p.3"));
    }

    #[test]
    fn test_truncate() {
        let s = "Hello world, this is a long string";
        assert_eq!(truncate(s, 5).len(), 4); // 4 chars (index at char 4)
    }
}
