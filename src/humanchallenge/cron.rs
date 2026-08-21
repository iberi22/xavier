//! HumanChallenge Cron & Background Task Runner
//!
//! Periodic scanner that periodically scans active sessions, persists candidate events
//! into `HumanChallengeStore`, and aggregates monthly X2 farming scores.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::crypto::hex_encode;
use crate::data_commons::reputation::reputation_weight;
use crate::humanchallenge::scanner::SessionScanner;
use crate::humanchallenge::store::HumanChallengeStore;
use crate::humanchallenge::types::{AnonymousMeshScore, FarmingSummary};
use crate::session::types::SessionEvent;

/// Configuration for HumanChallenge cron task
#[derive(Debug, Clone)]
pub struct HumanChallengeCronConfig {
    pub db_path: PathBuf,
    pub scan_interval: Duration,
    pub enabled: bool,
}

impl Default for HumanChallengeCronConfig {
    fn default() -> Self {
        Self {
            db_path: PathBuf::from(".xavier/humanchallenge.db"),
            scan_interval: Duration::from_secs(300), // 5 minutes default
            enabled: true,
        }
    }
}

pub struct HumanChallengeCron {
    config: HumanChallengeCronConfig,
    scanner: SessionScanner,
    store: Arc<HumanChallengeStore>,
}

impl HumanChallengeCron {
    pub fn new(config: HumanChallengeCronConfig) -> Result<Self, String> {
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

    /// Construct cron task with an existing store instance (e.g. for testing with in-memory DB)
    pub fn with_store(config: HumanChallengeCronConfig, store: HumanChallengeStore) -> Self {
        Self {
            config,
            scanner: SessionScanner::new(),
            store: Arc::new(store),
        }
    }

    /// Process a batch of session events, extract candidate challenges, and persist them.
    pub fn process_events(&self, events: &[SessionEvent]) -> Result<usize, String> {
        let candidates = self.scanner.scan_session_events(events);
        let count = candidates.len();

        for candidate in candidates {
            if let Err(e) = self.store.save_event(&candidate) {
                warn!("Failed to save HumanChallenge event candidate: {}", e);
            }
        }

        Ok(count)
    }

    /// Answer a challenge and award trust-weighted X2 farming points using data_commons reputation logic
    pub fn answer_and_award(
        &self,
        challenge_id: &str,
        response: &str,
        base_points: u32,
        wallet_id: &str,
    ) -> Result<bool, String> {
        // Calculate trust weight using data_commons::reputation
        let weight = reputation_weight(wallet_id);
        let final_points = base_points * (weight as u32);

        self.store
            .answer_challenge(challenge_id, response, final_points)
            .map_err(|e| e.to_string())
    }

    /// Scan active session events stored on disk (e.g., from failed-syncs or session logs)
    pub fn scan_pending_session_files(&self, sessions_dir: &PathBuf) -> Result<usize, String> {
        if !sessions_dir.exists() {
            return Ok(0);
        }

        let mut total_processed = 0;
        if let Ok(entries) = std::fs::read_dir(sessions_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(event) = serde_json::from_str::<SessionEvent>(&content) {
                            if let Ok(count) = self.process_events(&[event]) {
                                total_processed += count;
                            }
                        }
                    }
                }
            }
        }

        Ok(total_processed)
    }

    /// Get current monthly farming summary
    pub fn get_farming_summary(&self, year_month: &str) -> Result<FarmingSummary, String> {
        self.store
            .get_farming_summary(year_month)
            .map_err(|e| e.to_string())
    }

    /// Returns Privacy P4 compliant anonymous score summary for Mesh synchronization bounded by year_month
    pub fn prepare_mesh_scores(&self, year_month: &str) -> Result<Vec<AnonymousMeshScore>, String> {
        let events = self
            .store
            .list_events_by_month(year_month, 1000)
            .map_err(|e| e.to_string())?;

        let anonymous_scores = events
            .into_iter()
            .filter(|e| e.points_awarded > 0)
            .map(|e| {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(e.id.as_bytes());
                let id_hash = hex_encode(hasher.finalize());

                AnonymousMeshScore {
                    challenge_id_hash: id_hash,
                    challenge_type: e.challenge_type,
                    status: e.status,
                    timestamp: e.created_at,
                    points: e.points_awarded,
                }
            })
            .collect();

        Ok(anonymous_scores)
    }

    /// Spawns the background cron loop in a tokio task
    pub fn spawn_cron(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            info!("Starting HumanChallenge cron task loop");
            if self.config.enabled {
                let sessions_dir = PathBuf::from("failed-syncs");
                loop {
                    sleep(self.config.scan_interval).await;
                    info!("HumanChallenge cron heartbeat — scanning sessions");

                    if let Err(e) = self.scan_pending_session_files(&sessions_dir) {
                        warn!("HumanChallenge cron scan error: {}", e);
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::types::SessionEventType;
    use chrono::Utc;

    #[test]
    fn test_cron_process_events_and_mesh_scores() {
        let store = HumanChallengeStore::in_memory().unwrap();
        let config = HumanChallengeCronConfig::default();
        let cron = HumanChallengeCron::with_store(config, store);

        let events = vec![SessionEvent {
            session_id: "s_cron_1".into(),
            event_type: SessionEventType::Message,
            timestamp: Utc::now(),
            content: Some("Decidimos usar Rust para el scanner".into()),
            metadata: None,
        }];

        let count = cron.process_events(&events).unwrap();
        assert_eq!(count, 1);

        let candidates = cron.store.list_events(None, 10).unwrap();
        assert_eq!(candidates.len(), 1);

        // Answer candidate with data_commons reputation weighted reward
        let current_month = Utc::now().format("%Y-%m").to_string();
        cron.answer_and_award(&candidates[0].id, "Validado", 10, "0xwallet_test")
            .unwrap();

        let mesh_scores = cron.prepare_mesh_scores(&current_month).unwrap();
        assert_eq!(mesh_scores.len(), 1);
        assert!(mesh_scores[0].points > 0);
    }
}
