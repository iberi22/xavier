//! NightScheduler cron task for automated mini-expert training
//!
//! Schedules and triggers mini-expert LLM fine-tuning jobs during configured night windows
//! when sufficient curated human challenge datasets are ready.

use chrono::{DateTime, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn};

use crate::humanchallenge::curation_gate::{CurationGate, ReadinessThresholds};
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
        let curation_gate = Arc::new(CurationGate::new(
            store.clone(),
            ReadinessThresholds {
                min_examples_per_domain: config.min_curated_items,
                ..ReadinessThresholds::default()
            },
        ));
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
        let readiness = self.curation_gate.check_readiness();
        if !readiness.is_ready {
            return Err(format!("Curation gate not ready: {}", readiness.message));
        }
        Ok(TrainingJob::new(
            self.config.compute_provider.clone(),
            ".xavier/datasets/night_training_curated.json",
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
    use crate::humanchallenge::types::{
        ChallengeType, CurationVerdict, CurationVote, HumanChallengeEvent,
    };

    fn create_test_store() -> Arc<HumanChallengeStore> {
        Arc::new(HumanChallengeStore::in_memory().unwrap())
    }

    fn test_gate(store: &Arc<HumanChallengeStore>, min: usize) -> Arc<CurationGate> {
        Arc::new(
            CurationGate::new(
                store.clone(),
                ReadinessThresholds {
                    min_examples_per_domain: min,
                    ..ReadinessThresholds::default()
                },
            )
            .without_queue(),
        )
    }

    fn add_curated_votes(store: &HumanChallengeStore, n: usize) {
        for i in 0..n {
            let event = HumanChallengeEvent::new(
                format!("s_{}", i),
                ChallengeType::Decision,
                "Test prompt",
                "Raw content for decision",
                0.95,
            );
            store.save_event(&event).unwrap();
            let vote = CurationVote::new(
                &event.id,
                CurationVerdict::Accept,
                None,
                true,
                vec!["rust".to_string()],
                true,
            );
            store.save_curation_vote(&vote).unwrap();
        }
    }

    fn config(start: u32, end: u32, provider: ComputeProvider) -> NightTrainerConfig {
        NightTrainerConfig {
            training_window_start: start,
            training_window_end: end,
            min_curated_items: 10,
            compute_provider: provider,
        }
    }

    #[test]
    fn test_training_window_hours() {
        let store = create_test_store();
        let cfg = config(2, 6, ComputeProvider::Local);
        let gate = test_gate(&store, cfg.min_curated_items);
        let trainer = NightTrainer::new(cfg, gate, store);

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
        add_curated_votes(&store, 15);

        let cfg = config(2, 6, ComputeProvider::Local);
        let gate = test_gate(&store, cfg.min_curated_items);
        let trainer = NightTrainer::new(cfg, gate, store);

        assert!(trainer.curation_gate.check_readiness().is_ready);

        if !trainer.is_training_window() {
            assert!(!trainer.should_train());
        } else {
            let current_hour = Utc::now().hour();
            let disjoint_config = config(
                (current_hour + 12) % 24,
                (current_hour + 13) % 24,
                ComputeProvider::Local,
            );
            let disjoint_trainer = NightTrainer::new(
                disjoint_config,
                test_gate(&trainer.store, 10),
                trainer.store.clone(),
            );
            assert!(!disjoint_trainer.should_train());
        }
    }

    #[test]
    fn test_prepare_job_not_ready_below_threshold() {
        let store = create_test_store();
        add_curated_votes(&store, 9);
        let cfg = config(0, 23, ComputeProvider::Local);
        let gate = test_gate(&store, cfg.min_curated_items);
        let trainer = NightTrainer::new(cfg, gate, store);
        assert!(trainer.prepare_job().is_err());
    }

    #[test]
    fn test_prepare_job_structure() {
        let store = create_test_store();
        add_curated_votes(&store, 10);

        let cfg = config(0, 23, ComputeProvider::Modal);
        let gate = test_gate(&store, cfg.min_curated_items);
        let trainer = NightTrainer::new(cfg, gate, store);

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
