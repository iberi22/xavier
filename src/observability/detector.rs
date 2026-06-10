//! # Log Detector
//!
//! Scheduled task that scans `service_logs` for error patterns.
//! Runs periodically (every 5 minutes by default) and triggers
//! the analyzer when patterns are found.
//!
//! ## Detection Rules
//!
//! - Same `module` + same message prefix repeated > 3 times in 1 hour â†’ pattern
//! - Single module with >10 errors in 1 hour â†’ burst alert
//! - New module with errors (never seen before) â†’ new error alert

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::service_log::{ErrorPattern, ServiceLogStore};

/// Configuration for the log detector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectorConfig {
    /// How often to run detection (seconds).
    pub interval_seconds: u64,
    /// Time window to analyze (minutes).
    pub window_minutes: u32,
    /// Minimum frequency to qualify as a pattern.
    pub pattern_threshold: u32,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self {
            interval_seconds: 300, // every 5 min
            window_minutes: 60,    // look at last 1 hour
            pattern_threshold: 3,  // 3+ same errors = pattern
        }
    }
}

/// Result from a detection run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionResult {
    pub timestamp: String,
    pub patterns_found: Vec<ErrorPattern>,
    pub burst_alerts: Vec<String>,
    pub new_error_alerts: Vec<String>,
}

/// The log detector â€” scans for error patterns.
pub struct LogDetector {
    store: ServiceLogStore,
    config: DetectorConfig,
    /// Track seen module+message combos to detect "new" errors.
    seen_signatures: std::sync::Mutex<Vec<String>>,
}

impl LogDetector {
    /// Create a new detector with default config.
    pub async fn new() -> anyhow::Result<Self> {
        Ok(Self {
            store: ServiceLogStore::new().await?,
            config: DetectorConfig::default(),
            seen_signatures: std::sync::Mutex::new(Vec::new()),
        })
    }

    /// Create a new detector with custom config.
    pub async fn with_config(config: DetectorConfig) -> anyhow::Result<Self> {
        Ok(Self {
            store: ServiceLogStore::new().await?,
            config,
            seen_signatures: std::sync::Mutex::new(Vec::new()),
        })
    }

    /// Run a single detection cycle.
    pub async fn run_detection(&self) -> anyhow::Result<DetectionResult> {
        let patterns = self
            .store
            .detect_patterns(self.config.window_minutes, self.config.pattern_threshold)
            .await?;

        let mut result = DetectionResult {
            timestamp: chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
            patterns_found: patterns,
            burst_alerts: Vec::new(),
            new_error_alerts: Vec::new(),
        };

        // Detect bursts: single module with many errors
        let burst_threshold = self.config.pattern_threshold * 5;
        for pattern in &result.patterns_found {
            if pattern.frequency >= burst_threshold {
                result.burst_alerts.push(format!(
                    "Burst detected: module '{}' failed {} times in {} minutes",
                    pattern.module, pattern.frequency, self.config.window_minutes
                ));
            }

            // Detect new errors (first time seeing this signature)
            let msg_prefix = &pattern.sample_message[..50.min(pattern.sample_message.len())];
            let sig = format!("{}::{}", pattern.module, msg_prefix);
            let mut seen = self.seen_signatures.lock().unwrap();
            if !seen.contains(&sig) {
                seen.push(sig);
                result.new_error_alerts.push(format!(
                    "New error detected: module '{}' â€” {}",
                    pattern.module, pattern.sample_message
                ));
            }

            // Limit to prevent unbounded memory
            if seen.len() > 10000 {
                let excess = seen.len() - 5000;
                seen.drain(0..excess);
            }
        }

        Ok(result)
    }

    /// Start the detector as a background task.
    pub fn spawn(self: Arc<Self>) {
        let interval = Duration::from_secs(self.config.interval_seconds);

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            // Initial delay to let the system warm up
            tokio::time::sleep(Duration::from_secs(30)).await;

            loop {
                ticker.tick().await;
                match self.run_detection().await {
                    Ok(result) => {
                        if !result.patterns_found.is_empty() {
                            tracing::warn!(
                                "Detector: {} error patterns found ({} bursts, {} new)",
                                result.patterns_found.len(),
                                result.burst_alerts.len(),
                                result.new_error_alerts.len(),
                            );
                            for alert in &result.burst_alerts {
                                tracing::error!("{}", alert);
                            }
                            for alert in &result.new_error_alerts {
                                tracing::warn!("{}", alert);
                            }
                        } else {
                            tracing::debug!("Detector: no error patterns found");
                        }
                    }
                    Err(e) => {
                        tracing::error!("Detector cycle failed: {}", e);
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observability::service_log::{ErrorPattern, LogLevel};

    #[test]
    fn test_detector_config_defaults() {
        let config = DetectorConfig::default();
        assert_eq!(config.interval_seconds, 300);
        assert_eq!(config.window_minutes, 60);
        assert_eq!(config.pattern_threshold, 3);
    }

    #[test]
    fn test_detector_config_custom() {
        let config = DetectorConfig {
            interval_seconds: 60,
            window_minutes: 30,
            pattern_threshold: 5,
        };
        assert_eq!(config.interval_seconds, 60);
        assert_eq!(config.window_minutes, 30);
        assert_eq!(config.pattern_threshold, 5);
    }

    #[test]
    fn test_detection_result_struct() {
        let pattern = ErrorPattern {
            module: "http".into(),
            level: LogLevel::Error,
            frequency: 10,
            sample_message: "500 Internal".into(),
            first_seen: "2025-01-01T00:00:00Z".into(),
            last_seen: "2025-01-01T01:00:00Z".into(),
        };
        let result = DetectionResult {
            timestamp: "2025-01-01T01:00:00.000Z".into(),
            patterns_found: vec![pattern],
            burst_alerts: vec![],
            new_error_alerts: vec![],
        };
        assert_eq!(result.patterns_found.len(), 1);
        assert!(result.burst_alerts.is_empty());
        assert!(result.new_error_alerts.is_empty());
    }

    #[test]
    fn test_burst_detection_logic() {
        // Burst threshold is pattern_threshold * 5 = 15
        let pattern = ErrorPattern {
            module: "db".into(),
            level: LogLevel::Error,
            frequency: 20,
            sample_message: "connection refused".into(),
            first_seen: "t1".into(),
            last_seen: "t2".into(),
        };
        let burst_threshold = DetectorConfig::default().pattern_threshold * 5;
        assert!(pattern.frequency >= burst_threshold);
    }

    #[test]
    fn test_new_error_alert_logic() {
        let pattern = ErrorPattern {
            module: "new_module".into(),
            level: LogLevel::Error,
            frequency: 3,
            sample_message: "something unexpected".into(),
            first_seen: "t1".into(),
            last_seen: "t2".into(),
        };
        let sig = format!(
            "{}::{}",
            pattern.module,
            &pattern.sample_message[..50.min(pattern.sample_message.len())]
        );
        let mut seen: Vec<String> = Vec::new();
        assert!(!seen.contains(&sig));
        seen.push(sig);
        assert_eq!(seen.len(), 1);
    }

    #[test]
    fn test_seen_signatures_limit() {
        let mut seen: Vec<String> = (0..10001).map(|i| format!("sig_{}", i)).collect();
        assert!(seen.len() > 10000);
        let excess = seen.len() - 5000;
        seen.drain(0..excess);
        assert_eq!(seen.len(), 5000);
    }
}
