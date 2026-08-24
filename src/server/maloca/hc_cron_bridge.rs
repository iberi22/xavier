//! HumanChallenge Cron Bridge for Maloca
//!
//! Periodically scans agent interaction sessions, generates candidate challenges
//! across the 5 canonical types (Contradiction, Decision, Execution, Assumption, Clarification),
//! deduplicates and rate-limits them, and persists candidate events into `HumanChallengeStore`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tracing::{info, warn};

use crate::humanchallenge::scanner::SessionScanner;
use crate::humanchallenge::store::HumanChallengeStore;
use crate::humanchallenge::types::{FarmingSummary, HumanChallengeEvent};
use crate::session::types::SessionEvent;

/// Configuration for the Maloca HumanChallenge Cron Bridge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HcCronBridgeConfig {
    /// Path to the SQLite database file for HumanChallenge persistence
    pub db_path: PathBuf,
    /// Directory containing agent interaction session JSON files
    pub sessions_dir: PathBuf,
    /// Interval between background harvesting cycles
    pub scan_interval: Duration,
    /// Maximum candidate events to process and persist per harvest cycle (rate limiting)
    pub max_events_per_scan: usize,
    /// Whether background periodic scanning is enabled
    pub enabled: bool,
}

impl Default for HcCronBridgeConfig {
    fn default() -> Self {
        Self {
            db_path: PathBuf::from(".xavier/humanchallenge.db"),
            sessions_dir: PathBuf::from("sessions"),
            scan_interval: Duration::from_secs(300), // 5 minutes default
            max_events_per_scan: 100,
            enabled: true,
        }
    }
}

/// Periodic Session Event Scanner & Challenge Harvester Bridge
pub struct HcCronBridge {
    config: HcCronBridgeConfig,
    scanner: SessionScanner,
    store: Arc<HumanChallengeStore>,
}

impl HcCronBridge {
    /// Create a new `HcCronBridge` with the specified configuration, initializing the SQLite store
    pub fn new(config: HcCronBridgeConfig) -> Result<Self, String> {
        if let Some(parent) = config.db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let store = HumanChallengeStore::new(&config.db_path)
            .map_err(|e| format!("Failed to initialize HumanChallenge DB: {}", e))?;

        Ok(Self {
            config,
            scanner: SessionScanner::new(),
            store: Arc::new(store),
        })
    }

    /// Construct `HcCronBridge` with an existing `HumanChallengeStore` (useful for testing)
    pub fn with_store(config: HcCronBridgeConfig, store: HumanChallengeStore) -> Self {
        Self {
            config,
            scanner: SessionScanner::new(),
            store: Arc::new(store),
        }
    }

    /// Return reference to inner `HumanChallengeStore`
    pub fn store(&self) -> Arc<HumanChallengeStore> {
        self.store.clone()
    }

    /// Return configuration
    pub fn config(&self) -> &HcCronBridgeConfig {
        &self.config
    }

    /// Process a batch of session events, extract candidate challenges, rate-limit, and deduplicate into SQLite
    pub fn process_session_events(&self, events: &[SessionEvent]) -> Result<usize, String> {
        let mut candidates = self.scanner.scan_session_events(events);

        // Enforce rate-limiting cap per cycle
        if candidates.len() > self.config.max_events_per_scan {
            candidates.truncate(self.config.max_events_per_scan);
        }

        let mut saved_count = 0;
        for candidate in candidates {
            // save_event uses SQLite INSERT OR IGNORE for deduplication
            match self.store.save_event(&candidate) {
                Ok(_) => saved_count += 1,
                Err(e) => warn!("Failed to save candidate challenge {}: {}", candidate.id, e),
            }
        }

        Ok(saved_count)
    }

    /// Scan JSON session files in `sessions_dir` and extract candidate events
    pub fn scan_sessions_directory(&self, dir: &Path) -> Result<usize, String> {
        if !dir.exists() {
            return Ok(0);
        }

        let mut total_saved = 0;
        let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let mut events = Vec::new();

                    // Try parsing as array of events first, fallback to single event
                    if let Ok(vec_events) = serde_json::from_str::<Vec<SessionEvent>>(&content) {
                        events = vec_events;
                    } else if let Ok(single_event) = serde_json::from_str::<SessionEvent>(&content)
                    {
                        events.push(single_event);
                    }

                    if !events.is_empty() {
                        if let Ok(saved) = self.process_session_events(&events) {
                            total_saved += saved;
                        }
                    }
                }
            }
        }

        Ok(total_saved)
    }

    /// Execute a single harvesting cycle
    pub fn run_harvest_cycle(&self) -> Result<usize, String> {
        self.scan_sessions_directory(&self.config.sessions_dir)
    }

    /// Get monthly farming summary
    pub fn get_farming_summary(&self, year_month: &str) -> Result<FarmingSummary, String> {
        self.store
            .get_farming_summary(year_month)
            .map_err(|e| e.to_string())
    }

    /// Spawns the background worker task using Tokio interval loop, running filesystem & SQLite tasks non-blocking
    pub fn start_background_worker(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            if !self.config.enabled {
                info!("HcCronBridge background worker disabled in config");
                return;
            }

            info!("Starting HcCronBridge background harvester worker");
            let mut timer = interval(self.config.scan_interval);

            loop {
                timer.tick().await;

                let bridge = self.clone();
                let result = tokio::task::spawn_blocking(move || bridge.run_harvest_cycle()).await;

                match result {
                    Ok(Ok(count)) => {
                        info!(
                            "HcCronBridge harvest cycle completed, saved {} candidate events",
                            count
                        );
                    }
                    Ok(Err(e)) => {
                        warn!("HcCronBridge harvest cycle error: {}", e);
                    }
                    Err(e) => {
                        warn!("HcCronBridge harvest cycle panic/cancellation: {}", e);
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::humanchallenge::types::ChallengeType;
    use crate::session::types::SessionEventType;
    use chrono::Utc;
    use tempfile::TempDir;

    #[test]
    fn test_hc_cron_bridge_process_events_and_dedup() {
        let store = HumanChallengeStore::in_memory().unwrap();
        let config = HcCronBridgeConfig {
            enabled: true,
            max_events_per_scan: 10,
            ..Default::default()
        };
        let bridge = HcCronBridge::with_store(config, store);

        let events = vec![
            SessionEvent {
                session_id: "s1".into(),
                event_type: SessionEventType::Message,
                timestamp: Utc::now(),
                content: Some("Sin embargo esto contradice lo anterior".into()),
                metadata: None,
            },
            SessionEvent {
                session_id: "s1".into(),
                event_type: SessionEventType::Message,
                timestamp: Utc::now(),
                content: Some("Decidimos usar la arquitectura hexagonal".into()),
                metadata: None,
            },
            SessionEvent {
                session_id: "s1".into(),
                event_type: SessionEventType::ToolCall,
                timestamp: Utc::now(),
                content: Some("sudo systemctl restart xavier".into()),
                metadata: None,
            },
            SessionEvent {
                session_id: "s1".into(),
                event_type: SessionEventType::Message,
                timestamp: Utc::now(),
                content: Some("Asumiendo que el puerto 8006 esta libre".into()),
                metadata: None,
            },
            SessionEvent {
                session_id: "s1".into(),
                event_type: SessionEventType::Message,
                timestamp: Utc::now(),
                content: Some("Por favor aclara la configuracion de la base de datos".into()),
                metadata: None,
            },
        ];

        let saved = bridge.process_session_events(&events).unwrap();
        assert_eq!(saved, 5);

        let stored_events = bridge.store().list_events(None, 10).unwrap();
        assert_eq!(stored_events.len(), 5);

        // Check that all 5 canonical types were detected
        let types: Vec<_> = stored_events.iter().map(|e| e.challenge_type).collect();
        assert!(types.contains(&ChallengeType::Contradiction));
        assert!(types.contains(&ChallengeType::Decision));
        assert!(types.contains(&ChallengeType::Execution));
        assert!(types.contains(&ChallengeType::Assumption));
        assert!(types.contains(&ChallengeType::Clarification));
    }

    #[test]
    fn test_hc_cron_bridge_rate_limit() {
        let store = HumanChallengeStore::in_memory().unwrap();
        let config = HcCronBridgeConfig {
            max_events_per_scan: 2,
            ..Default::default()
        };
        let bridge = HcCronBridge::with_store(config, store);

        let events = vec![
            SessionEvent {
                session_id: "s1".into(),
                event_type: SessionEventType::Message,
                timestamp: Utc::now(),
                content: Some("Sin embargo contradice".into()),
                metadata: None,
            },
            SessionEvent {
                session_id: "s1".into(),
                event_type: SessionEventType::Message,
                timestamp: Utc::now(),
                content: Some("Decidimos usar Rust".into()),
                metadata: None,
            },
            SessionEvent {
                session_id: "s1".into(),
                event_type: SessionEventType::Message,
                timestamp: Utc::now(),
                content: Some("Asumiendo parametro valido".into()),
                metadata: None,
            },
        ];

        let saved = bridge.process_session_events(&events).unwrap();
        assert_eq!(saved, 2); // capped at max_events_per_scan = 2
    }

    #[test]
    fn test_hc_cron_bridge_directory_scan() {
        let temp_dir = TempDir::new().unwrap();
        let sessions_path = temp_dir.path().join("sessions");
        std::fs::create_dir_all(&sessions_path).unwrap();

        let event = SessionEvent {
            session_id: "sess_dir_01".into(),
            event_type: SessionEventType::Message,
            timestamp: Utc::now(),
            content: Some("Decidimos implementar HcCronBridge".into()),
            metadata: None,
        };

        let file_path = sessions_path.join("session_01.json");
        std::fs::write(&file_path, serde_json::to_string(&event).unwrap()).unwrap();

        let store = HumanChallengeStore::in_memory().unwrap();
        let config = HcCronBridgeConfig {
            sessions_dir: sessions_path,
            ..Default::default()
        };
        let bridge = HcCronBridge::with_store(config, store);

        let count = bridge.run_harvest_cycle().unwrap();
        assert_eq!(count, 1);

        let list = bridge.store().list_events(None, 10).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].challenge_type, ChallengeType::Decision);
    }
}
