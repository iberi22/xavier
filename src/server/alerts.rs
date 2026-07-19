use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::RwLock;

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
