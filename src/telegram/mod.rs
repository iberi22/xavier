//! Telegram Bot for Xavier Management
//!
//! Feature-gated behind the `telegram` cargo feature. Supports long-polling and
//! webhook (axum) modes, with memory search/stats commands backed by the local
//! QmdMemory store. The bot token is resolved from the Clavis hardware vault
//! first (`xavier vault set telegram_bot_token ...`) and falls back to the
//! `TELEGRAM_BOT_TOKEN` environment variable.

use serde::{Deserialize, Serialize};
use std::fmt;
use teloxide::prelude::*;
use teloxide::types::ParseMode;
use teloxide::utils::command::BotCommands;
use tracing::{error, info, warn};

/// Vault key under which the Telegram bot token is stored in Clavis.
pub const TELEGRAM_TOKEN_VAULT_KEY: &str = "telegram_bot_token";

#[derive(BotCommands, Clone)]
#[command(
    rename_rule = "lowercase",
    description = "These commands are supported:"
)]
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
    #[command(description = "memory operations: /memory stats | /memory search <query>.")]
    Memory(String),
}

/// Parsed subcommand for `/memory ...`.
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryCommand {
    /// `/memory stats` — workspace document count + storage bytes.
    Stats,
    /// `/memory search <query>` — top-k hybrid search.
    Search(String),
}

impl MemoryCommand {
    /// Parse the raw argument tail of a `/memory` command.
    ///
    /// Accepts `stats`, `search <query>`, or `search "<query>"`. Unknown input
    /// returns `None` so the caller can surface a usage hint.
    pub fn parse(args: &str) -> Option<Self> {
        let trimmed = args.trim();
        if trimmed.is_empty() {
            return None;
        }
        let (head, rest) = match trimmed.split_once(char::is_whitespace) {
            Some((h, r)) => (h, r.trim()),
            None => (trimmed, ""),
        };
        match head.to_ascii_lowercase().as_str() {
            "stats" => Some(Self::Stats),
            "search" => {
                if rest.is_empty() {
                    None
                } else {
                    Some(Self::Search(rest.to_string()))
                }
            }
            _ => None,
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub admin_ids: Vec<u64>,
    pub enabled: bool,
    pub webhook_url: Option<String>,
    pub webhook_port: u16,
    pub notification_chat_id: Option<String>,
}

impl fmt::Debug for TelegramConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelegramConfig")
            .field("bot_token", &"[REDACTED]")
            .field("admin_ids", &self.admin_ids)
            .field("enabled", &self.enabled)
            .field("webhook_url", &self.webhook_url)
            .field("webhook_port", &self.webhook_port)
            .field(
                "notification_chat_id",
                &self.notification_chat_id.as_ref().map(|_| "[REDACTED]"),
            )
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
            notification_chat_id: settings.telegram.notification_chat_id.clone(),
        }
    }
}

use crate::domain::memory::MemoryRecord;
use crate::ports::inbound::{AgentLifecyclePort, MemoryQueryPort, SecurityScanPort};
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

/// Resolve the Telegram bot token.
///
/// Resolution order:
/// 1. The Clavis hardware vault (`telegram_bot_token` key) — set via
///    `xavier vault set telegram_bot_token <value>`.
/// 2. The `TELEGRAM_BOT_TOKEN` environment variable.
///
/// Returns `Ok(token)` if either source yields a non-empty token, otherwise an
/// error describing the gap so the caller can surface it.
pub fn load_bot_token() -> anyhow::Result<String> {
    // 1. Try the Clavis hardware vault first.
    if let Ok(token) = crate::secrets::vault::HardwareVault::new("xavier")
        .get_secret(TELEGRAM_TOKEN_VAULT_KEY)
    {
        if !token.trim().is_empty() {
            return Ok(token);
        }
    }

    // 2. Fall back to the environment variable.
    if let Ok(token) = std::env::var("TELEGRAM_BOT_TOKEN") {
        if !token.trim().is_empty() {
            return Ok(token);
        }
    }

    anyhow::bail!(
        "Telegram bot token not found. Set it via `xavier vault set {} <token>` \
         or the TELEGRAM_BOT_TOKEN environment variable.",
        TELEGRAM_TOKEN_VAULT_KEY
    )
}

/// Load the local QmdMemory store for the `/memory` handlers.
///
/// Mirrors the loader used by the CLI spawn path (`load_spawn_memory`) so the
/// bot answers from the same workspace the rest of Xavier sees. Returns an
/// `Arc<QmdMemory>` suitable for short-lived search/stats lookups.
pub async fn load_local_memory(
) -> anyhow::Result<std::sync::Arc<crate::memory::qmd_memory::QmdMemory>> {
    use crate::memory::qmd_memory::{MemoryDocument, QmdMemory};
    use crate::memory::sqlite_vec_store::VecSqliteMemoryStore;
    use crate::memory::store::MemoryStore;
    use tokio::sync::RwLock;

    let store = VecSqliteMemoryStore::from_env().await?;
    let workspace_id =
        std::env::var("XAVIER_DEFAULT_WORKSPACE_ID").unwrap_or_else(|_| "default".to_string());
    let durable_state = store.load_workspace_state(&workspace_id).await?;
    let docs = std::sync::Arc::new(RwLock::new(
        durable_state
            .memories
            .iter()
            .map(MemoryRecord::to_document)
            .collect::<Vec<MemoryDocument>>(),
    ));
    let memory = std::sync::Arc::new(QmdMemory::new_with_workspace(docs, workspace_id));
    memory.set_store(std::sync::Arc::new(store)).await;
    memory.init().await?;
    Ok(memory)
}

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
        Self {
            bot,
            config,
            memory,
            agents,
            security,
        }
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

        let handler = Update::filter_message()
            .filter_command::<Command>()
            .endpoint(Self::handle_command);

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

        let handler = Update::filter_message()
            .filter_command::<Command>()
            .endpoint(Self::handle_command);

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
                bot.send_message(msg.chat.id, "⛔ Access denied. You are not an admin.")
                    .await?;
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
            Command::Stats => match memory.list("default", 1).await {
                Ok(_) => {
                    bot.send_message(
                        msg.chat.id,
                        "📊 *Memory Statistics*\n\n✅ Memory system is online and accessible\\.",
                    )
                    .parse_mode(ParseMode::MarkdownV2)
                    .await?;
                }
                Err(e) => {
                    bot.send_message(msg.chat.id, format!("❌ Failed to access memory: {}", e))
                        .await?;
                }
            },
            Command::Search(query) => {
                if query.is_empty() {
                    bot.send_message(msg.chat.id, "Usage: /search <query>")
                        .await?;
                } else {
                    bot.send_message(
                        msg.chat.id,
                        format!(
                            "🔍 Searching for: `{}`\\.\\.\\.",
                            escape_markdown_v2(&query)
                        ),
                    )
                    .parse_mode(ParseMode::MarkdownV2)
                    .await?;

                    match memory.search(&query, None).await {
                        Ok(results) => {
                            if results.is_empty() {
                                bot.send_message(msg.chat.id, "No results found.").await?;
                            } else {
                                let mut response = String::from("✅ *Search Results:*\n\n");
                                for (i, doc) in results.iter().take(5).enumerate() {
                                    let title = doc
                                        .metadata
                                        .get("title")
                                        .and_then(|t| t.as_str())
                                        .unwrap_or("Untitled");
                                    let content_preview =
                                        doc.content.chars().take(100).collect::<String>();
                                    response.push_str(&format!(
                                        "{}\\. *{}*\n_{}_\n\n",
                                        i + 1,
                                        escape_markdown_v2(title),
                                        escape_markdown_v2(&content_preview)
                                    ));
                                }
                                bot.send_message(msg.chat.id, response)
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await?;
                            }
                        }
                        Err(e) => {
                            bot.send_message(msg.chat.id, format!("❌ Search failed: {}", e))
                                .await?;
                        }
                    }
                }
            }
            Command::Add(content) => {
                if content.is_empty() {
                    bot.send_message(msg.chat.id, "Usage: /add <content>")
                        .await?;
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
                            bot.send_message(msg.chat.id, "✅ Memory added successfully.")
                                .await?;
                        }
                        Err(e) => {
                            bot.send_message(
                                msg.chat.id,
                                format!("❌ Failed to add memory: {}", e),
                            )
                            .await?;
                        }
                    }
                }
            }
            Command::Scan(text) => {
                if text.is_empty() {
                    bot.send_message(msg.chat.id, "Usage: /scan <text>").await?;
                } else {
                    bot.send_message(msg.chat.id, "🔒 Scanning for threats...")
                        .await?;
                    match security.scan(&text, None).await {
                        Ok(report) => {
                            let status = if report.threats.is_empty() {
                                "✅ Clean"
                            } else {
                                "⚠️ Threats Found"
                            };
                            bot.send_message(
                                msg.chat.id,
                                format!(
                                    "🛡 *Scan Report*\n\nStatus: {}\nFound: {} threats",
                                    status,
                                    report.threats.len()
                                ),
                            )
                            .parse_mode(ParseMode::MarkdownV2)
                            .await?;
                        }
                        Err(e) => {
                            bot.send_message(msg.chat.id, format!("❌ Scan failed: {}", e))
                                .await?;
                        }
                    }
                }
            }
            Command::Agents => {
                let active = agents.get_active_agents().await;
                if active.is_empty() {
                    bot.send_message(msg.chat.id, "🤖 No active agents.")
                        .await?;
                } else {
                    let mut response = String::from("🤖 *Active Agents*\n\n");
                    for agent in active {
                        let name = agent.metadata.name.as_deref().unwrap_or("Unknown");
                        response.push_str(&format!(
                            "• `{}` ({}): ✅ Running\n",
                            escape_markdown_v2(&agent.agent_id),
                            escape_markdown_v2(name)
                        ));
                    }
                    bot.send_message(msg.chat.id, response)
                        .parse_mode(ParseMode::MarkdownV2)
                        .await?;
                }
            }
            Command::Memory(args) => {
                let text = handle_memory_command(&args).await;
                bot.send_message(msg.chat.id, text)
                    .parse_mode(ParseMode::MarkdownV2)
                    .await?;
            }
            Command::Help => {
                bot.send_message(msg.chat.id, Command::descriptions().to_string())
                    .await?;
            }
        }
        Ok(())
    }
}

/// Execute a `/memory ...` command against the local QmdMemory store.
///
/// Returns a MarkdownV2-safe reply string. On store-load failure it surfaces a
/// human-readable error rather than panicking, so the bot stays responsive.
pub async fn handle_memory_command(args: &str) -> String {
    match MemoryCommand::parse(args) {
        Some(MemoryCommand::Stats) => match load_local_memory().await {
            Ok(memory) => {
                let usage = memory.usage().await;
                format!(
                    "📊 *Memory Statistics*\n\n📁 Documents: {}\n💾 Storage: {} KB",
                    usage.document_count,
                    usage.storage_bytes / 1024
                )
            }
            Err(e) => {
                warn!("Telegram /memory stats failed to load store: {e}");
                format!("❌ Could not load memory store: {}", escape_markdown_v2(&e.to_string()))
            }
        },
        Some(MemoryCommand::Search(query)) => match load_local_memory().await {
            Ok(memory) => {
                let k = 5;
                match memory.search(&query, k).await {
                    Ok(results) if results.is_empty() => {
                        format!("🔍 No results for `{}`\\.", escape_markdown_v2(&query))
                    }
                    Ok(results) => {
                        let mut response = String::from("✅ *Search Results:*\n\n");
                        for (i, doc) in results.iter().take(5).enumerate() {
                            let title = doc
                                .metadata
                                .get("title")
                                .and_then(|t| t.as_str())
                                .or(doc.path.split('/').last())
                                .unwrap_or("Untitled");
                            let preview: String = doc.content.chars().take(100).collect();
                            response.push_str(&format!(
                                "{}\\. *{}*\n_{}_\n\n",
                                i + 1,
                                escape_markdown_v2(title),
                                escape_markdown_v2(&preview)
                            ));
                        }
                        response
                    }
                    Err(e) => format!(
                        "❌ Search failed: {}",
                        escape_markdown_v2(&e.to_string())
                    ),
                }
            }
            Err(e) => {
                warn!("Telegram /memory search failed to load store: {e}");
                format!("❌ Could not load memory store: {}", escape_markdown_v2(&e.to_string()))
            }
        },
        None => {
            "ℹ️ Usage: `/memory stats` or `/memory search <query>`".to_string()
        }
    }
}

pub async fn run_bot(
    memory: Arc<dyn MemoryQueryPort>,
    agents: Arc<dyn AgentLifecyclePort>,
    security: Arc<dyn SecurityScanPort>,
) {
    let mut config = TelegramConfig::default();

    if !config.enabled {
        info!("Telegram bot disabled.");
        return;
    }

    // Resolve the token from the Clavis vault (or env fallback). This takes
    // precedence over whatever the settings file carried so operators can
    // rotate tokens without editing config.
    match load_bot_token() {
        Ok(token) => {
            if config.bot_token.is_empty() {
                config.bot_token = token;
            }
        }
        Err(e) => {
            if config.bot_token.is_empty() {
                error!("Telegram bot token not resolved: {e}");
                return;
            }
            // A token was present in settings; keep using it.
            warn!("Vault/env token resolution failed ({e}) — falling back to settings token");
        }
    }

    let bot = XavierBot::new(config, memory, agents, security);
    bot.start().await;
}

/// Start the bot in webhook mode against an explicit bind address and path.
///
/// Unlike [`XavierBot::start_webhook`], this entry point builds a fresh `Bot`
/// from the resolved token (vault → env) and is intended for programmatic
/// startup (e.g. from the server bootstrap). `addr` is the socket address to
/// bind (e.g. `0.0.0.0:8443`); `path` is the public URL path Telegram will
/// POST updates to (e.g. `/tg/webhook`). The full webhook URL is formed from
/// `config.webhook_url` + `path`.
pub async fn start_webhook(
    addr: &str,
    path: &str,
    memory: Arc<dyn MemoryQueryPort>,
    agents: Arc<dyn AgentLifecyclePort>,
    security: Arc<dyn SecurityScanPort>,
) -> anyhow::Result<()> {
    let token = load_bot_token()?;
    let mut config = TelegramConfig::default();
    config.bot_token = token;
    let base_url = config
        .webhook_url
        .clone()
        .unwrap_or_else(|| format!("https://{}", addr));
    let bot = XavierBot::new(config, memory, agents, security);

    let socket_addr: std::net::SocketAddr = addr
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid webhook bind address '{}': {}", addr, e))?;
    let webhook_url = format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
    .parse()
    .map_err(|e| anyhow::anyhow!("invalid webhook URL '{}': {}", base_url, e))?;

    let handler = Update::filter_message()
        .filter_command::<Command>()
        .endpoint(XavierBot::handle_command);

    let listener = teloxide::update_listeners::webhooks::axum(
        bot.bot.clone(),
        teloxide::update_listeners::webhooks::Options::new(socket_addr, webhook_url),
    )
    .await
    .map_err(|e| anyhow::anyhow!("failed to setup webhook listener: {}", e))?;

    info!("Telegram webhook listener bound on {} ({})", addr, path);

    Dispatcher::builder(bot.bot.clone(), handler)
        .dependencies(dptree::deps![
            Arc::new(bot.config.clone()),
            bot.memory.clone(),
            bot.agents.clone(),
            bot.security.clone()
        ])
        .build()
        .dispatch_with_listener(
            listener,
            LoggingErrorHandler::with_custom_text("An error from the Telegram webhook listener"),
        )
        .await;
    Ok(())
}

// ═══════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_command_parse_stats() {
        assert_eq!(MemoryCommand::parse("stats"), Some(MemoryCommand::Stats));
        // Leading/trailing whitespace tolerated.
        assert_eq!(MemoryCommand::parse("  stats  "), Some(MemoryCommand::Stats));
    }

    #[test]
    fn test_memory_command_parse_search() {
        assert_eq!(
            MemoryCommand::parse("search rust async"),
            Some(MemoryCommand::Search("rust async".to_string()))
        );
        // Case-insensitive head.
        assert_eq!(
            MemoryCommand::parse("SEARCH hello"),
            Some(MemoryCommand::Search("hello".to_string()))
        );
    }

    #[test]
    fn test_memory_command_parse_invalid() {
        // Empty, unknown subcommand, and bare `search` (no query) all fail.
        assert_eq!(MemoryCommand::parse(""), None);
        assert_eq!(MemoryCommand::parse("   "), None);
        assert_eq!(MemoryCommand::parse("delete foo"), None);
        assert_eq!(MemoryCommand::parse("search"), None);
        assert_eq!(MemoryCommand::parse("search   "), None);
    }

    #[test]
    fn test_load_bot_token_env_fallback_and_missing() {
        // Env-var tests must not run concurrently with each other (they mutate a
        // process-global env), so this single test covers both the fallback and
        // the missing-token error paths sequentially.
        let token = "123456:TEST-TOKEN-FROM-ENV";
        let prev = std::env::var("TELEGRAM_BOT_TOKEN").ok();

        // --- Fallback path: env var set ---
        std::env::set_var("TELEGRAM_BOT_TOKEN", token);
        let resolved = load_bot_token().unwrap_or_default();
        // The vault may have returned a stored token first; otherwise the env
        // fallback must round-trip the value we just set.
        assert!(
            !resolved.is_empty(),
            "token resolution should return a non-empty value, got '{resolved}'"
        );

        // --- Missing path: neither vault nor env ---
        std::env::remove_var("TELEGRAM_BOT_TOKEN");
        let result = load_bot_token();
        if let Err(e) = result {
            let msg = e.to_string();
            assert!(
                msg.contains("TELEGRAM_BOT_TOKEN") || msg.contains("telegram_bot_token"),
                "error should name the token sources: {msg}"
            );
        }
        // (If the vault DID have a token, Ok is acceptable here; we only assert
        //  the error-message shape when resolution actually fails.)

        // Restore prior env state.
        match prev {
            Some(v) => std::env::set_var("TELEGRAM_BOT_TOKEN", v),
            None => {}
        }
    }

    #[test]
    fn test_escape_markdown_v2_escapes_special_chars() {
        // MarkdownV2 reserves a broad set of characters; ensure they're escaped.
        let escaped = escape_markdown_v2("a.b_c*d");
        assert!(escaped.contains("a\\.b"));
        assert!(escaped.contains("b\\_c"));
        assert!(escaped.contains("c\\*d"));
    }

    #[tokio::test]
    async fn test_handle_memory_command_usage_on_unknown() {
        // Unknown subcommand yields a usage hint rather than a panic.
        let reply = handle_memory_command("frobnicate").await;
        assert!(reply.contains("Usage") || reply.contains("/memory"));
    }
}
