use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::Instant;

#[derive(Debug, Clone)]
struct SuppressionRecord {
    last_seen: Instant,
    count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum OperationalMode {
    LocalHealthy,
    LocalDegraded,
    CloudFallback,
    Disabled,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SystemAlert {
    pub id: String,
    pub level: String,
    pub message: String,
    pub component: String,
    pub created_at: DateTime<Utc>,
}

pub struct SystemAlertStore {
    alerts: RwLock<Vec<SystemAlert>>,
    last_email_sent: RwLock<std::collections::HashMap<String, DateTime<Utc>>>,
    suppression_map: Mutex<HashMap<(String, String), SuppressionRecord>>,
    total_suppressed_count: AtomicU64,
}

impl SystemAlertStore {
    /// New.
    pub fn new() -> Self {
        Self {
            alerts: RwLock::new(Vec::new()),
            last_email_sent: RwLock::new(std::collections::HashMap::new()),
            suppression_map: Mutex::new(HashMap::new()),
            total_suppressed_count: AtomicU64::new(0),
        }
    }

    /// Push alert.
    pub fn push_alert(&self, level: &str, message: &str, component: &str) {
        let key = (component.to_string(), message.to_string());
        let now = Instant::now();
        let mut formatted_message = message.to_string();

        if let Ok(mut map) = self.suppression_map.lock() {
            if let Some(record) = map.get_mut(&key) {
                if now.duration_since(record.last_seen) < std::time::Duration::from_secs(60) {
                    record.count += 1;
                    self.total_suppressed_count.fetch_add(1, Ordering::Relaxed);
                    return;
                } else {
                    if record.count > 0 {
                        formatted_message = format!("[suppressed {}x] {}", record.count, message);
                    }
                    record.last_seen = now;
                    record.count = 0;
                }
            } else {
                map.insert(
                    key,
                    SuppressionRecord {
                        last_seen: now,
                        count: 0,
                    },
                );
            }
        }

        let alert = SystemAlert {
            id: uuid::Uuid::new_v4().to_string(),
            level: level.to_string(),
            message: formatted_message,
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

    /// Get alerts.
    pub fn get_alerts(&self) -> Vec<SystemAlert> {
        if let Ok(alerts) = self.alerts.read() {
            alerts.clone()
        } else {
            Vec::new()
        }
    }

    /// Clear.
    pub fn clear(&self) {
        if let Ok(mut alerts) = self.alerts.write() {
            alerts.clear();
        }
        if let Ok(mut emails) = self.last_email_sent.write() {
            emails.clear();
        }
        if let Ok(mut map) = self.suppression_map.lock() {
            map.clear();
        }
        self.total_suppressed_count.store(0, Ordering::Relaxed);
    }

    /// Check if an email notification for the given key should be sent based on deduplication window.
    pub async fn should_notify_email_async(&self, alert_key: &str, window_secs: u64) -> bool {
        let now = Utc::now();
        let window_duration = chrono::Duration::seconds(window_secs as i64);

        if let Ok(guard) = self.last_email_sent.read() {
            if let Some(last_time) = guard.get(alert_key) {
                if now < *last_time + window_duration {
                    return false;
                }
            }
        }

        // Check SQLite notifications table if memory pool is available to persist window check across restarts
        let key_owned = alert_key.to_string();
        let db_last_ts: Option<DateTime<Utc>> = crate::codebase::connection_manager::ConnectionManager::global()
            .with_conn("memory", move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT timestamp FROM notifications WHERE title = ? ORDER BY timestamp DESC LIMIT 1",
                )?;
                let mut rows = stmt.query([key_owned])?;
                if let Some(row) = rows.next()? {
                    let ts_str: String = row.get(0)?;
                    Ok(ts_str.parse::<DateTime<Utc>>().ok())
                } else {
                    Ok(None)
                }
            })
            .await
            .unwrap_or(None);

        if let Some(last_ts) = db_last_ts {
            if now < last_ts + window_duration {
                return false;
            }
        }

        true
    }

    /// Record that an email notification was sent for the given key.
    pub fn record_email_sent(&self, alert_key: &str) {
        if let Ok(mut guard) = self.last_email_sent.write() {
            guard.insert(alert_key.to_string(), Utc::now());
        }
    }

    /// Derive operational mode.
    pub fn derive_operational_mode(
        llm_reachable: bool,
        embedding_reachable: bool,
        provider_setting: &str,
    ) -> OperationalMode {
        let provider = provider_setting.trim().to_ascii_lowercase();
        if provider == "disabled" {
            OperationalMode::Disabled
        } else if provider == "local" || provider == "ollama" {
            if !llm_reachable || !embedding_reachable {
                OperationalMode::LocalDegraded
            } else {
                OperationalMode::LocalHealthy
            }
        } else {
            OperationalMode::CloudFallback
        }
    }

    /// Get mode.
    pub fn get_mode(&self) -> OperationalMode {
        let alerts = self.get_alerts();
        let provider = std::env::var("XAVIER_PROVIDER").unwrap_or_else(|_| "local".to_string());

        let has_llm_error = alerts
            .iter()
            .any(|a| a.component == "llm" && a.level == "ERROR");
        let has_embedding_error = alerts
            .iter()
            .any(|a| a.component == "embedding" && a.level == "ERROR");

        Self::derive_operational_mode(!has_llm_error, !has_embedding_error, &provider)
    }

    /// Returns the total cumulative count of suppressed alerts.
    pub fn suppressed_count(&self) -> u64 {
        self.total_suppressed_count.load(Ordering::Relaxed)
    }
}

/// Returns the total cumulative count of suppressed alerts from the global store.
pub fn suppressed_count() -> u64 {
    SYSTEM_ALERTS.suppressed_count()
}

impl Default for SystemAlertStore {
    fn default() -> Self {
        Self::new()
    }
}

// Global instance
pub static SYSTEM_ALERTS: std::sync::LazyLock<SystemAlertStore> =
    std::sync::LazyLock::new(SystemAlertStore::new);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alert_suppression_window() {
        let store = SystemAlertStore::new();

        // 1st alert: recorded immediately
        store.push_alert("ERROR", "Connection failed", "embedding");
        let alerts = store.get_alerts();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].message, "Connection failed");
        assert_eq!(store.suppressed_count(), 0);

        // 2nd alert within 60s: suppressed
        store.push_alert("ERROR", "Connection failed", "embedding");
        assert_eq!(store.get_alerts().len(), 1);
        assert_eq!(store.suppressed_count(), 1);

        // 3rd alert within 60s: suppressed again
        store.push_alert("ERROR", "Connection failed", "embedding");
        assert_eq!(store.get_alerts().len(), 1);
        assert_eq!(store.suppressed_count(), 2);

        // Different component/message pair within 60s: recorded independently
        store.push_alert("ERROR", "Connection failed", "health");
        let alerts_after_diff = store.get_alerts();
        assert_eq!(alerts_after_diff.len(), 2);
        assert_eq!(alerts_after_diff[1].message, "Connection failed");
        assert_eq!(alerts_after_diff[1].component, "health");
        assert_eq!(store.suppressed_count(), 2);

        // Simulate passage of time (>60 seconds) for ("embedding", "Connection failed")
        {
            let mut map = store.suppression_map.lock().unwrap();
            let key = ("embedding".to_string(), "Connection failed".to_string());
            if let Some(record) = map.get_mut(&key) {
                record.last_seen = Instant::now() - std::time::Duration::from_secs(61);
            }
        }

        // Push alert after 60s window: recorded with [suppressed 2x] prefix
        store.push_alert("ERROR", "Connection failed", "embedding");
        let alerts_after_window = store.get_alerts();
        assert_eq!(alerts_after_window.len(), 3);
        assert_eq!(
            alerts_after_window[2].message,
            "[suppressed 2x] Connection failed"
        );
        // Total suppressed count remains cumulative (2)
        assert_eq!(store.suppressed_count(), 2);

        // Clearing store resets alerts and suppressed count
        store.clear();
        assert_eq!(store.get_alerts().len(), 0);
        assert_eq!(store.suppressed_count(), 0);
    }

    #[test]
    fn test_mode_derivation() {
        let store = SystemAlertStore::new();

        // Default should be LocalHealthy (assuming no env var overrides in test)
        std::env::set_var("XAVIER_PROVIDER", "local");
        assert_eq!(store.get_mode(), OperationalMode::LocalHealthy);

        // With LLM error, should be LocalDegraded
        store.push_alert("ERROR", "Ollama down", "llm");
        assert_eq!(store.get_mode(), OperationalMode::LocalDegraded);

        // If provider is cloud, should be CloudFallback
        std::env::set_var("XAVIER_PROVIDER", "openai");
        assert_eq!(store.get_mode(), OperationalMode::CloudFallback);

        // If provider is disabled, should be Disabled
        std::env::set_var("XAVIER_PROVIDER", "disabled");
        assert_eq!(store.get_mode(), OperationalMode::Disabled);

        std::env::remove_var("XAVIER_PROVIDER");
    }

    #[test]
    fn test_derive_operational_mode() {
        // LocalHealthy
        assert_eq!(
            SystemAlertStore::derive_operational_mode(true, true, "local"),
            OperationalMode::LocalHealthy
        );
        assert_eq!(
            SystemAlertStore::derive_operational_mode(true, true, "ollama"),
            OperationalMode::LocalHealthy
        );

        // LocalDegraded
        assert_eq!(
            SystemAlertStore::derive_operational_mode(false, true, "local"),
            OperationalMode::LocalDegraded
        );
        assert_eq!(
            SystemAlertStore::derive_operational_mode(true, false, "local"),
            OperationalMode::LocalDegraded
        );
        assert_eq!(
            SystemAlertStore::derive_operational_mode(false, false, "local"),
            OperationalMode::LocalDegraded
        );

        // CloudFallback
        assert_eq!(
            SystemAlertStore::derive_operational_mode(true, true, "openai"),
            OperationalMode::CloudFallback
        );
        assert_eq!(
            SystemAlertStore::derive_operational_mode(false, false, "anthropic"),
            OperationalMode::CloudFallback
        );

        // Disabled
        assert_eq!(
            SystemAlertStore::derive_operational_mode(true, true, "disabled"),
            OperationalMode::Disabled
        );
        assert_eq!(
            SystemAlertStore::derive_operational_mode(false, false, "disabled"),
            OperationalMode::Disabled
        );
    }
}
