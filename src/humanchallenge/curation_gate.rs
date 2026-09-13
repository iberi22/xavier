//! CurationGate — decides if curated HumanChallenge data is ready for TrainingExporter.
//!
//! Connects the HumanChallenge pipeline with the training pipeline:
//! curated votes → TrainingReadinessGate check → trigger export.

use crate::humanchallenge::store::HumanChallengeStore;
use crate::humanchallenge::types::{CurationVote, TrainingReadinessGate};

/// Result of a readiness check.
#[derive(Debug, Clone)]
pub struct ReadinessCheckResult {
    pub is_ready: bool,
    pub eligible_count: usize,
    pub fact_verified_count: usize,
    pub training_eligible_count: usize,
    pub domain_tags: Vec<String>,
    pub message: String,
}

/// CurationGate orchestrates checking readiness and collecting the bundle payload.
pub struct CurationGate {
    store: std::sync::Arc<HumanChallengeStore>,
    gate: TrainingReadinessGate,
}

impl CurationGate {
    pub fn new(store: std::sync::Arc<HumanChallengeStore>, gate: TrainingReadinessGate) -> Self {
        Self { store, gate }
    }

    pub fn with_defaults(store: std::sync::Arc<HumanChallengeStore>) -> Self {
        Self {
            store,
            gate: TrainingReadinessGate::default(),
        }
    }

    /// Check readiness without triggering export.
    pub fn check_readiness(&self) -> ReadinessCheckResult {
        let votes = self
            .store
            .get_training_eligible_votes(10_000)
            .unwrap_or_default();

        let eligible_count = votes.len();
        let fact_verified_count = votes.iter().filter(|v| v.fact_verified).count();
        let training_eligible_count = votes.iter().filter(|v| v.training_eligible).count();

        // Collect all domain tags across votes
        let mut all_tags: std::collections::HashSet<String> = std::collections::HashSet::new();
        for vote in &votes {
            for tag in &vote.domain_tags {
                all_tags.insert(tag.clone());
            }
        }
        let domain_tags: Vec<String> = all_tags.into_iter().collect();

        let is_ready = self.gate.is_ready(&votes);

        let message = if is_ready {
            format!(
                "Ready: {} eligible votes ({} fact-verified, {} training-eligible) across domains: {}",
                eligible_count,
                fact_verified_count,
                training_eligible_count,
                domain_tags.join(", ")
            )
        } else {
            format!(
                "Not ready: {}/{} votes needed. fact_verified: {}/{:.0}% required, training_eligible: {}/{:.0}% required.",
                eligible_count,
                self.gate.min_accepted,
                fact_verified_count,
                self.gate.min_fact_verified_ratio * 100.0,
                training_eligible_count,
                self.gate.min_training_eligible_ratio * 100.0,
            )
        };

        ReadinessCheckResult {
            is_ready,
            eligible_count,
            fact_verified_count,
            training_eligible_count,
            domain_tags,
            message,
        }
    }

    /// Collect votes for a training bundle and log the gate event.
    /// Returns (votes, log_id) if ready, or an error message.
    pub fn collect_for_training(
        &self,
        bundle_id: &str,
        privacy_level: &str,
    ) -> Result<(Vec<CurationVote>, String), String> {
        let result = self.check_readiness();
        if !result.is_ready {
            return Err(result.message);
        }

        let votes = self
            .store
            .get_training_eligible_votes(10_000)
            .map_err(|e| e.to_string())?;

        let challenge_ids: Vec<String> = votes.iter().map(|v| v.challenge_id.clone()).collect();

        let log_id = self
            .store
            .log_training_gate(
                bundle_id,
                &challenge_ids,
                votes.len(),
                &result.domain_tags,
                privacy_level,
            )
            .map_err(|e| e.to_string())?;

        Ok((votes, log_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::humanchallenge::{
        store::HumanChallengeStore,
        types::{ChallengeType, CurationVerdict, CurationVote, HumanChallengeEvent},
    };
    use std::sync::Arc;

    fn make_vote(challenge_id: &str, eligible: bool) -> CurationVote {
        CurationVote::new(
            challenge_id,
            CurationVerdict::Accept,
            None,
            true,
            vec!["rust".to_string()],
            eligible,
        )
    }

    #[test]
    fn test_gate_not_ready_below_threshold() {
        let store = Arc::new(HumanChallengeStore::in_memory().unwrap());
        let gate = CurationGate::with_defaults(store.clone());

        // Add only 5 votes (below default min_accepted=20)
        for i in 0..5 {
            let challenge = HumanChallengeEvent::new(
                format!("sess_{}", i),
                ChallengeType::Decision,
                "test",
                "content",
                0.9,
            );
            store.save_event(&challenge).unwrap();
            let vote = make_vote(&challenge.id, true);
            store.save_curation_vote(&vote).unwrap();
        }

        let result = gate.check_readiness();
        assert!(!result.is_ready);
        assert_eq!(result.eligible_count, 5);
    }

    #[test]
    fn test_gate_ready_above_threshold() {
        let store = Arc::new(HumanChallengeStore::in_memory().unwrap());
        let custom_gate = TrainingReadinessGate {
            min_accepted: 3,
            min_fact_verified_ratio: 0.5,
            min_training_eligible_ratio: 0.5,
        };
        let gate = CurationGate::new(store.clone(), custom_gate);

        for i in 0..5 {
            let challenge = HumanChallengeEvent::new(
                format!("sess_{}", i),
                ChallengeType::Decision,
                "test",
                "content",
                0.9,
            );
            store.save_event(&challenge).unwrap();
            let vote = make_vote(&challenge.id, true);
            store.save_curation_vote(&vote).unwrap();
        }

        let result = gate.check_readiness();
        assert!(result.is_ready);
        assert_eq!(result.eligible_count, 5);
    }
}
