//! # Log Detector
//!
//! Scheduled task that scans `service_logs` for error patterns.
//! Runs periodically (every 5 minutes by default) and triggers
//! the analyzer when patterns are found.
//!
//! ## Detection Rules
//!
//! - Same `module` + same message prefix repeated > 3 times in 1 hour → pattern
//! - Single module with >10 errors in 1 hour → burst alert
//! - New module with errors (never seen before) → new error alert

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

/// The log detector — scans for error patterns.
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
                    "New error detected: module '{}' — {}",
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
