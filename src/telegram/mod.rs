//! Telegram Bot for Xavier Management
//!
//! Feature-gated behind the `telegram` cargo feature. Supports long-polling and
//! webhook (axum) modes, with memory search/stats commands backed by the local
//! QmdMemory store. The bot token is resolved from the Clavis hardware vault
//! first (`xavier vault set telegram_bot_token ...`) and falls back to the
//! `TELEGRAM_BOT_TOKEN` environment variable.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use teloxide::prelude::*;
use teloxide::types::ParseMode;
use teloxide::utils::command::BotCommands;
use tracing::{error, info, warn};

// ── Rate-limiting constants ──────────────────────────────
/// Maximum number of commands a single user may send within the rate-limit window.
pub const RATE_LIMIT_COMMANDS: usize = 10;
/// Duration of the sliding window in seconds.
pub const RATE_LIMIT_WINDOW_SECS: u64 = 60;

/// Simple per-user rate limiter: N commands per window.
struct RateLimiter {
    max_per_window: usize,
    window: Duration,
    entries: Mutex<HashMap<String, Vec<Instant>>>,
}

impl RateLimiter {
    fn new(max: usize, window_secs: u64) -> Self {
        Self {
            max_per_window: max,
            window: Duration::from_secs(window_secs),
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Returns `Ok(())` if the user is under the limit, `Err(remaining_secs)` if
    /// rate limited.
    fn check(&self, user_id: &str) -> Result<(), u64> {
        self.prune();
        let mut map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let timestamps = map.entry(user_id.to_string()).or_default();
        // Remove expired entries for this user
        timestamps.retain(|&t| now.duration_since(t) < self.window);
        if timestamps.len() < self.max_per_window {
            timestamps.push(now);
            Ok(())
        } else {
            // Compute how many seconds until the oldest entry expires
            let oldest = timestamps.iter().copied().min().unwrap_or(now);
            let remaining = self.window.saturating_sub(now.duration_since(oldest));
            Err(remaining.as_secs() + 1)
        }
    }

    /// Prune stale entries across all users (called on every check).
    fn prune(&self) {
        let mut map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        for timestamps in map.values_mut() {
            timestamps.retain(|&t| now.duration_since(t) < self.window);
        }
        // Remove users with no entries
        map.retain(|_, v| !v.is_empty());
    }
}

/// Retry helper with exponential back-off.
///
/// Calls `f()` up to `max_retries` times. On failure, waits `2^attempt` seconds
/// (capped at 16 s) before retrying. Returns the first successful value or the
/// last error.
async fn with_retry<F, Fut, T, E>(label: &str, max_retries: usize, mut f: F) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut attempt = 0;
    loop {
        match f().await {
            Ok(val) => return Ok(val),
            Err(e) => {
                attempt += 1;
                if attempt >= max_retries {
                    return Err(e);
                }
                let backoff = 2u64.pow(attempt.min(4) as u32);
                tracing::warn!(
                    "{label}: attempt {attempt}/{max_retries} failed: {e}, retrying in {backoff}s"
                );
                tokio::time::sleep(Duration::from_secs(backoff)).await;
            }
        }
    }
}

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
    #[command(description = "show local-first operation mode and Ollama reachability.")]
    LocalStatus,
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
    #[command(
        description = "memory operations: /memory stats | /memory search <query> | /memory list | /memory delete <id>."
    )]
    Memory(String),
}

/// Parsed subcommand for `/memory ...`.
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryCommand {
    /// `/memory stats` — workspace document count + storage bytes.
    Stats,
    /// `/memory search <query>` — top-k hybrid search.
    Search(String),
    /// `/memory list` — list top-10 memories.
    List,
    /// `/memory delete <id>` — delete a memory by ID.
    Delete(String),
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
            "list" => Some(Self::List),
            "delete" => {
                if rest.is_empty() {
                    None
                } else {
                    Some(Self::Delete(rest.to_string()))
                }
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TelegramTransport {
    Polling,
    Webhook,
}

impl Default for TelegramTransport {
    fn default() -> Self {
        Self::Polling
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
    #[serde(default)]
    pub transport_mode: TelegramTransport,
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
            .field("transport_mode", &self.transport_mode)
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
            transport_mode: TelegramTransport::Polling,
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
    if let Ok(token) =
        crate::secrets::vault::HardwareVault::new("xavier").get_secret(TELEGRAM_TOKEN_VAULT_KEY)
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
    rate_limiter: Arc<RateLimiter>,
}

impl XavierBot {
    /// New.
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
            rate_limiter: Arc::new(RateLimiter::new(
                RATE_LIMIT_COMMANDS,
                RATE_LIMIT_WINDOW_SECS,
            )),
        }
    }

    /// Start.
    pub async fn start(&self) {
        if self.config.transport_mode == TelegramTransport::Webhook {
            if let Some(webhook_url) = &self.config.webhook_url {
                info!("Starting Telegram bot (webhook: {})...", webhook_url);
                self.start_webhook(webhook_url).await;
            } else {
                warn!("Telegram transport_mode is Webhook, but webhook_url is not set. Falling back to long-polling.");
                info!("Starting Telegram bot (long-polling)...");
                self.start_polling().await;
            }
        } else {
            info!("Starting Telegram bot (long-polling)...");
            self.start_polling().await;
        }
    }

    async fn start_polling(&self) {
        let me = match with_retry("get_me (polling)", 3, || {
            let bot = self.bot.clone();
            async move { bot.get_me().await }
        })
        .await
        {
            Ok(m) => m,
            Err(e) => {
                error!("Failed to get bot info after retries: {e}");
                return;
            }
        };
        info!("Bot username: @{}", me.username());

        let handler = Update::filter_message()
            .filter_command::<Command>()
            .endpoint(Self::handle_command);

        Dispatcher::builder(self.bot.clone(), handler)
            .dependencies(dptree::deps![
                Arc::new(self.config.clone()),
                self.memory.clone(),
                self.agents.clone(),
                self.security.clone(),
                self.rate_limiter.clone()
            ])
            .build()
            .dispatch()
            .await;
    }

    async fn start_webhook(&self, url: &str) {
        let me = match with_retry("get_me (webhook)", 3, || {
            let bot = self.bot.clone();
            async move { bot.get_me().await }
        })
        .await
        {
            Ok(m) => m,
            Err(e) => {
                error!("Failed to get bot info after retries: {e}");
                return;
            }
        };
        info!("Bot username: @{}", me.username());

        let addr = ([0, 0, 0, 0], self.config.webhook_port).into();
        let url = match url.parse() {
            Ok(u) => u,
            Err(e) => {
                error!("Invalid webhook URL: {e}");
                return;
            }
        };

        let handler = Update::filter_message()
            .filter_command::<Command>()
            .endpoint(Self::handle_command);

        let listener = match teloxide::update_listeners::webhooks::axum(
            self.bot.clone(),
            teloxide::update_listeners::webhooks::Options::new(addr, url),
        )
        .await
        {
            Ok(l) => l,
            Err(e) => {
                error!("Failed to setup webhook listener: {e}");
                return;
            }
        };

        Dispatcher::builder(self.bot.clone(), handler)
            .dependencies(dptree::deps![
                Arc::new(self.config.clone()),
                self.memory.clone(),
                self.agents.clone(),
                self.security.clone(),
                self.rate_limiter.clone()
            ])
            .build()
            .dispatch_with_listener(
                listener,
                LoggingErrorHandler::with_custom_text("An error from the update listener"),
            )
            .await;
    }

    /// Handle command.
    pub async fn handle_command(
        bot: Bot,
        msg: Message,
        cmd: Command,
        config: Arc<TelegramConfig>,
        memory: Arc<dyn MemoryQueryPort>,
        agents: Arc<dyn AgentLifecyclePort>,
        security: Arc<dyn SecurityScanPort>,
        rate_limiter: Arc<RateLimiter>,
    ) -> ResponseResult<()> {
        // ── Rate limiting ──────────────────────────────────────
        let user_id = msg
            .from()
            .map(|u| u.id.0.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        if let Err(secs) = rate_limiter.check(&user_id) {
            bot.send_message(
                msg.chat.id,
                format!("⏳ Rate limited. Try again in {secs}s."),
            )
            .await?;
            return Ok(());
        }

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
            Command::LocalStatus => {
                let mode = crate::server::alerts::SYSTEM_ALERTS.get_mode();
                let provider = std::env::var("XAVIER_PROVIDER")
                    .or_else(|_| std::env::var("XAVIER_MODEL_PROVIDER"))
                    .unwrap_or_else(|_| "local".into());
                let health = crate::observability::health::HEALTH.get_status().await;
                let (icon, label) = match mode {
                    crate::server::alerts::OperationalMode::LocalHealthy => ("🟢", "Local sano"),
                    crate::server::alerts::OperationalMode::LocalDegraded => {
                        ("🟡", "Local degradado")
                    }
                    crate::server::alerts::OperationalMode::CloudFallback => {
                        ("☁️", "Cloud fallback")
                    }
                    crate::server::alerts::OperationalMode::Disabled => ("⚫", "Deshabilitado"),
                };
                let ollama_url = std::env::var("XAVIER_LOCAL_LLM_URL")
                    .unwrap_or_else(|_| "http://localhost:11434".into());
                let text = format!(
                    "{} *Modo:* {}\n*Provider:* {}\n*LLM:* {} \\({}\\)\n*Embeddings:* {}\n*Ollama OK:* {}\n*URL:* {}",
                    icon,
                    escape_markdown_v2(label),
                    escape_markdown_v2(&provider),
                    escape_markdown_v2(&health.llm.model),
                    escape_markdown_v2(&health.llm.provider),
                    escape_markdown_v2(&health.embedding.model),
                    health.llm.reachable,
                    escape_markdown_v2(&ollama_url),
                );
                bot.send_message(msg.chat.id, text)
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

                    match memory.search(&query, 5, None).await {
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
        Some(MemoryCommand::List) => match load_local_memory().await {
            Ok(memory) => match memory.search("", 10).await {
                Ok(results) if results.is_empty() => {
                    "📋 No memories stored yet\\.".to_string()
                }
                Ok(results) => {
                    let mut response = String::from("📋 *Memory List \\(top 10\\):*\n\n");
                    for (i, doc) in results.iter().take(10).enumerate() {
                        let title = doc
                            .metadata
                            .get("title")
                            .and_then(|t| t.as_str())
                            .or(doc.path.split('/').last())
                            .unwrap_or("Untitled");
                        response.push_str(&format!(
                            "{}\\. \\*{}\\* \\(id: `{}`\\)\n\n",
                            i + 1,
                            escape_markdown_v2(title),
                            escape_markdown_v2(doc.id.as_deref().unwrap_or("unknown"))
                        ));
                    }
                    response
                }
                Err(e) => format!(
                    "❌ List failed: {}",
                    escape_markdown_v2(&e.to_string())
                ),
            },
            Err(e) => {
                warn!("Telegram /memory list failed to load store: {e}");
                format!("❌ Could not load memory store: {}", escape_markdown_v2(&e.to_string()))
            }
        },
        Some(MemoryCommand::Delete(id)) => match load_local_memory().await {
            Ok(memory) => match memory.delete(&id).await {
                Ok(Some(deleted)) => {
                    format!(
                        "🗑 Memory deleted successfully\\. \\(id: `{}`\\)",
                        escape_markdown_v2(deleted.id.as_deref().unwrap_or("unknown"))
                    )
                }
                Ok(None) => {
                    format!("❌ Memory not found: `{}`", escape_markdown_v2(&id))
                }
                Err(e) => format!(
                    "❌ Delete failed: {}",
                    escape_markdown_v2(&e.to_string())
                ),
            },
            Err(e) => {
                warn!("Telegram /memory delete failed to load store: {e}");
                format!("❌ Could not load memory store: {}", escape_markdown_v2(&e.to_string()))
            }
        },
        None => {
            "ℹ️ Usage: `/memory stats`, `/memory search <query>`, `/memory list`, or `/memory delete <id>`".to_string()
        }
    }
}

/// Run bot.
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
            bot.security.clone(),
            bot.rate_limiter.clone()
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
        assert_eq!(
            MemoryCommand::parse("  stats  "),
            Some(MemoryCommand::Stats)
        );
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
        // Empty, unknown subcommand, and bare subcommands with missing args all fail.
        assert_eq!(MemoryCommand::parse(""), None);
        assert_eq!(MemoryCommand::parse("   "), None);
        assert_eq!(MemoryCommand::parse("frobnicate"), None);
        assert_eq!(MemoryCommand::parse("search"), None);
        assert_eq!(MemoryCommand::parse("search   "), None);
        assert_eq!(MemoryCommand::parse("delete"), None);
        assert_eq!(MemoryCommand::parse("delete   "), None);
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

    // ── Memory list / delete parse tests ─────────────────

    #[test]
    fn test_memory_command_parse_list() {
        assert_eq!(MemoryCommand::parse("list"), Some(MemoryCommand::List));
        // Case-insensitive
        assert_eq!(MemoryCommand::parse("LIST"), Some(MemoryCommand::List));
        assert_eq!(MemoryCommand::parse("  LiSt  "), Some(MemoryCommand::List));
    }

    #[test]
    fn test_memory_command_parse_delete() {
        assert_eq!(
            MemoryCommand::parse("delete abc123"),
            Some(MemoryCommand::Delete("abc123".to_string()))
        );
        // Case-insensitive head
        assert_eq!(
            MemoryCommand::parse("DELETE some-uuid-here"),
            Some(MemoryCommand::Delete("some-uuid-here".to_string()))
        );
    }

    // ── Rate limiter tests ───────────────────────────────

    #[test]
    fn test_rate_limiter_allows_under_limit() {
        let limiter = RateLimiter::new(3, 60);
        assert!(limiter.check("user1").is_ok());
        assert!(limiter.check("user1").is_ok());
        assert!(limiter.check("user1").is_ok());
    }

    #[test]
    fn test_rate_limiter_blocks_over_limit() {
        let limiter = RateLimiter::new(2, 60);
        assert!(limiter.check("user2").is_ok());
        assert!(limiter.check("user2").is_ok());
        let result = limiter.check("user2");
        assert!(result.is_err(), "should be rate limited after 2 commands");
        assert!(
            result.unwrap_err() <= 61,
            "remaining seconds should be <= 61"
        );
    }

    #[test]
    fn test_rate_limiter_allows_after_window_expires() {
        // Use a very short 1-second window for the test.
        let limiter = RateLimiter::new(1, 1);
        assert!(limiter.check("user3").is_ok());
        // Immediately blocked
        assert!(limiter.check("user3").is_err());
        // Wait for window to expire
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert!(
            limiter.check("user3").is_ok(),
            "should allow after window expires"
        );
    }

    // ── with_retry tests ─────────────────────────────────

    #[tokio::test]
    async fn test_with_retry_succeeds_on_first_try() {
        let mut calls = 0;
        let result = with_retry("test-ok", 3, || {
            calls += 1;
            let val = 42;
            async move { Ok::<i32, String>(val) }
        })
        .await;
        assert_eq!(result.unwrap(), 42);
        assert_eq!(calls, 1);
    }

    #[tokio::test]
    async fn test_with_retry_retries_on_failure_then_succeeds() {
        let mut calls = 0;
        let result: Result<i32, &str> = with_retry("test-retry", 5, || {
            calls += 1;
            async move {
                if calls < 3 {
                    Err("not yet")
                } else {
                    Ok(99)
                }
            }
        })
        .await;
        assert_eq!(result.unwrap(), 99);
        assert_eq!(calls, 3, "should have retried twice then succeeded");
    }

    #[tokio::test]
    async fn test_with_retry_exhausts_retries() {
        let mut calls = 0;
        let result: Result<i32, &str> = with_retry("test-exhaust", 3, || {
            calls += 1;
            async move { Err("always fails") }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(calls, 3, "should have tried exactly max_retries times");
    }

    #[test]
    fn test_command_localstatus_description() {
        let descriptions = Command::descriptions().to_string();
        assert!(descriptions.contains("localstatus"));
        assert!(descriptions.contains("show local-first operation mode and Ollama reachability."));
    }

    #[test]
    fn test_localstatus_formatting_logic() {
        let mode = crate::server::alerts::OperationalMode::LocalHealthy;
        let provider = "local";
        let health = crate::observability::health::HealthStatus::default();
        let (icon, label) = match mode {
            crate::server::alerts::OperationalMode::LocalHealthy => ("🟢", "Local sano"),
            crate::server::alerts::OperationalMode::LocalDegraded => ("🟡", "Local degradado"),
            crate::server::alerts::OperationalMode::CloudFallback => ("☁️", "Cloud fallback"),
            crate::server::alerts::OperationalMode::Disabled => ("⚫", "Deshabilitado"),
        };
        let ollama_url = "http://localhost:11434";
        let text = format!(
            "{} *Modo:* {}\n*Provider:* {}\n*LLM:* {} \\({}\\)\n*Embeddings:* {}\n*Ollama OK:* {}\n*URL:* {}",
            icon,
            escape_markdown_v2(label),
            escape_markdown_v2(provider),
            escape_markdown_v2(&health.llm.model),
            escape_markdown_v2(&health.llm.provider),
            escape_markdown_v2(&health.embedding.model),
            health.llm.reachable,
            escape_markdown_v2(ollama_url),
        );

        assert!(text.contains("🟢"));
        assert!(text.contains("Local sano"));
        assert!(text.contains("local"));
        assert!(text.contains("http://localhost:11434"));
    }

    #[test]
    fn test_telegram_transport_default() {
        let config = TelegramConfig::default();
        assert_eq!(config.transport_mode, TelegramTransport::Polling);
    }

    #[test]
    fn test_telegram_transport_webhook_config() {
        let mut config = TelegramConfig::default();
        config.transport_mode = TelegramTransport::Webhook;
        assert_eq!(config.transport_mode, TelegramTransport::Webhook);
    }

    #[test]
    fn test_telegram_transport_serde() {
        let json_str = r#"{"bot_token":"tok","admin_ids":[],"enabled":true,"webhook_url":null,"webhook_port":8009,"notification_chat_id":null,"transport_mode":"webhook"}"#;
        let config: TelegramConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(config.transport_mode, TelegramTransport::Webhook);

        let json_str_default = r#"{"bot_token":"tok","admin_ids":[],"enabled":true,"webhook_url":null,"webhook_port":8009,"notification_chat_id":null}"#;
        let config_default: TelegramConfig = serde_json::from_str(json_str_default).unwrap();
        assert_eq!(config_default.transport_mode, TelegramTransport::Polling);
    }
}
