//! # Notifier
//!
//! Sends notifications about errors, fixes, and system events
//! via Telegram (already configured) and other channels.
//!
//! ## Channels
//!
//! - **Telegram** → immediate notifications to the configured bot
//! - **tracing!** → logs to stdout + file (always on)
//! - **(Future)** Webhook, email, Discord

use serde::{Deserialize, Serialize};

use super::analyzer::{ErrorDiagnosis, Urgency};
use super::fixer::FixerResult;
use crate::messaging::DiscordClient;
use crate::settings::XavierSettings;
#[cfg(feature = "tauri")]
use crate::utils::tauri_utils::get_tauri_app_handle;
#[cfg(feature = "tauri")]
use tauri::Emitter;

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
        self.send_background(notif.clone(), Some(level.clone()));
        notif
    }

    fn send_background(&self, notif: Notification, level: Option<NotificationLevel>) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            tracing::debug!("Skipping async notification delivery: no Tokio runtime is active");
            return;
        };

        // Forward to the unified notification system
        let notif_for_unified = notif.clone();
        let level_for_unified = level.clone();
        handle.spawn(async move {
            use crate::notifications::{NOTIFICATIONS, IslandId};
            let (island, severity) = match level_for_unified {
                Some(NotificationLevel::Critical) => (IslandId::Errors, "error"),
                Some(NotificationLevel::Error) => (IslandId::Errors, "error"),
                Some(NotificationLevel::Warning) => (IslandId::Errors, "warning"),
                Some(NotificationLevel::Success) => (IslandId::System, "success"),
                _ => (IslandId::System, "info"),
            };

            let _ = NOTIFICATIONS.notify(
                island,
                &notif_for_unified.title,
                &notif_for_unified.message,
                severity
            ).await;
        });

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
        self.send_background(notif.clone(), Some(level.clone()));
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
        self.send_background(notif.clone(), Some(NotificationLevel::Info));
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
        self.send_background(notif.clone(), Some(level.clone()));
        notif
    }
}

impl Default for Notifier {
    fn default() -> Self {
        Self::new()
    }
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
}
