use chrono::{DateTime, Utc};
use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::domain::cycle_breaks::w30_04::SuppressionRecord;
pub use crate::domain::cycle_breaks::w30_04::{OperationalMode, SystemAlert, SystemAlertStore};

impl SystemAlertStore {
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

        let island_id = if level.eq_ignore_ascii_case("error") {
            crate::notifications::IslandId::Errors
        } else {
            crate::notifications::IslandId::System
        };
        let severity = match level.to_ascii_uppercase().as_str() {
            "ERROR" => "error",
            "WARN" | "WARNING" => "warning",
            _ => "info",
        };
        let title = format!(
            "[{}] {}",
            component.to_ascii_uppercase(),
            level.to_ascii_uppercase()
        );
        // Emit alert notification event (dispatches via NOTIFICATIONS.send to providers and listeners)
        crate::notifications::emit_island_event_sync(island_id, title, message, severity);
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
    ///
    /// `exclude_id` is the id of the notification that triggered this check. The
    /// in-app provider runs before the email provider and writes the *same*
    /// event to the shared `notifications` table; without excluding that row the
    /// dedup query would see the event's own in-app row and self-suppress every
    /// first email. Excluding the current id keeps genuine repeat-suppression
    /// (a *different* row for the same title within the window) intact.
    pub async fn should_notify_email_async(
        &self,
        alert_key: &str,
        exclude_id: &str,
        window_secs: u64,
    ) -> bool {
        let key = crate::notifications::db::resolve_key(None).ok().flatten();
        self.should_notify_email_async_with_key(alert_key, exclude_id, window_secs, key.as_ref())
            .await
    }

    /// Check whether an email notification should be sent with an explicit key handle.
    pub async fn should_notify_email_async_with_key(
        &self,
        alert_key: &str,
        exclude_id: &str,
        window_secs: u64,
        key: Option<&crate::espacio::keys::KeyHandle>,
    ) -> bool {
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
        let exclude_owned = exclude_id.to_string();
        let dek = key.cloned();
        let db_last_ts: Option<DateTime<Utc>> = crate::codebase::connection_manager::ConnectionManager::global()
            .with_conn("memory", move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, title, body, timestamp FROM notifications WHERE id != ?1 ORDER BY timestamp DESC LIMIT 200",
                )?;
                let mut rows = stmt.query([exclude_owned])?;
                while let Some(row) = rows.next()? {
                    let row_id: String = row.get(0)?;
                    let raw_title: String = row.get(1)?;
                    let raw_body: String = row.get(2)?;
                    let ts_str: String = row.get(3)?;

                    let opened_title = match crate::notifications::db::open_notification_text(
                        dek.as_ref(),
                        &row_id,
                        &raw_title,
                        &raw_body,
                    ) {
                        Ok((t, _)) => t,
                        Err(e) => {
                            tracing::debug!("failed to open notification for dedup check: {e}");
                            continue;
                        }
                    };

                    if opened_title == key_owned {
                        return Ok(ts_str.parse::<DateTime<Utc>>().ok());
                    }
                }
                Ok(None)
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

pub use crate::domain::cycle_breaks::w30_04::SYSTEM_ALERTS;

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

    #[tokio::test]
    async fn test_alerts_dedupe_window_with_sealed_titles() {
        use crate::espacio::keys::{KeyRing, StaticNodeKek, UnlockMode};
        let dir = tempfile::tempdir().unwrap();
        let keys_dir = tempfile::tempdir().unwrap();
        let ring = KeyRing::new(
            keys_dir.path(),
            std::sync::Arc::new(StaticNodeKek::new([0x42; 32])),
        );
        let dek = ring
            .create_keys("default", UnlockMode::NodeUnlock, None)
            .unwrap()
            .0;

        let db_path = dir.path().join("alerts_test.sqlite3");
        let cm = crate::codebase::connection_manager::ConnectionManager::global();
        cm.disconnect("memory");
        cm.connect_with_path("memory", db_path).unwrap();
        crate::notifications::db::init_notifications_schema()
            .await
            .unwrap();

        let store = SystemAlertStore::new();
        let alert_title = "Sealed Storage Disk Failure";

        let now_str = Utc::now().to_rfc3339();
        let inserted_id = "alert-notif-1";
        cm.with_conn("memory", {
            let dek = dek.clone();
            let now_str = now_str.clone();
            move |conn| {
                crate::notifications::db::insert_notification_sealed(
                    conn,
                    Some(&dek),
                    inserted_id,
                    "system",
                    alert_title,
                    "disk failure body",
                    &now_str,
                    0,
                    "error",
                )
            }
        })
        .await
        .unwrap();

        // 1. Should dedupe when the title matches and id != exclude_id
        let should_send = store
            .should_notify_email_async_with_key(alert_title, "different-id", 300, Some(&dek))
            .await;
        assert!(
            !should_send,
            "sealed alert title within window must be deduplicated"
        );

        // 2. Self-notification (id == exclude_id) should not be self-suppressed
        let should_send_self = store
            .should_notify_email_async_with_key(alert_title, inserted_id, 300, Some(&dek))
            .await;
        assert!(
            should_send_self,
            "alert with own exclude_id must not self-suppress"
        );

        // 3. Different alert title should not be suppressed
        let should_send_diff = store
            .should_notify_email_async_with_key("Other Alert", "different-id", 300, Some(&dek))
            .await;
        assert!(
            should_send_diff,
            "different alert title must not be suppressed"
        );

        // 4. Also works via global injected key
        crate::notifications::db::set_injected_key_for_test(Some(dek.clone()));
        let should_send_injected = store
            .should_notify_email_async(alert_title, "different-id", 300)
            .await;
        assert!(!should_send_injected, "dedupe should work via injected key");
        crate::notifications::db::set_injected_key_for_test(None);

        cm.disconnect("memory");
    }
}
