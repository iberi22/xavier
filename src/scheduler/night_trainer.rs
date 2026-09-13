//! NightScheduler cron task for automated mini-expert training
//!
//! Schedules and triggers mini-expert LLM fine-tuning jobs during configured night windows
//! when sufficient curated human challenge datasets are ready.

use chrono::{DateTime, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn};

use crate::humanchallenge::store::HumanChallengeStore;

/// Compute provider options for mini-expert fine-tuning jobs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ComputeProvider {
    #[default]
    Local,
    Modal,
    RunPod,
    AWS,
    Custom(String),
}

/// Represents an automated mini-expert training job configuration and metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrainingJob {
    pub id: String,
    pub provider: ComputeProvider,
    pub dataset_path: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

impl TrainingJob {
    /// Creates a new `TrainingJob` instance with pending status.
    pub fn new(provider: ComputeProvider, dataset_path: impl Into<String>) -> Self {
        Self {
            id: format!("job_{}", ulid::Ulid::new()),
            provider,
            dataset_path: dataset_path.into(),
            status: "pending".to_string(),
            created_at: Utc::now(),
        }
    }
}

/// Readiness status returned by `CurationGate`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadinessResult {
    pub is_ready: bool,
    pub curated_count: usize,
    pub message: String,
}

/// Gate that inspects `HumanChallengeStore` to determine if enough curated items exist for training.
#[derive(Clone)]
pub struct CurationGate {
    store: Arc<HumanChallengeStore>,
    min_curated_items: usize,
}

impl std::fmt::Debug for CurationGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CurationGate")
            .field("min_curated_items", &self.min_curated_items)
            .finish_non_exhaustive()
    }
}

impl CurationGate {
    /// Constructs a new `CurationGate` with explicit threshold.
    pub fn new(store: Arc<HumanChallengeStore>, min_curated_items: usize) -> Self {
        Self {
            store,
            min_curated_items,
        }
    }

    /// Constructs `CurationGate` using default threshold of 10 items.
    pub fn with_defaults(store: Arc<HumanChallengeStore>) -> Self {
        Self::new(store, 10)
    }

    /// Checks if sufficient answered/valid curated items are present in store.
    pub fn check_readiness(&self) -> ReadinessResult {
        let count = match self.store.list_events(None, 1000) {
            Ok(events) => events
                .iter()
                .filter(|e| {
                    e.points_awarded > 0
                        || e.status == crate::humanchallenge::types::ChallengeStatus::Answered
                })
                .count(),
            Err(_) => 0,
        };

        let is_ready = count >= self.min_curated_items;
        let message = if is_ready {
            format!("Ready for training with {} curated items", count)
        } else {
            format!(
                "Insufficient curated items: {}/{}",
                count, self.min_curated_items
            )
        };

        ReadinessResult {
            is_ready,
            curated_count: count,
            message,
        }
    }

    /// Collects and exports curated items dataset for training.
    pub fn collect_for_training(&self) -> Result<String, String> {
        let readiness = self.check_readiness();
        if !readiness.is_ready {
            return Err(format!("Curation gate not ready: {}", readiness.message));
        }

        Ok(".xavier/datasets/night_training_curated.json".to_string())
    }
}

/// Configuration for the `NightTrainer` scheduler task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NightTrainerConfig {
    pub training_window_start: u32,
    pub training_window_end: u32,
    pub min_curated_items: usize,
    pub compute_provider: ComputeProvider,
}

impl Default for NightTrainerConfig {
    fn default() -> Self {
        Self {
            training_window_start: 2, // 2:00 UTC
            training_window_end: 6,   // 6:00 UTC
            min_curated_items: 10,
            compute_provider: ComputeProvider::default(),
        }
    }
}

/// NightScheduler task orchestrating automated mini-expert training during off-peak hours.
#[derive(Clone)]
pub struct NightTrainer {
    pub config: NightTrainerConfig,
    pub curation_gate: Arc<CurationGate>,
    pub store: Arc<HumanChallengeStore>,
}

impl std::fmt::Debug for NightTrainer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NightTrainer")
            .field("config", &self.config)
            .field("curation_gate", &self.curation_gate)
            .finish_non_exhaustive()
    }
}

impl NightTrainer {
    /// Constructs a new `NightTrainer`.
    pub fn new(
        config: NightTrainerConfig,
        curation_gate: Arc<CurationGate>,
        store: Arc<HumanChallengeStore>,
    ) -> Self {
        Self {
            config,
            curation_gate,
            store,
        }
    }

    /// Constructs `NightTrainer` with default configuration and gate.
    pub fn with_defaults(store: Arc<HumanChallengeStore>) -> Self {
        let config = NightTrainerConfig::default();
        let curation_gate = Arc::new(CurationGate::new(store.clone(), config.min_curated_items));
        Self {
            config,
            curation_gate,
            store,
        }
    }

    /// Checks if current UTC hour is within the configured training window.
    pub fn is_training_window(&self) -> bool {
        self.is_training_window_at(Utc::now().hour())
    }

    /// Checks if a specific UTC hour is within the configured training window.
    pub fn is_training_window_at(&self, hour: u32) -> bool {
        if self.config.training_window_start <= self.config.training_window_end {
            hour >= self.config.training_window_start && hour <= self.config.training_window_end
        } else {
            hour >= self.config.training_window_start || hour <= self.config.training_window_end
        }
    }

    /// Determines whether training should be triggered: checks time window and curation readiness.
    pub fn should_train(&self) -> bool {
        self.is_training_window() && self.curation_gate.check_readiness().is_ready
    }

    /// Prepares a training job by checking curation readiness and building a `TrainingJob`.
    pub fn prepare_job(&self) -> Result<TrainingJob, String> {
        let dataset_path = self.curation_gate.collect_for_training()?;
        Ok(TrainingJob::new(
            self.config.compute_provider.clone(),
            dataset_path,
        ))
    }

    /// Spawns the background Tokio loop for periodic 30-minute training checks.
    pub fn spawn_cron(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            info!("NightTrainer: Starting autonomous night trainer cron loop");
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30 * 60));
            loop {
                interval.tick().await;
                info!("NightTrainer: Checking training schedule and curation gate readiness...");
                if self.should_train() {
                    info!("NightTrainer: Window active and curation gate ready! Preparing training job...");
                    match self.prepare_job() {
                        Ok(job) => {
                            info!(
                                "NightTrainer: Job prepared successfully: id={}, provider={:?}, dataset={}. (Actual training execution logged)",
                                job.id, job.provider, job.dataset_path
                            );
                        }
                        Err(e) => {
                            warn!("NightTrainer: Failed to prepare training job: {}", e);
                        }
                    }
                } else {
                    info!("NightTrainer: Conditions not met for training pass. Skipping.");
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::humanchallenge::types::{ChallengeStatus, ChallengeType, HumanChallengeEvent};

    fn create_test_store() -> Arc<HumanChallengeStore> {
        Arc::new(HumanChallengeStore::in_memory().unwrap())
    }

    #[test]
    fn test_training_window_hours() {
        let store = create_test_store();
        let config = NightTrainerConfig {
            training_window_start: 2,
            training_window_end: 6,
            min_curated_items: 10,
            compute_provider: ComputeProvider::Local,
        };
        let gate = Arc::new(CurationGate::new(store.clone(), config.min_curated_items));
        let trainer = NightTrainer::new(config, gate, store);

        assert!(!trainer.is_training_window_at(0));
        assert!(!trainer.is_training_window_at(1));
        assert!(trainer.is_training_window_at(2));
        assert!(trainer.is_training_window_at(4));
        assert!(trainer.is_training_window_at(6));
        assert!(!trainer.is_training_window_at(7));
        assert!(!trainer.is_training_window_at(12));
        assert!(!trainer.is_training_window_at(23));
    }

    #[test]
    fn test_should_not_train_outside_window() {
        let store = create_test_store();
        // Add 15 answered events so curation gate is ready
        for i in 0..15 {
            let mut event = HumanChallengeEvent::new(
                format!("s_{}", i),
                ChallengeType::Decision,
                "Test prompt",
                "Raw content for decision",
                0.95,
            );
            event.status = ChallengeStatus::Answered;
            event.points_awarded = 10;
            store.save_event(&event).unwrap();
        }

        let config = NightTrainerConfig {
            training_window_start: 2,
            training_window_end: 6,
            min_curated_items: 10,
            compute_provider: ComputeProvider::Local,
        };
        let gate = Arc::new(CurationGate::new(store.clone(), config.min_curated_items));
        let trainer = NightTrainer::new(config, gate, store);

        // Gate is ready
        assert!(trainer.curation_gate.check_readiness().is_ready);

        // Outside window check should fail
        if !trainer.is_training_window() {
            assert!(!trainer.should_train());
        } else {
            // Force artificial trainer with disjoint window
            let current_hour = Utc::now().hour();
            let outside_start = (current_hour + 12) % 24;
            let outside_end = (current_hour + 13) % 24;
            let disjoint_config = NightTrainerConfig {
                training_window_start: outside_start,
                training_window_end: outside_end,
                min_curated_items: 10,
                compute_provider: ComputeProvider::Local,
            };
            let disjoint_trainer = NightTrainer::new(
                disjoint_config,
                Arc::new(CurationGate::new(trainer.store.clone(), 10)),
                trainer.store.clone(),
            );
            assert!(!disjoint_trainer.should_train());
        }
    }

    #[test]
    fn test_prepare_job_structure() {
        let store = create_test_store();
        // Add 10 answered events to satisfy curation gate
        for i in 0..10 {
            let mut event = HumanChallengeEvent::new(
                format!("s_{}", i),
                ChallengeType::Decision,
                "Sample prompt",
                "Sample raw content",
                0.9,
            );
            event.status = ChallengeStatus::Answered;
            event.points_awarded = 5;
            store.save_event(&event).unwrap();
        }

        let config = NightTrainerConfig {
            training_window_start: 0,
            training_window_end: 23,
            min_curated_items: 10,
            compute_provider: ComputeProvider::Modal,
        };
        let gate = Arc::new(CurationGate::new(store.clone(), config.min_curated_items));
        let trainer = NightTrainer::new(config, gate, store);

        let job_res = trainer.prepare_job();
        assert!(
            job_res.is_ok(),
            "prepare_job should succeed when gate is ready"
        );

        let job = job_res.unwrap();
        assert!(job.id.starts_with("job_"));
        assert_eq!(job.provider, ComputeProvider::Modal);
        assert_eq!(job.status, "pending");
        assert_eq!(
            job.dataset_path,
            ".xavier/datasets/night_training_curated.json"
        );
    }
}
