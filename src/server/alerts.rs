use serde::{Deserialize, Serialize};
use std::sync::RwLock;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemAlert {
    pub id: String,
    pub level: String,
    pub message: String,
    pub component: String,
    pub created_at: DateTime<Utc>,
}

pub struct SystemAlertStore {
    alerts: RwLock<Vec<SystemAlert>>,
}

impl SystemAlertStore {
    pub fn new() -> Self {
        Self {
            alerts: RwLock::new(Vec::new()),
        }
    }

    pub fn push_alert(&self, level: &str, message: &str, component: &str) {
        let alert = SystemAlert {
            id: uuid::Uuid::new_v4().to_string(),
            level: level.to_string(),
            message: message.to_string(),
            component: component.to_string(),
            created_at: Utc::now(),
        };

        if let Ok(mut alerts) = self.alerts.write() {
            alerts.push(alert);
            // Keep only the most recent 100 alerts
            if alerts.len() > 100 {
                alerts.remove(0);
            }
        }
    }

    pub fn get_alerts(&self) -> Vec<SystemAlert> {
        if let Ok(alerts) = self.alerts.read() {
            alerts.clone()
        } else {
            Vec::new()
        }
    }

    pub fn clear(&self) {
        if let Ok(mut alerts) = self.alerts.write() {
            alerts.clear();
        }
    }
}

// Global instance
pub static SYSTEM_ALERTS: std::sync::LazyLock<SystemAlertStore> =
    std::sync::LazyLock::new(|| SystemAlertStore::new());
