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

/// Notification severity (maps to emoji + level).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationLevel {
    Success,
    Info,
    Warning,
    Error,
    Critical,
}

/// A notification to be sent.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct Notifier;

impl Notifier {
    /// Create a new notifier.
    pub fn new() -> Self {
        Self
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
        notif
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
        notif
    }
}

impl Default for Notifier {
    fn default() -> Self {
        Self::new()
    }
}
