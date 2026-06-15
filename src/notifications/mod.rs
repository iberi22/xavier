use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use tokio::sync::broadcast;
use crate::codebase::connection_manager::ConnectionManager;
use crate::memory::sqlite_store::TABLE_NOTIFICATIONS;
use rusqlite::params;
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IslandId {
    System,
    Memory,
    Agents,
    Errors,
}

impl IslandId {
    pub fn as_str(&self) -> &'static str {
        match self {
            IslandId::System => "system",
            IslandId::Memory => "memory",
            IslandId::Agents => "agents",
            IslandId::Errors => "errors",
        }
    }

    #[allow(clippy::should_implement_trait)]
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

pub struct NotificationManager {
    event_tx: broadcast::Sender<Notification>,
}

impl NotificationManager {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(100);
        Self { event_tx: tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Notification> {
        self.event_tx.subscribe()
    }

    #[cfg(feature = "tauri")]
    pub fn spawn_tauri_forwarder(&self) {
        use tauri::Emitter;
        let mut rx = self.subscribe();
        tokio::spawn(async move {
            while let Ok(notification) = rx.recv().await {
                if let Some(handle) = crate::utils::tauri_utils::get_tauri_app_handle() {
                    let _ = handle.emit("new-notification", notification);
                }
            }
        });
    }

    pub async fn notify(&self, island_id: IslandId, title: &str, body: &str, severity: &str) -> Result<Notification> {
        let notification = Notification {
            id: uuid::Uuid::new_v4().to_string(),
            island_id,
            title: title.to_string(),
            body: body.to_string(),
            timestamp: Utc::now(),
            read: false,
            severity: severity.to_string(),
        };

        self.persist_notification(&notification).await?;
        let _ = self.event_tx.send(notification.clone());
        Ok(notification)
    }

    async fn persist_notification(&self, n: &Notification) -> Result<()> {
        let island_id = n.island_id.as_str().to_string();
        let id = n.id.clone();
        let title = n.title.clone();
        let body = n.body.clone();
        let timestamp = n.timestamp.to_rfc3339();
        let read = if n.read { 1 } else { 0 };
        let severity = n.severity.clone();

        ConnectionManager::global().with_conn("memory", move |conn| {
            conn.execute(
                &format!("INSERT INTO {} (id, island_id, title, body, timestamp, read, severity) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", TABLE_NOTIFICATIONS),
                params![id, island_id, title, body, timestamp, read, severity],
            )?;
            Ok(())
        }).await
    }

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

    pub async fn mark_as_read(&self, id: &str) -> Result<()> {
        let id = id.to_string();
        ConnectionManager::global().with_conn("memory", move |conn| {
            conn.execute(
                &format!("UPDATE {} SET read = 1 WHERE id = ?", TABLE_NOTIFICATIONS),
                params![id],
            )?;
            Ok(())
        }).await
    }

    pub async fn mark_all_as_read(&self) -> Result<()> {
        ConnectionManager::global().with_conn("memory", move |conn| {
            conn.execute(
                &format!("UPDATE {} SET read = 1", TABLE_NOTIFICATIONS),
                [],
            )?;
            Ok(())
        }).await
    }

    pub async fn delete_all(&self) -> Result<()> {
        ConnectionManager::global().with_conn("memory", move |conn| {
            conn.execute(
                &format!("DELETE FROM {}", TABLE_NOTIFICATIONS),
                [],
            )?;
            Ok(())
        }).await
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

        // Use a mock notification that doesn't trigger persistence for this test
        // Or handle the fact that notify() calls persist_notification()
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
