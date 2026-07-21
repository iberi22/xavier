use crate::codebase::connection_manager::ConnectionManager;
use crate::memory::sqlite_store::TABLE_NOTIFICATIONS;
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IslandId {
    System,
    Memory,
    Agents,
    Errors,
}

impl IslandId {
    /// As str.
    pub fn as_str(&self) -> &'static str {
        match self {
            IslandId::System => "system",
            IslandId::Memory => "memory",
            IslandId::Agents => "agents",
            IslandId::Errors => "errors",
        }
    }

    #[allow(clippy::should_implement_trait)]
    /// From str.
    pub fn from_str(s: &str) -> Self {
        match s {
            "memory" => IslandId::Memory,
            "agents" => IslandId::Agents,
            "errors" => IslandId::Errors,
            _ => IslandId::System,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: String,
    pub island_id: IslandId,
    pub title: String,
    pub body: String,
    pub timestamp: DateTime<Utc>,
    pub read: bool,
    pub severity: String, // info, warning, error, success
}

#[async_trait::async_trait]
pub trait NotificationProvider: Send + Sync {
    async fn send(&self, notification: &Notification) -> Result<()>;
    fn id(&self) -> &'static str;
    fn is_enabled(&self) -> bool;
}

pub struct InAppProvider;

#[async_trait::async_trait]
impl NotificationProvider for InAppProvider {
    async fn send(&self, notification: &Notification) -> Result<()> {
        let island_id = notification.island_id.as_str().to_string();
        let id = notification.id.clone();
        let title = notification.title.clone();
        let body = notification.body.clone();
        let timestamp = notification.timestamp.to_rfc3339();
        let read = if notification.read { 1 } else { 0 };
        let severity = notification.severity.clone();

        ConnectionManager::global().with_conn("memory", move |conn| {
            conn.execute(
                &format!("INSERT INTO {} (id, island_id, title, body, timestamp, read, severity) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", TABLE_NOTIFICATIONS),
                params![id, island_id, title, body, timestamp, read, severity],
            )?;
            Ok(())
        }).await
    }

    fn id(&self) -> &'static str {
        "in_app"
    }

    fn is_enabled(&self) -> bool {
        #[cfg(feature = "notification-in-app")]
        {
            true
        }
        #[cfg(not(feature = "notification-in-app"))]
        {
            false
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookSubscription {
    pub id: String,
    pub url: String,
    pub event_types: Vec<String>,
    pub active: bool,
    pub created_at: DateTime<Utc>,
}

pub struct WebhookProvider;

#[async_trait::async_trait]
impl NotificationProvider for WebhookProvider {
    async fn send(&self, notification: &Notification) -> Result<()> {
        let subs = NOTIFICATIONS.list_subscriptions().await?;
        let client = reqwest::Client::new();
        let island_str = notification.island_id.as_str();

        for sub in subs {
            if !sub.active {
                continue;
            }
            let matches = sub.event_types.iter().any(|t| t == "*" || t == island_str);
            if !matches {
                continue;
            }

            let url = sub.url.clone();
            let notification_clone = notification.clone();
            let client_clone = client.clone();

            tokio::spawn(async move {
                match client_clone.post(&url).json(&notification_clone).send().await {
                    Ok(resp) => {
                        if !resp.status().is_success() {
                            tracing::error!("Webhook to {} returned status {}", url, resp.status());
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to send webhook to {}: {}", url, e);
                    }
                }
            });
        }
        Ok(())
    }

    fn id(&self) -> &'static str {
        "webhook"
    }

    fn is_enabled(&self) -> bool {
        #[cfg(feature = "notification-webhook")]
        {
            true
        }
        #[cfg(not(feature = "notification-webhook"))]
        {
            false
        }
    }
}

pub static SENT_EMAILS: std::sync::LazyLock<Arc<tokio::sync::Mutex<Vec<Notification>>>> =
    std::sync::LazyLock::new(|| Arc::new(tokio::sync::Mutex::new(Vec::new())));

pub struct EmailProvider;

#[async_trait::async_trait]
impl NotificationProvider for EmailProvider {
    async fn send(&self, notification: &Notification) -> Result<()> {
        tracing::info!("Sending email notification to configured address: {}", notification.title);
        let mut emails = SENT_EMAILS.lock().await;
        emails.push(notification.clone());
        Ok(())
    }

    fn id(&self) -> &'static str {
        "email"
    }

    fn is_enabled(&self) -> bool {
        #[cfg(feature = "notification-email")]
        {
            true
        }
        #[cfg(not(feature = "notification-email"))]
        {
            false
        }
    }
}

pub struct NotificationManager {
    event_tx: broadcast::Sender<Notification>,
    pub providers: Vec<Arc<dyn NotificationProvider>>,
}

impl NotificationManager {
    /// New.
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(100);
        let mut providers: Vec<Arc<dyn NotificationProvider>> = Vec::new();

        providers.push(Arc::new(InAppProvider));
        providers.push(Arc::new(WebhookProvider));
        providers.push(Arc::new(EmailProvider));

        Self {
            event_tx: tx,
            providers,
        }
    }

    /// Subscribe.
    pub fn subscribe(&self) -> broadcast::Receiver<Notification> {
        self.event_tx.subscribe()
    }

    /// Forward notifications to the Tauri webview.
    ///
    /// Safe to call from the Tauri setup hook (which is **not** on a Tokio
    /// runtime). Uses a dedicated background thread + Tokio runtime instead of
    /// bare `tokio::spawn`, which panics with "no reactor running".
    #[cfg(feature = "tauri")]
    pub fn spawn_tauri_forwarder(&self) {
        use tauri::Emitter;
        let mut rx = self.subscribe();
        std::thread::Builder::new()
            .name("xavier-tauri-notify".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        eprintln!("Failed to create notification runtime: {e}");
                        return;
                    }
                };
                rt.block_on(async move {
                    while let Ok(notification) = rx.recv().await {
                        if let Some(handle) = crate::utils::tauri_utils::get_tauri_app_handle() {
                            let _ = handle.emit("new-notification", notification);
                        }
                    }
                });
            })
            .ok();
    }

    /// Notify.
    pub async fn notify(
        &self,
        island_id: IslandId,
        title: &str,
        body: &str,
        severity: &str,
    ) -> Result<Notification> {
        let notification = Notification {
            id: uuid::Uuid::new_v4().to_string(),
            island_id,
            title: title.to_string(),
            body: body.to_string(),
            timestamp: Utc::now(),
            read: false,
            severity: severity.to_string(),
        };

        // Deliver to each enabled provider
        for provider in &self.providers {
            if provider.is_enabled() {
                if let Err(e) = provider.send(&notification).await {
                    tracing::error!("Failed to send notification via provider '{}': {}", provider.id(), e);
                }
            }
        }

        let _ = self.event_tx.send(notification.clone());
        Ok(notification)
    }

    /// Ensure webhook table.
    pub async fn ensure_webhook_table(&self) -> Result<()> {
        ConnectionManager::global()
            .with_conn("memory", move |conn| {
                conn.execute(
                    "CREATE TABLE IF NOT EXISTS webhook_subscriptions (
                        id TEXT PRIMARY KEY,
                        url TEXT NOT NULL,
                        event_types TEXT NOT NULL,
                        active INTEGER NOT NULL DEFAULT 1,
                        created_at TEXT NOT NULL
                    )",
                    [],
                )?;
                Ok(())
            })
            .await
    }

    /// Add subscription.
    pub async fn add_subscription(&self, url: &str, event_types: Vec<String>) -> Result<WebhookSubscription> {
        self.ensure_webhook_table().await?;
        let sub = WebhookSubscription {
            id: uuid::Uuid::new_v4().to_string(),
            url: url.to_string(),
            event_types,
            active: true,
            created_at: Utc::now(),
        };
        let id = sub.id.clone();
        let url = sub.url.clone();
        let event_types_str = sub.event_types.join(",");
        let active = if sub.active { 1 } else { 0 };
        let created_at = sub.created_at.to_rfc3339();

        ConnectionManager::global().with_conn("memory", move |conn| {
            conn.execute(
                "INSERT INTO webhook_subscriptions (id, url, event_types, active, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, url, event_types_str, active, created_at],
            )?;
            Ok(())
        }).await?;
        Ok(sub)
    }

    /// List subscriptions.
    pub async fn list_subscriptions(&self) -> Result<Vec<WebhookSubscription>> {
        self.ensure_webhook_table().await?;
        ConnectionManager::global().with_conn("memory", move |conn| {
            let mut stmt = conn.prepare("SELECT id, url, event_types, active, created_at FROM webhook_subscriptions")?;
            let mut rows = stmt.query([])?;
            let mut result = Vec::new();
            while let Some(row) = rows.next()? {
                let event_types_str: String = row.get(2)?;
                let event_types = event_types_str.split(',').map(|s| s.to_string()).filter(|s| !s.is_empty()).collect();
                let created_at_str: String = row.get(4)?;
                result.push(WebhookSubscription {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    event_types,
                    active: row.get::<_, i32>(3)? != 0,
                    created_at: created_at_str.parse().unwrap_or_else(|_| Utc::now()),
                });
            }
            Ok(result)
        }).await
    }

    /// Remove subscription.
    pub async fn remove_subscription(&self, id: &str) -> Result<()> {
        self.ensure_webhook_table().await?;
        let id = id.to_string();
        ConnectionManager::global().with_conn("memory", move |conn| {
            conn.execute("DELETE FROM webhook_subscriptions WHERE id = ?", params![id])?;
            Ok(())
        }).await
    }

    /// List notifications.
    pub async fn list_notifications(&self) -> Result<Vec<Notification>> {
        ConnectionManager::global().with_conn("memory", move |conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT id, island_id, title, body, timestamp, read, severity FROM {} ORDER BY timestamp DESC LIMIT 100",
                TABLE_NOTIFICATIONS
            ))?;
            let mut rows = stmt.query([])?;
            let mut result = Vec::new();
            while let Some(row) = rows.next()? {
                let island_str: String = row.get(1)?;
                let ts_str: String = row.get(4)?;
                result.push(Notification {
                    id: row.get(0)?,
                    island_id: IslandId::from_str(&island_str),
                    title: row.get(2)?,
                    body: row.get(3)?,
                    timestamp: ts_str.parse().unwrap_or_else(|_| Utc::now()),
                    read: row.get::<_, i32>(5)? != 0,
                    severity: row.get(6)?,
                });
            }
            Ok(result)
        }).await
    }

    /// Mark as read.
    pub async fn mark_as_read(&self, id: &str) -> Result<()> {
        let id = id.to_string();
        ConnectionManager::global()
            .with_conn("memory", move |conn| {
                conn.execute(
                    &format!("UPDATE {} SET read = 1 WHERE id = ?", TABLE_NOTIFICATIONS),
                    params![id],
                )?;
                Ok(())
            })
            .await
    }

    /// Mark all as read.
    pub async fn mark_all_as_read(&self) -> Result<()> {
        ConnectionManager::global()
            .with_conn("memory", move |conn| {
                conn.execute(&format!("UPDATE {} SET read = 1", TABLE_NOTIFICATIONS), [])?;
                Ok(())
            })
            .await
    }

    /// Delete all.
    pub async fn delete_all(&self) -> Result<()> {
        ConnectionManager::global()
            .with_conn("memory", move |conn| {
                conn.execute(&format!("DELETE FROM {}", TABLE_NOTIFICATIONS), [])?;
                Ok(())
            })
            .await
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self::new()
    }
}

pub static NOTIFICATIONS: std::sync::LazyLock<NotificationManager> =
    std::sync::LazyLock::new(NotificationManager::new);

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_notification_broadcast() {
        let manager = NotificationManager::new();
        let mut rx = manager.subscribe();

        let notification = Notification {
            id: "test-id".to_string(),
            island_id: IslandId::System,
            title: "Test Title".to_string(),
            body: "Test Body".to_string(),
            timestamp: Utc::now(),
            read: false,
            severity: "info".to_string(),
        };

        let _ = manager.event_tx.send(notification);

        let received = rx.recv().await.unwrap();
        assert_eq!(received.title, "Test Title");
        assert_eq!(received.body, "Test Body");
        assert_eq!(received.severity, "info");
    }
}