//! # Notifier
//!
//! Sends notifications about errors, fixes, and system events
//! via Telegram (already configured) and other channels.
//!
//! ## Channels
//!
//! - **Telegram** → immediate notifications to the configured bot
//! - **tracing!** → logs to stderr + file (always on)
//! - **(Future)** Webhook, email, Discord

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use tokio::sync::broadcast;

use super::analyzer::{ErrorDiagnosis, Urgency};
use super::fixer::FixerResult;
use crate::messaging::DiscordClient;
use crate::settings::XavierSettings;
#[cfg(feature = "tauri")]
use crate::utils::tauri_utils::get_tauri_app_handle;
#[cfg(feature = "tauri")]
use tauri::Emitter;

/// Capacity of the notification event bus. Late/lagging subscribers (e.g. the
/// Tauri frontend) will observe a `RecvError::Lagged(k)` for the `k` missed
/// events rather than blocking the notifier.
const EVENT_BUS_CAPACITY: usize = 256;

/// Module-level notification event bus (broadcast channel).
///
/// Held in an `OnceLock` so the Panel UI (Tauri) — or any other subscriber,
/// such as a webhook forwarder — can attach at runtime via [`subscribe`]
/// without a Tauri hard-dependency. The actual `tauri::Manager::emit_all` call
/// happens in the Panel UI crate, which subscribes here and bridges to the
/// frontend. Headless builds simply never subscribe and the bus stays idle.
static EVENT_BUS: OnceLock<broadcast::Sender<Notification>> = OnceLock::new();

/// Lazily initialize (or return the existing) notification event-bus sender.
///
/// Safe to call repeatedly; the first caller wins and subsequent calls return
/// the same sender. The bus is created with [`EVENT_BUS_CAPACITY`] slots.
pub fn event_bus() -> &'static broadcast::Sender<Notification> {
    EVENT_BUS.get_or_init(|| broadcast::channel::<Notification>(EVENT_BUS_CAPACITY).0)
}

/// Subscribe to the notification event bus.
///
/// Returns a fresh `broadcast::Receiver` each call, so multiple subscribers
/// (e.g. a Tauri emit-all bridge and a debug log tap) can coexist. Receivers
/// created after notifications are published will not see those past events —
/// subscribe early at startup.
pub fn subscribe() -> broadcast::Receiver<Notification> {
    event_bus().subscribe()
}

/// Publish a notification to the event bus.
///
/// Non-blocking and fallible: if there are no subscribers, or a receiver's
/// buffer is full, the notification is silently dropped (the legacy
/// channels — tracing/Discord/Telegram — are unaffected). Returns the number
/// of receivers that received the event, for telemetry.
pub fn publish(notif: &Notification) -> usize {
    event_bus().send(notif.clone()).unwrap_or(0)
}

/// Notification severity (maps to emoji + level).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NotificationLevel {
    Success,
    Info,
    Warning,
    Error,
    Critical,
}

/// A notification to be sent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    pub level: NotificationLevel,
    pub title: String,
    pub message: String,
    pub metadata: Option<serde_json::Value>,
}

impl Notification {
    /// Format as a Telegram-friendly message.
    pub fn to_telegram_text(&self) -> String {
        let emoji = match self.level {
            NotificationLevel::Success => "✅",
            NotificationLevel::Info => "ℹ️",
            NotificationLevel::Warning => "⚠️",
            NotificationLevel::Error => "❌",
            NotificationLevel::Critical => "🚨",
        };

        format!("{} *{}*\n\n{}", emoji, self.title, self.message)
    }

    /// Get Discord color for notification level
    pub fn discord_color(&self) -> u32 {
        match self.level {
            NotificationLevel::Success => 0x39ff14,  // Xavier Green
            NotificationLevel::Info => 0x3498db,     // Blue
            NotificationLevel::Warning => 0xf1c40f,  // Yellow
            NotificationLevel::Error => 0xe74c3c,    // Red
            NotificationLevel::Critical => 0x992d22, // Dark Red
        }
    }

    /// Format as a tracing log message.
    pub fn log(&self) {
        match self.level {
            NotificationLevel::Success | NotificationLevel::Info => {
                tracing::info!("{}: {}", self.title, self.message);
            }
            NotificationLevel::Warning => {
                tracing::warn!("{}: {}", self.title, self.message);
            }
            NotificationLevel::Error | NotificationLevel::Critical => {
                tracing::error!("{}: {}", self.title, self.message);
            }
        }
    }
}

/// The notifier — sends notifications via configured channels.
pub struct Notifier {
    discord: Option<DiscordClient>,
    #[cfg(feature = "telegram")]
    telegram_bot: Option<teloxide::prelude::Bot>,
    #[cfg(feature = "telegram")]
    notification_chat_id: Option<String>,
}

impl Notifier {
    /// Create a new notifier.
    pub fn new() -> Self {
        let settings = XavierSettings::current();
        let discord = if settings.discord.enabled {
            Some(DiscordClient::new(
                settings.discord.webhook_url.clone(),
                settings.discord.rate_limit_per_min,
            ))
        } else {
            None
        };

        #[cfg(feature = "telegram")]
        let (telegram_bot, notification_chat_id) =
            if settings.telegram.enabled && settings.telegram.bot_token.is_some() {
                (
                    Some(teloxide::prelude::Bot::new(
                        settings.telegram.bot_token.unwrap(),
                    )),
                    settings.telegram.notification_chat_id,
                )
            } else {
                (None, None)
            };

        Self {
            discord,
            #[cfg(feature = "telegram")]
            telegram_bot,
            #[cfg(feature = "telegram")]
            notification_chat_id,
        }
    }

    /// Notify about a detected error.
    pub fn notify_error(&self, diagnosis: &ErrorDiagnosis) -> Notification {
        let level = match diagnosis.urgency {
            Urgency::Critical => NotificationLevel::Critical,
            Urgency::High => NotificationLevel::Error,
            Urgency::Medium => NotificationLevel::Warning,
            Urgency::Low => NotificationLevel::Info,
        };

        let notif = Notification {
            level,
            title: format!("Error detected in {}", diagnosis.pattern.module),
            message: format!(
                "Module `{}`\nFrequency: {}x\n\n*Root cause:* {}\n\n*Suggested fix:* {}",
                diagnosis.pattern.module,
                diagnosis.pattern.frequency,
                diagnosis.root_cause,
                diagnosis.suggested_fix,
            ),
            metadata: Some(serde_json::json!({
                "module": diagnosis.pattern.module,
                "frequency": diagnosis.pattern.frequency,
                "urgency": diagnosis.urgency.to_string(),
                "confidence": diagnosis.confidence,
            })),
        };

        notif.log();
        self.send_background(notif.clone());
        self.emit_tauri(&notif);
        publish(&notif);
        notif
    }

    #[cfg(feature = "tauri")]
    fn emit_tauri(&self, notif: &Notification) {
        if let Some(handle) = get_tauri_app_handle() {
            let _ = handle.emit("notification", notif);
        }
    }

    #[cfg(not(feature = "tauri"))]
    fn emit_tauri(&self, _notif: &Notification) {
        // No-op when tauri feature is disabled
    }
    fn send_background(&self, notif: Notification) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            tracing::debug!("Skipping async notification delivery: no Tokio runtime is active");
            return;
        };

        let notif_clone = notif.clone();
        if let Some(client) = self.discord.clone() {
            handle.spawn(async move {
                let _ = client
                    .send_embed(
                        Some(notif_clone.title.clone()),
                        notif_clone.message.clone(),
                        Some(notif_clone.discord_color()),
                    )
                    .await;
            });
        }

        #[cfg(feature = "telegram")]
        {
            let notif_clone = notif.clone();
            let bot = self.telegram_bot.clone();
            let chat_id = self.notification_chat_id.clone();
            handle.spawn(async move {
                if let (Some(bot), Some(chat_id)) = (bot, chat_id) {
                    use teloxide::payloads::SendMessageSetters;
                    use teloxide::requests::Requester;
                    let text = notif_clone.to_telegram_text();
                    let chat_id = teloxide::types::ChatId(chat_id.parse().unwrap_or(0));
                    if chat_id.0 != 0 {
                        let _ = bot
                            .send_message(chat_id, text)
                            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                            .await;
                    }
                }
            });
        }
    }

    /// Notify about a fixer action result.
    pub fn notify_fixer_result(&self, result: &FixerResult) -> Notification {
        let (level, icon) = if result.success {
            (NotificationLevel::Success, "✅")
        } else {
            (NotificationLevel::Error, "❌")
        };

        let title = format!("{} Fixer: {}", icon, result.message);
        let message = if let Some(ref url) = result.url {
            format!("Issue created: {}", url)
        } else {
            result.message.clone()
        };

        let notif = Notification {
            level,
            title,
            message,
            metadata: Some(serde_json::json!({
                "success": result.success,
                "action": format!("{:?}", result.action),
                "url": result.url,
                "number": result.number,
            })),
        };

        notif.log();
        self.send_background(notif.clone());
        self.emit_tauri(&notif);
        publish(&notif);
        notif
    }

    /// Notify about system startup.
    pub fn notify_startup(&self) -> Notification {
        let notif = Notification {
            level: NotificationLevel::Info,
            title: "🚀 Xavier Observability Active".to_string(),
            message: format!(
                "File logging enabled\nError detection active\nAuto-fixer ready\nUptime monitor active\n\n_System started at {}_",
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
            ),
            metadata: None,
        };

        notif.log();
        self.send_background(notif.clone());
        self.emit_tauri(&notif);
        publish(&notif);
        notif
    }

    /// Notify about self-healing action.
    pub fn notify_self_heal(&self, description: &str, success: bool) -> Notification {
        let (level, icon) = if success {
            (NotificationLevel::Success, "🩹")
        } else {
            (NotificationLevel::Error, "💔")
        };

        let notif = Notification {
            level,
            title: format!(
                "{} Self-heal: {}",
                icon,
                if success { "Applied" } else { "Failed" }
            ),
            message: description.to_string(),
            metadata: None,
        };

        notif.log();
        self.send_background(notif.clone());
        self.emit_tauri(&notif);
        publish(&notif);
        notif
    }
}

impl Default for Notifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper: optionally emit a Telegram notification after a successful fixer
/// action.
///
/// Logs a trace-level message when called. Pushes a [`FixerAction::TelegramNotified`]
/// notification through the [`Notifier::send_background`] path when the `telegram`
/// feature is enabled.
///
/// Intended to be called at the fixer call site after `result.success` is true.
#[cfg(feature = "telegram")]
#[allow(dead_code)] // Reserved for Telegram fixer notification dispatch
pub fn maybe_notify_telegram_fix(action: &super::fixer::FixerAction) {
    tracing::trace!("maybe_notify_telegram_fix called with action: {action:?}");
    let result = super::fixer::FixerResult {
        action: super::fixer::FixerAction::TelegramNotified,
        url: None,
        number: None,
        success: true,
        message: format!("Telegram notification sent for fixer action: {:?}", action),
    };
    let notifier = Notifier::new();
    notifier.notify_fixer_result(&result);
}

/// Fallback: no-op when the telegram feature is disabled.
#[cfg(not(feature = "telegram"))]
#[allow(dead_code)] // Reserved for Telegram fixer notification dispatch
pub fn maybe_notify_telegram_fix(action: &super::fixer::FixerAction) {
    tracing::trace!("maybe_notify_telegram_fix (telegram disabled): {action:?}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observability::analyzer::{ErrorDiagnosis, Urgency};
    use crate::observability::fixer::{FixerAction, FixerResult};
    use crate::observability::service_log::{ErrorPattern, LogLevel};

    fn make_diagnosis(urgency: Urgency) -> ErrorDiagnosis {
        ErrorDiagnosis {
            pattern: ErrorPattern {
                module: "http::server".into(),
                level: LogLevel::Error,
                frequency: 5,
                sample_message: "connection timeout".into(),
                first_seen: "2025-01-01T00:00:00Z".into(),
                last_seen: "2025-01-01T01:00:00Z".into(),
            },
            analyzed_at: "2025-01-01T02:00:00.000Z".into(),
            root_cause: "Network timeout".into(),
            source_location: Some("src/http.rs:42".into()),
            suggested_fix: "Increase timeout".into(),
            confidence: 0.85,
            urgency,
        }
    }

    #[test]
    fn test_notification_level_emoji() {
        let n = Notification {
            level: NotificationLevel::Critical,
            title: "Test".into(),
            message: "Critical error".into(),
            metadata: None,
        };
        let text = n.to_telegram_text();
        assert!(text.starts_with("\u{1F6A8}"));
    }

    #[test]
    fn test_notification_level_success() {
        let n = Notification {
            level: NotificationLevel::Success,
            title: "Done".into(),
            message: "All good".into(),
            metadata: None,
        };
        assert!(n.to_telegram_text().starts_with("\u{2705}"));
    }

    #[test]
    fn test_notification_level_warning() {
        let n = Notification {
            level: NotificationLevel::Warning,
            title: "Warn".into(),
            message: "Disk full".into(),
            metadata: None,
        };
        assert!(n.to_telegram_text().starts_with("\u{26A0}\u{FE0F}"));
    }

    #[test]
    fn test_notification_level_info() {
        let n = Notification {
            level: NotificationLevel::Info,
            title: "Info".into(),
            message: "Started".into(),
            metadata: None,
        };
        assert!(n.to_telegram_text().starts_with("\u{2139}\u{FE0F}"));
    }

    #[test]
    fn test_notification_level_error() {
        let n = Notification {
            level: NotificationLevel::Error,
            title: "Error".into(),
            message: "Failed".into(),
            metadata: None,
        };
        assert!(n.to_telegram_text().starts_with("\u{274C}"));
    }

    #[test]
    fn test_notify_error_critical() {
        let notifier = Notifier::new();
        let diagnosis = make_diagnosis(Urgency::Critical);
        let notif = notifier.notify_error(&diagnosis);
        assert_eq!(notif.level, NotificationLevel::Critical);
        assert!(notif.title.contains("http::server"));
    }

    #[test]
    fn test_notify_error_high() {
        let notifier = Notifier::new();
        let diagnosis = make_diagnosis(Urgency::High);
        let notif = notifier.notify_error(&diagnosis);
        assert_eq!(notif.level, NotificationLevel::Error);
    }

    #[test]
    fn test_notify_error_medium() {
        let notifier = Notifier::new();
        let diagnosis = make_diagnosis(Urgency::Medium);
        let notif = notifier.notify_error(&diagnosis);
        assert_eq!(notif.level, NotificationLevel::Warning);
    }

    #[test]
    fn test_notify_error_low() {
        let notifier = Notifier::new();
        let diagnosis = make_diagnosis(Urgency::Low);
        let notif = notifier.notify_error(&diagnosis);
        assert_eq!(notif.level, NotificationLevel::Info);
    }

    #[test]
    fn test_notify_fixer_result_success() {
        let notifier = Notifier::new();
        let result = FixerResult {
            action: FixerAction::IssueCreated,
            url: Some("https://github.com/iberi22/xavier/issues/1".into()),
            number: Some(1),
            success: true,
            message: "Issue created successfully".into(),
        };
        let notif = notifier.notify_fixer_result(&result);
        assert_eq!(notif.level, NotificationLevel::Success);
    }

    #[test]
    fn test_notify_fixer_result_failure() {
        let notifier = Notifier::new();
        let result = FixerResult {
            action: FixerAction::IssueCreated,
            url: None,
            number: None,
            success: false,
            message: "gh CLI not available".into(),
        };
        let notif = notifier.notify_fixer_result(&result);
        assert_eq!(notif.level, NotificationLevel::Error);
    }

    #[test]
    fn test_notify_startup() {
        let notifier = Notifier::new();
        let notif = notifier.notify_startup();
        assert_eq!(notif.level, NotificationLevel::Info);
        assert!(notif.title.contains("Observability"));
    }

    #[test]
    fn test_notify_self_heal_success() {
        let notifier = Notifier::new();
        let notif = notifier.notify_self_heal("Restarted service", true);
        assert_eq!(notif.level, NotificationLevel::Success);
    }

    #[test]
    fn test_notify_self_heal_failure() {
        let notifier = Notifier::new();
        let notif = notifier.notify_self_heal("Failed to restart", false);
        assert_eq!(notif.level, NotificationLevel::Error);
    }

    #[test]
    fn test_notification_with_metadata() {
        let n = Notification {
            level: NotificationLevel::Error,
            title: "Test".into(),
            message: "With metadata".into(),
            metadata: Some(serde_json::json!({"key": "value"})),
        };
        assert!(n.metadata.is_some());
        assert_eq!(n.metadata.as_ref().unwrap()["key"], "value");
    }

    // --- Event bus (broadcast channel) tests ---

    #[test]
    fn test_event_bus_singleton_is_stable() {
        // event_bus() is a process-wide singleton; two lookups return the same
        // sender reference.
        let a = event_bus();
        let b = event_bus();
        assert!(std::ptr::eq(a, b));
    }

    #[test]
    fn test_publish_delivers_to_subscriber() {
        // Subscribe before publishing so the receiver captures the event.
        let mut rx = subscribe();
        let n = Notification {
            level: NotificationLevel::Info,
            title: "bus-delivery".into(),
            message: "hello bus".into(),
            metadata: None,
        };
        let delivered = publish(&n);
        assert!(delivered >= 1, "at least one subscriber should receive it");

        // Drain: the most recent message should match what we published.
        let received = rx.try_recv().expect("receiver should have a message");
        assert_eq!(received.title, "bus-delivery");
        assert_eq!(received.level, NotificationLevel::Info);
    }

    #[test]
    fn test_publish_with_no_subscribers_is_safe() {
        // Publishing with no new subscriber must not panic; returns 0.
        // (We can't guarantee zero total subscribers across the whole test
        //  binary, but publish is always fallible, so just assert it returns.)
        let n = Notification {
            level: NotificationLevel::Warning,
            title: "no-sub".into(),
            message: "dropped".into(),
            metadata: None,
        };
        let _ = publish(&n); // must not panic
    }

    #[test]
    fn test_notify_startup_publishes_to_bus() {
        // The high-level integration path: a notify_* method should fan out to
        // the event bus in addition to its legacy channels.
        let mut rx = subscribe();
        let notifier = Notifier::new();
        let notif = notifier.notify_startup();
        // The published payload should match the returned notification.
        // Drain any lagged/older messages until we see ours (or hit Empty).
        let mut saw = false;
        while let Ok(received) = rx.try_recv() {
            if received.title == notif.title {
                saw = true;
                break;
            }
        }
        assert!(
            saw,
            "notify_startup should publish its notification to the bus"
        );
    }

    #[test]
    fn test_maybe_notify_telegram_fix_helper_exists_and_callable() {
        let mut rx = subscribe();
        maybe_notify_telegram_fix(&FixerAction::IssueCreated);

        let mut saw_telegram_notified = false;
        while let Ok(received) = rx.try_recv() {
            if let Some(metadata) = &received.metadata {
                if metadata.get("action").and_then(|a| a.as_str()) == Some("\"TelegramNotified\"")
                    || metadata.get("action").and_then(|a| a.as_str()) == Some("TelegramNotified")
                {
                    saw_telegram_notified = true;
                    break;
                }
            }
        }

        #[cfg(feature = "telegram")]
        assert!(
            saw_telegram_notified,
            "maybe_notify_telegram_fix should publish a notification with FixerAction::TelegramNotified"
        );
        #[cfg(not(feature = "telegram"))]
        let _ = saw_telegram_notified;
    }
}
