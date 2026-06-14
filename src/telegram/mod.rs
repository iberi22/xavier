//! Telegram Bot for Xavier Management

use serde::{Deserialize, Serialize};
use std::fmt;
use teloxide::prelude::*;
use teloxide::types::ParseMode;
use teloxide::utils::command::BotCommands;
use tracing::{error, info};

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase", description = "These commands are supported:")]
pub enum Command {
    #[command(description = "display this text.")]
    Help,
    #[command(description = "start the bot.")]
    Start,
    #[command(description = "check system health.")]
    Health,
    #[command(description = "show memory statistics.")]
    Stats,
    #[command(description = "search memories.")]
    Search(String),
    #[command(description = "add a new memory.")]
    Add(String),
    #[command(description = "perform a security scan.")]
    Scan(String),
    #[command(description = "list active agents.")]
    Agents,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub admin_ids: Vec<u64>,
    pub enabled: bool,
    pub webhook_url: Option<String>,
    pub webhook_port: u16,
}

impl fmt::Debug for TelegramConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelegramConfig")
            .field("bot_token", &"[REDACTED]")
            .field("admin_ids", &self.admin_ids)
            .field("enabled", &self.enabled)
            .field("webhook_url", &self.webhook_url)
            .field("webhook_port", &self.webhook_port)
            .finish()
    }
}

impl Default for TelegramConfig {
    fn default() -> Self {
        let settings = crate::settings::XavierSettings::current();
        Self {
            bot_token: settings.telegram.bot_token.clone().unwrap_or_default(),
            admin_ids: settings.telegram.admin_ids.clone(),
            enabled: settings.telegram.enabled,
            webhook_url: settings.telegram.webhook_url.clone(),
            webhook_port: settings.telegram.webhook_port,
        }
    }
}

use std::sync::Arc;
use crate::ports::inbound::{MemoryQueryPort, AgentLifecyclePort, SecurityScanPort};
use crate::domain::memory::MemoryRecord;
use serde_json::json;
use uuid::Uuid;
use chrono::Utc;

fn escape_markdown_v2(text: &str) -> String {
    let escapes = [
        "_", "*", "[", "]", "(", ")", "~", "`", ">", "#", "+", "-", "=", "|", "{", "}", ".", "!",
    ];
    let mut escaped = text.to_string();
    for e in escapes {
        escaped = escaped.replace(e, &format!("\\{}", e));
    }
    escaped
}

pub struct XavierBot {
    bot: Bot,
    config: TelegramConfig,
    memory: Arc<dyn MemoryQueryPort>,
    agents: Arc<dyn AgentLifecyclePort>,
    security: Arc<dyn SecurityScanPort>,
}

impl XavierBot {
    pub fn new(
        config: TelegramConfig,
        memory: Arc<dyn MemoryQueryPort>,
        agents: Arc<dyn AgentLifecyclePort>,
        security: Arc<dyn SecurityScanPort>,
    ) -> Self {
        let bot = Bot::new(&config.bot_token);
        Self { bot, config, memory, agents, security }
    }

    pub async fn start(&self) {
        if let Some(webhook_url) = &self.config.webhook_url {
            info!("Starting Telegram bot (webhook: {})...", webhook_url);
            self.start_webhook(webhook_url).await;
        } else {
            info!("Starting Telegram bot (long-polling)...");
            self.start_polling().await;
        }
    }

    async fn start_polling(&self) {
        let me = self.bot.get_me().await.expect("Failed to get bot info");
        info!("Bot username: @{}", me.username());

        let handler = Update::filter_message().filter_command::<Command>().endpoint(Self::handle_command);

        Dispatcher::builder(self.bot.clone(), handler)
            .dependencies(dptree::deps![
                Arc::new(self.config.clone()),
                self.memory.clone(),
                self.agents.clone(),
                self.security.clone()
            ])
            .build()
            .dispatch()
            .await;
    }

    async fn start_webhook(&self, url: &str) {
        let me = self.bot.get_me().await.expect("Failed to get bot info");
        info!("Bot username: @{}", me.username());

        let addr = ([0, 0, 0, 0], self.config.webhook_port).into();
        let url = url.parse().expect("Invalid webhook URL");

        let handler = Update::filter_message().filter_command::<Command>().endpoint(Self::handle_command);

        let listener = teloxide::update_listeners::webhooks::axum(
            self.bot.clone(),
            teloxide::update_listeners::webhooks::Options::new(addr, url),
        )
        .await
        .expect("Failed to setup webhook listener");

        Dispatcher::builder(self.bot.clone(), handler)
            .dependencies(dptree::deps![
                Arc::new(self.config.clone()),
                self.memory.clone(),
                self.agents.clone(),
                self.security.clone()
            ])
            .build()
            .dispatch_with_listener(
                listener,
                LoggingErrorHandler::with_custom_text("An error from the update listener"),
            )
            .await;
    }

    pub async fn handle_command(
        bot: Bot,
        msg: Message,
        cmd: Command,
        config: Arc<TelegramConfig>,
        memory: Arc<dyn MemoryQueryPort>,
        agents: Arc<dyn AgentLifecyclePort>,
        security: Arc<dyn SecurityScanPort>,
    ) -> ResponseResult<()> {
        // Simple admin check
        if !config.admin_ids.is_empty() {
            let user_id = msg.from().map(|u| u.id.0).unwrap_or(0);
            if !config.admin_ids.contains(&user_id) {
                bot.send_message(msg.chat.id, "⛔ Access denied. You are not an admin.").await?;
                return Ok(());
            }
        }

        match cmd {
            Command::Start => {
                bot.send_message(
                    msg.chat.id,
                    "🦀 *Xavier Bot*\n\nWelcome\\! I am your cognitive memory interface\\. Use /help for available commands\\.",
                )
                .parse_mode(ParseMode::MarkdownV2)
                .await?;
            }
            Command::Health => {
                let version = escape_markdown_v2(env!("CARGO_PKG_VERSION"));
                bot.send_message(
                    msg.chat.id,
                    format!(
                        "🟢 *Xavier System Status*\n\n✅ Service: Online\n⚡ Version: v{}\n📡 Bot: Connected",
                        version
                    ),
                )
                .parse_mode(ParseMode::MarkdownV2)
                .await?;
            }
            Command::Stats => {
                match memory.list("default", 1).await {
                    Ok(_) => {
                        bot.send_message(
                            msg.chat.id,
                            "📊 *Memory Statistics*\n\n✅ Memory system is online and accessible\\.",
                        )
                        .parse_mode(ParseMode::MarkdownV2)
                        .await?;
                    }
                    Err(e) => {
                        bot.send_message(msg.chat.id, format!("❌ Failed to access memory: {}", e)).await?;
                    }
                }
            }
            Command::Search(query) => {
                if query.is_empty() {
                    bot.send_message(msg.chat.id, "Usage: /search <query>").await?;
                } else {
                    bot.send_message(msg.chat.id, format!("🔍 Searching for: `{}`\\.\\.\\.", escape_markdown_v2(&query)))
                        .parse_mode(ParseMode::MarkdownV2)
                        .await?;

                    match memory.search(&query, None).await {
                        Ok(results) => {
                            if results.is_empty() {
                                bot.send_message(msg.chat.id, "No results found.").await?;
                            } else {
                                let mut response = String::from("✅ *Search Results:*\n\n");
                                for (i, doc) in results.iter().take(5).enumerate() {
                                    let title = doc.metadata.get("title").and_then(|t| t.as_str()).unwrap_or("Untitled");
                                    let content_preview = doc.content.chars().take(100).collect::<String>();
                                    response.push_str(&format!("{}\\. *{}*\n_{}_\n\n", i + 1, escape_markdown_v2(title), escape_markdown_v2(&content_preview)));
                                }
                                bot.send_message(msg.chat.id, response)
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await?;
                            }
                        }
                        Err(e) => {
                            bot.send_message(msg.chat.id, format!("❌ Search failed: {}", e)).await?;
                        }
                    }
                }
            }
            Command::Add(content) => {
                if content.is_empty() {
                    bot.send_message(msg.chat.id, "Usage: /add <content>").await?;
                } else {
                    let record = MemoryRecord {
                        id: Uuid::new_v4().to_string(),
                        workspace_id: "default".to_string(),
                        content: content.clone(),
                        metadata: json!({"source": "telegram"}),
                        created_at: Utc::now(),
                        updated_at: Utc::now(),
                        ..Default::default()
                    };

                    match memory.add(record).await {
                        Ok(_) => {
                            bot.send_message(msg.chat.id, "✅ Memory added successfully.").await?;
                        }
                        Err(e) => {
                            bot.send_message(msg.chat.id, format!("❌ Failed to add memory: {}", e)).await?;
                        }
                    }
                }
            }
            Command::Scan(text) => {
                if text.is_empty() {
                    bot.send_message(msg.chat.id, "Usage: /scan <text>").await?;
                } else {
                    bot.send_message(msg.chat.id, "🔒 Scanning for threats...").await?;
                    match security.scan(&text, None).await {
                        Ok(report) => {
                            let status = if report.threats.is_empty() { "✅ Clean" } else { "⚠️ Threats Found" };
                            bot.send_message(
                                msg.chat.id,
                                format!("🛡 *Scan Report*\n\nStatus: {}\nFound: {} threats", status, report.threats.len()),
                            )
                            .parse_mode(ParseMode::MarkdownV2)
                            .await?;
                        }
                        Err(e) => {
                            bot.send_message(msg.chat.id, format!("❌ Scan failed: {}", e)).await?;
                        }
                    }
                }
            }
            Command::Agents => {
                let active = agents.get_active_agents().await;
                if active.is_empty() {
                    bot.send_message(msg.chat.id, "🤖 No active agents.").await?;
                } else {
                    let mut response = String::from("🤖 *Active Agents*\n\n");
                    for agent in active {
                        let name = agent.metadata.name.as_deref().unwrap_or("Unknown");
                        response.push_str(&format!("• `{}` ({}): ✅ Running\n", escape_markdown_v2(&agent.agent_id), escape_markdown_v2(name)));
                    }
                    bot.send_message(msg.chat.id, response)
                        .parse_mode(ParseMode::MarkdownV2)
                        .await?;
                }
            }
            Command::Help => {
                bot.send_message(msg.chat.id, Command::descriptions().to_string()).await?;
            }
        }
        Ok(())
    }
}

pub async fn run_bot(
    memory: Arc<dyn MemoryQueryPort>,
    agents: Arc<dyn AgentLifecyclePort>,
    security: Arc<dyn SecurityScanPort>,
) {
    let config = TelegramConfig::default();

    if !config.enabled {
        info!("Telegram bot disabled.");
        return;
    }

    if config.bot_token.is_empty() {
        error!("Telegram bot token not set.");
        return;
    }

    let bot = XavierBot::new(config, memory, agents, security);
    bot.start().await;
}
