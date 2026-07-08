// src/ui_logger.rs
// Sistema de logging robusto para la UI de Tauri que registra errores en memoria de Xavier

use crate::notifications::{IslandId, NOTIFICATIONS};
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UILogLevel {
    Debug,
    Info,
    Warning,
    Error,
    Critical,
}

impl UILogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            UILogLevel::Debug => "debug",
            UILogLevel::Info => "info",
            UILogLevel::Warning => "warning",
            UILogLevel::Error => "error",
            UILogLevel::Critical => "critical",
        }
    }

    pub fn to_notification_severity(&self) -> &'static str {
        match self {
            UILogLevel::Debug => "info",
            UILogLevel::Info => "info",
            UILogLevel::Warning => "warning",
            UILogLevel::Error => "error",
            UILogLevel::Critical => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UILogEntry {
    pub timestamp: String,
    pub level: UILogLevel,
    pub component: String,
    pub message: String,
    pub context: Option<serde_json::Value>,
    pub stack_trace: Option<String>,
}

pub struct UILogger {
    log_file: PathBuf,
}

impl UILogger {
    pub fn new(workspace_id: &str) -> Self {
        let log_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Xavier")
            .join("logs");

        std::fs::create_dir_all(&log_dir).ok();

        let log_file = log_dir.join(format!("ui-{}.log", workspace_id));

        Self { log_file }
    }

    /// Log a UI event
    pub async fn log(
        &self,
        level: UILogLevel,
        component: &str,
        message: &str,
        context: Option<serde_json::Value>,
        stack_trace: Option<String>,
    ) -> Result<()> {
        let entry = UILogEntry {
            timestamp: Utc::now().to_rfc3339(),
            level: level.clone(),
            component: component.to_string(),
            message: message.to_string(),
            context,
            stack_trace,
        };

        // 1. Write to file
        self.write_to_file(&entry).await?;

        // 2. Send notification for errors/critical
        if matches!(entry.level, UILogLevel::Error | UILogLevel::Critical) {
            self.send_notification(&entry).await?;
        }

        // 3. Also log to console/tracing
        self.log_to_tracing(&entry);

        Ok(())
    }

    async fn write_to_file(&self, entry: &UILogEntry) -> Result<()> {
        let mut log_line = format!(
            "[{}] [{}] [{}] {}\n",
            entry.timestamp,
            entry.level.as_str().to_uppercase(),
            entry.component,
            entry.message
        );

        if let Some(ctx) = &entry.context {
            log_line.push_str(&format!("  Context: {}\n", serde_json::to_string(ctx)?));
        }

        if let Some(stack) = &entry.stack_trace {
            log_line.push_str(&format!("  Stack: {}\n", stack));
        }

        // Append to log file
        if let Ok(existing) = fs::read_to_string(&self.log_file).await {
            fs::write(&self.log_file, format!("{}{}", existing, log_line)).await?;
        } else {
            fs::write(&self.log_file, log_line).await?;
        }

        Ok(())
    }

    async fn send_notification(&self, entry: &UILogEntry) -> Result<()> {
        let title = format!("UI {}: {}", entry.level.as_str().to_uppercase(), entry.component);
        
        let body = if let Some(stack) = &entry.stack_trace {
            format!("{}\n\nStack:\n{}", entry.message, stack)
        } else {
            entry.message.clone()
        };

        NOTIFICATIONS
            .notify(
                IslandId::Errors,
                &title,
                &body,
                entry.level.to_notification_severity(),
            )
            .await?;

        Ok(())
    }

    fn log_to_tracing(&self, entry: &UILogEntry) {
        let msg = format!("[UI:{}] {}", entry.component, entry.message);
        
        match entry.level {
            UILogLevel::Debug => tracing::debug!("{}", msg),
            UILogLevel::Info => tracing::info!("{}", msg),
            UILogLevel::Warning => tracing::warn!("{}", msg),
            UILogLevel::Error => tracing::error!("{}", msg),
            UILogLevel::Critical => tracing::error!("[CRITICAL] {}", msg),
        }
    }
}

// Global logger instance
static UI_LOGGER: std::sync::OnceLock<Arc<tokio::sync::Mutex<UILogger>>> =
    std::sync::OnceLock::new();

pub fn init_ui_logger(workspace_id: &str) {
    let logger = UILogger::new(workspace_id);
    UI_LOGGER.get_or_init(|| Arc::new(tokio::sync::Mutex::new(logger)));
}

pub async fn log_ui_event(
    level: UILogLevel,
    component: &str,
    message: &str,
    context: Option<serde_json::Value>,
    stack_trace: Option<String>,
) -> Result<()> {
    if let Some(logger) = UI_LOGGER.get() {
        logger
            .lock()
            .await
            .log(level, component, message, context, stack_trace)
            .await?;
    } else {
        tracing::warn!("UI logger not initialized, falling back to tracing");
        tracing::error!("[UI:{}] {}", component, message);
    }
    Ok(())
}

// Convenience functions
pub async fn log_ui_error(component: &str, message: &str, stack: Option<String>) {
    let _ = log_ui_event(UILogLevel::Error, component, message, None, stack).await;
}

pub async fn log_ui_info(component: &str, message: &str) {
    let _ = log_ui_event(UILogLevel::Info, component, message, None, None).await;
}

pub async fn log_ui_warning(component: &str, message: &str) {
    let _ = log_ui_event(UILogLevel::Warning, component, message, None, None).await;
}

pub async fn log_ui_critical(component: &str, message: &str, context: serde_json::Value) {
    let _ = log_ui_event(UILogLevel::Critical, component, message, Some(context), None).await;
}
