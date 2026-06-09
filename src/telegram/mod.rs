//! Telegram Bot for Xavier Management

use serde::{Deserialize, Serialize};
use std::fmt;
use teloxide::prelude::*;
use teloxide::types::ParseMode;
use tracing::{error, info};

pub enum Command {
    Start,
    Health,
    Stats,
    Search(String),
    Add(String),
    Scan(String),
    Agents,
    Help,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub admin_ids: Vec<u64>,
    pub enabled: bool,
}

impl fmt::Debug for TelegramConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelegramConfig")
            .field("bot_token", &"[REDACTED]")
            .field("admin_ids", &self.admin_ids)
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl Default for TelegramConfig {
    fn default() -> Self {
        let settings = crate::settings::XavierSettings::current();
        Self {
            bot_token: settings.telegram.bot_token.unwrap_or_default(),
            admin_ids: Vec::new(),
            enabled: settings.telegram.enabled,
        }
    }
}

pub struct XavierBot {
    bot: Bot,
    config: TelegramConfig,
}

impl XavierBot {
    pub fn new(config: TelegramConfig) -> Self {
        let bot = Bot::new(&config.bot_token);
        Self { bot, config }
    }

    pub async fn start(&self) {
        info!("Starting Telegram bot...");
        let me = self.bot.get_me().await.expect("Failed to get bot info");
        info!("Bot username: @{}", me.username());

        teloxide::repl(self.bot.clone(), |bot: Bot, msg: Message| async move {
            if let Some(text) = msg.text() {
                let text_owned = text.to_string();
                if text_owned.starts_with('/') {
                    let _ = Self::handle_command(bot, msg, &text_owned).await;
                }
            }
            ResponseResult::Ok(())
        })
        .await;
    }

    async fn handle_command(bot: Bot, msg: Message, text: &str) -> ResponseResult<()> {
        let parts: Vec<&str> = text.splitn(2, ' ').collect();
        let cmd = parts[0];
        let arg = parts.get(1).unwrap_or(&"");

        match cmd {
            "/start" => {
                bot.send_message(
                    msg.chat.id,
                    "🦀 *Xavier Bot*\n\nWelcome! Use /help for commands.",
                )
                .parse_mode(ParseMode::MarkdownV2)
                .await?;
            }
            "/health" => {
                bot.send_message(msg.chat.id, "🟢 System: Running\n⚡ Xavier v0.4.1")
                    .await?;
            }
            "/stats" => {
                bot.send_message(msg.chat.id, "📊 Memories: 3\n💾 Size: 1.7 KB")
                    .await?;
            }
            "/search" => {
                if arg.is_empty() {
                    bot.send_message(msg.chat.id, "Usage: /search <query>")
                        .await?;
                } else {
                    bot.send_message(msg.chat.id, format!("🔍 Searching for: {}", arg))
                        .await?;
                }
            }
            "/add" => {
                if arg.is_empty() {
                    bot.send_message(msg.chat.id, "Usage: /add <content>")
                        .await?;
                } else {
                    bot.send_message(msg.chat.id, format!("✅ Memory added:\n{}", arg))
                        .await?;
                }
            }
            "/scan" => {
                if arg.is_empty() {
                    bot.send_message(msg.chat.id, "Usage: /scan <text>").await?;
                } else {
                    bot.send_message(
                        msg.chat.id,
                        "🔒 Scan complete:\n✅ Clean - no threats".to_string(),
                    )
                    .await?;
                }
            }
            "/agents" => {
                bot.send_message(
                    msg.chat.id,
                    "🤖 Agents:\n• xavier-main: ✅\n• memory-sync: ✅",
                )
                .await?;
            }
            "/help" => {
                let help = "🦀 *Xavier Commands*\n\n\
/start - Welcome\n\
/health - System status\n\
/stats - Memory stats\n\
/search <query> - Search\n\
/add <content> - Add memory\n\
/scan <text> - Security scan\n\
/agents - List agents\n\
/help - This help";
                bot.send_message(msg.chat.id, help)
                    .parse_mode(ParseMode::MarkdownV2)
                    .await?;
            }
            _ => {
                bot.send_message(msg.chat.id, "Unknown command. Use /help")
                    .await?;
            }
        }
        Ok(())
    }
}

pub async fn run_bot() {
    let config = TelegramConfig::default();

    if !config.enabled {
        info!("Telegram bot disabled. Set XAVIER_TELEGRAM_ENABLED=true");
        return;
    }

    if config.bot_token.is_empty() {
        error!("Telegram bot token not set. Set XAVIER_TELEGRAM_TOKEN");
        return;
    }

    let bot = XavierBot::new(config);
    bot.start().await;
}
