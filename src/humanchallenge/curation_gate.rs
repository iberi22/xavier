//! CurationGate — the single training-readiness gate for curated data.
//!
//! Unifies HumanChallenge curation votes and the JSON curation queue
//! (`data/curation/queue.json`). Readiness is evaluated per domain and
//! produces `ReadyDomain` entries consumed by the training pipeline.

use crate::curation::{CurationItem, CurationQueue};
use crate::humanchallenge::store::HumanChallengeStore;
use crate::humanchallenge::types::{CurationVerdict, CurationVote, TrainingReadinessGate};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

const GENERAL_DOMAIN: &str = "general";
const DEFAULT_QUEUE_PATH: &str = "data/curation/queue.json";
/// Clearance assumed for votes, which carry no clearance of their own.
const VOTE_DEFAULT_CLEARANCE: &str = "internal";

/// Configurable thresholds of the gate.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadinessThresholds {
    /// Minimum accepted examples (votes + approved queue items) per domain.
    pub min_examples_per_domain: usize,
    /// Minimum fraction of examples that are fact-verified.
    pub min_fact_verified_ratio: f32,
    /// Minimum fraction of examples that are training-eligible.
    pub min_training_eligible_ratio: f32,
}

impl Default for ReadinessThresholds {
    fn default() -> Self {
        let g = TrainingReadinessGate::default();
        Self {
            min_examples_per_domain: g.min_accepted,
            min_fact_verified_ratio: g.min_fact_verified_ratio,
            min_training_eligible_ratio: g.min_training_eligible_ratio,
        }
    }
}

impl ReadinessThresholds {
    fn to_gate(&self) -> TrainingReadinessGate {
        TrainingReadinessGate {
            min_accepted: self.min_examples_per_domain,
            min_fact_verified_ratio: self.min_fact_verified_ratio,
            min_training_eligible_ratio: self.min_training_eligible_ratio,
        }
    }
}

/// A domain that passed the gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadyDomain {
    pub domain: String,
    pub n_examples: usize,
    /// Highest clearance among the domain's examples.
    pub clearance_max: String,
}

/// Result of a readiness check.
#[derive(Debug, Clone)]
pub struct ReadinessCheckResult {
    /// True when at least one domain is ready.
    pub is_ready: bool,
    /// Votes plus approved queue items.
    pub eligible_count: usize,
    pub fact_verified_count: usize,
    pub training_eligible_count: usize,
    pub queue_approved_count: usize,
    pub domain_tags: Vec<String>,
    pub ready_domains: Vec<ReadyDomain>,
    pub message: String,
}

/// Rank of a clearance label; unknown labels rank highest (conservative).
fn clearance_rank(label: &str) -> u8 {
    match label.trim().to_ascii_lowercase().as_str() {
        "public" | "unclassified" => 0,
        "internal" => 1,
        "restricted" => 2,
        "confidential" => 3,
        "secret" => 4,
        _ => 5,
    }
}

fn vote_domains(vote: &CurationVote) -> Vec<String> {
    let tags: BTreeSet<String> = vote
        .domain_tags
        .iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    if tags.is_empty() {
        vec![GENERAL_DOMAIN.to_string()]
    } else {
        tags.into_iter().collect()
    }
}

/// Pure per-domain evaluation over votes and approved queue items.
pub fn evaluate_domains(
    thresholds: &ReadinessThresholds,
    votes: &[CurationVote],
    approved_items: &[CurationItem],
) -> Vec<ReadyDomain> {
    // domain -> (examples, max clearance label)
    let mut by_domain: BTreeMap<String, (Vec<CurationVote>, String)> = BTreeMap::new();
    let mut bump = |domain: String, vote: CurationVote, clearance: &str| {
        let entry = by_domain
            .entry(domain)
            .or_insert_with(|| (Vec::new(), clearance.to_string()));
        if clearance_rank(clearance) > clearance_rank(&entry.1) {
            entry.1 = clearance.to_string();
        }
        entry.0.push(vote);
    };

    for vote in votes {
        for domain in vote_domains(vote) {
            bump(domain, vote.clone(), VOTE_DEFAULT_CLEARANCE);
        }
    }
    for item in approved_items {
        // A human approval counts as an accepted, verified, eligible example.
        let domain = item.domain();
        let as_vote = CurationVote::new(
            item.id.clone(),
            CurationVerdict::Accept,
            None,
            true,
            vec![domain.clone()],
            true,
        );
        bump(domain, as_vote, &item.proposed_clearance);
    }

    let gate = thresholds.to_gate();
    by_domain
        .into_iter()
        .filter(|(_, (examples, _))| gate.is_ready(examples))
        .map(|(domain, (examples, clearance_max))| ReadyDomain {
            domain,
            n_examples: examples.len(),
            clearance_max,
        })
        .collect()
}

/// The single gate: reads votes from the store and approved items from the queue.
pub struct CurationGate {
    store: std::sync::Arc<HumanChallengeStore>,
    thresholds: ReadinessThresholds,
    queue_path: Option<PathBuf>,
}

impl std::fmt::Debug for CurationGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CurationGate")
            .field("thresholds", &self.thresholds)
            .field("queue_path", &self.queue_path)
            .finish_non_exhaustive()
    }
}

impl CurationGate {
    pub fn new(
        store: std::sync::Arc<HumanChallengeStore>,
        thresholds: ReadinessThresholds,
    ) -> Self {
        Self {
            store,
            thresholds,
            queue_path: Some(PathBuf::from(DEFAULT_QUEUE_PATH)),
        }
    }

    pub fn with_defaults(store: std::sync::Arc<HumanChallengeStore>) -> Self {
        Self::new(store, ReadinessThresholds::default())
    }

    /// Read the curation queue from `path` instead of the default location.
    pub fn with_queue_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.queue_path = Some(path.into());
        self
    }

    /// Ignore the JSON curation queue (votes only).
    pub fn without_queue(mut self) -> Self {
        self.queue_path = None;
        self
    }

    fn approved_items(&self) -> Vec<CurationItem> {
        match &self.queue_path {
            Some(p) => CurationQueue::load_from_path(p)
                .map(|q| q.curated_items())
                .unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// Ready domains right now.
    pub fn ready_domains(&self) -> Vec<ReadyDomain> {
        let votes = self
            .store
            .get_training_eligible_votes(10_000)
            .unwrap_or_default();
        evaluate_domains(&self.thresholds, &votes, &self.approved_items())
    }

    /// Check readiness without triggering export.
    pub fn check_readiness(&self) -> ReadinessCheckResult {
        let votes = self
            .store
            .get_training_eligible_votes(10_000)
            .unwrap_or_default();
        let items = self.approved_items();
        let ready_domains = evaluate_domains(&self.thresholds, &votes, &items);

        let queue_approved_count = items.len();
        let eligible_count = votes.len() + queue_approved_count;
        let fact_verified_count =
            votes.iter().filter(|v| v.fact_verified).count() + queue_approved_count;
        let training_eligible_count =
            votes.iter().filter(|v| v.training_eligible).count() + queue_approved_count;

        let mut all_tags: BTreeSet<String> = BTreeSet::new();
        for vote in &votes {
            all_tags.extend(vote_domains(vote));
        }
        all_tags.extend(items.iter().map(|i| i.domain()));
        let domain_tags: Vec<String> = all_tags.into_iter().collect();

        let is_ready = !ready_domains.is_empty();
        let message = if is_ready {
            format!(
                "Ready: {} eligible examples ({} from curation queue, {} fact-verified, {} training-eligible); ready domains: {}",
                eligible_count,
                queue_approved_count,
                fact_verified_count,
                training_eligible_count,
                ready_domains
                    .iter()
                    .map(|d| format!("{} ({})", d.domain, d.n_examples))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            format!(
                "Not ready: no domain has {} examples yet ({} total across {} domains). fact_verified: {:.0}% required, training_eligible: {:.0}% required.",
                self.thresholds.min_examples_per_domain,
                eligible_count,
                domain_tags.len(),
                self.thresholds.min_fact_verified_ratio * 100.0,
                self.thresholds.min_training_eligible_ratio * 100.0,
            )
        };

        ReadinessCheckResult {
            is_ready,
            eligible_count,
            fact_verified_count,
            training_eligible_count,
            queue_approved_count,
            domain_tags,
            ready_domains,
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

        // Only votes belonging to a ready domain feed the bundle.
        let ready: BTreeSet<&str> = result
            .ready_domains
            .iter()
            .map(|d| d.domain.as_str())
            .collect();
        let votes: Vec<CurationVote> = votes
            .into_iter()
            .filter(|v| vote_domains(v).iter().any(|d| ready.contains(d.as_str())))
            .collect();
        let domains: Vec<String> = result
            .ready_domains
            .iter()
            .map(|d| d.domain.clone())
            .collect();

        let challenge_ids: Vec<String> = votes.iter().map(|v| v.challenge_id.clone()).collect();

        let log_id = self
            .store
            .log_training_gate(
                bundle_id,
                &challenge_ids,
                votes.len(),
                &domains,
                privacy_level,
            )
            .map_err(|e| e.to_string())?;

        Ok((votes, log_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::humanchallenge::types::{ChallengeType, HumanChallengeEvent};
    use std::sync::Arc;

    fn thresholds(min: usize) -> ReadinessThresholds {
        ReadinessThresholds {
            min_examples_per_domain: min,
            min_fact_verified_ratio: 0.5,
            min_training_eligible_ratio: 0.5,
        }
    }

    fn add_votes(store: &HumanChallengeStore, n: usize, tags: &[&str]) {
        for i in 0..n {
            let challenge = HumanChallengeEvent::new(
                format!("sess_{}_{}", tags.join("-"), i),
                ChallengeType::Decision,
                "test",
                "content",
                0.9,
            );
            store.save_event(&challenge).unwrap();
            let vote = CurationVote::new(
                &challenge.id,
                CurationVerdict::Accept,
                None,
                true,
                tags.iter().map(|t| t.to_string()).collect(),
                true,
            );
            store.save_curation_vote(&vote).unwrap();
        }
    }

    fn queue_file(
        dir: &tempfile::TempDir,
        n: usize,
        classification: &str,
        clearance: &str,
    ) -> PathBuf {
        let path = dir.path().join("queue.json");
        let mut q = CurationQueue::new_with_path(path.clone());
        for i in 0..n {
            let item = q.submit_for_curation(format!("doc-{i}"), clearance.to_string(), None);
            q.approve(&item.id, "alice".into(), Some(classification.into()), None)
                .unwrap();
        }
        q.submit_for_curation("pending".into(), "public".into(), None);
        q.save().unwrap();
        path
    }

    #[test]
    fn test_gate_not_ready_below_threshold() {
        let store = Arc::new(HumanChallengeStore::in_memory().unwrap());
        let gate = CurationGate::with_defaults(store.clone()).without_queue();
        add_votes(&store, 5, &["rust"]);

        let result = gate.check_readiness();
        assert!(!result.is_ready);
        assert_eq!(result.eligible_count, 5);
        assert!(result.ready_domains.is_empty());
    }

    #[test]
    fn test_gate_ready_above_threshold() {
        let store = Arc::new(HumanChallengeStore::in_memory().unwrap());
        let gate = CurationGate::new(store.clone(), thresholds(3)).without_queue();
        add_votes(&store, 5, &["rust"]);

        let result = gate.check_readiness();
        assert!(result.is_ready);
        assert_eq!(result.eligible_count, 5);
        assert_eq!(result.ready_domains.len(), 1);
        assert_eq!(result.ready_domains[0].domain, "rust");
        assert_eq!(result.ready_domains[0].n_examples, 5);
    }

    #[test]
    fn test_per_domain_readiness() {
        let store = Arc::new(HumanChallengeStore::in_memory().unwrap());
        let gate = CurationGate::new(store.clone(), thresholds(4)).without_queue();
        add_votes(&store, 5, &["rust"]);
        add_votes(&store, 2, &["python"]);

        let ready = gate.ready_domains();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].domain, "rust");
        assert!(gate
            .check_readiness()
            .domain_tags
            .contains(&"python".to_string()));
    }

    #[test]
    fn test_queue_items_counted() {
        let store = Arc::new(HumanChallengeStore::in_memory().unwrap());
        let dir = tempfile::tempdir().unwrap();
        let path = queue_file(&dir, 4, "Rust", "confidential");
        // 2 votes + 4 approved queue items (pending one ignored) = 6 in "rust"
        add_votes(&store, 2, &["rust"]);
        let gate = CurationGate::new(store.clone(), thresholds(6)).with_queue_path(path.clone());

        let result = gate.check_readiness();
        assert_eq!(result.queue_approved_count, 4);
        assert_eq!(result.eligible_count, 6);
        assert!(result.is_ready);
        assert_eq!(
            result.ready_domains,
            vec![ReadyDomain {
                domain: "rust".into(),
                n_examples: 6,
                clearance_max: "confidential".into(),
            }]
        );

        // Without the queue the same votes are not enough.
        let no_queue = CurationGate::new(store, thresholds(6)).without_queue();
        assert!(!no_queue.check_readiness().is_ready);
    }

    #[test]
    fn test_thresholds_respected() {
        let store = Arc::new(HumanChallengeStore::in_memory().unwrap());
        add_votes(&store, 5, &["rust"]);
        let exact = CurationGate::new(store.clone(), thresholds(5)).without_queue();
        assert!(exact.check_readiness().is_ready);
        let above = CurationGate::new(store.clone(), thresholds(6)).without_queue();
        assert!(!above.check_readiness().is_ready);

        // Fact-verified ratio: 2 verified of 4 queue items-as-votes is not applicable;
        // exercise it on the pure evaluator.
        let mk = |fv: bool| {
            CurationVote::new(
                "c",
                CurationVerdict::Accept,
                None,
                fv,
                vec!["x".into()],
                true,
            )
        };
        let votes = vec![mk(true), mk(false), mk(false), mk(false)];
        let strict = ReadinessThresholds {
            min_examples_per_domain: 2,
            min_fact_verified_ratio: 0.5,
            min_training_eligible_ratio: 0.5,
        };
        assert!(evaluate_domains(&strict, &votes, &[]).is_empty());
        let lax = ReadinessThresholds {
            min_fact_verified_ratio: 0.25,
            ..strict
        };
        assert_eq!(evaluate_domains(&lax, &votes, &[]).len(), 1);
    }

    #[test]
    fn test_clearance_max_ordering() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.json");
        let mut q = CurationQueue::new_with_path(path);
        for clr in ["public", "SECRET", "internal"] {
            let it = q.submit_for_curation("d".into(), clr.into(), None);
            q.approve(&it.id, "a".into(), Some("ops".into()), None)
                .unwrap();
        }
        let ready = evaluate_domains(&thresholds(3), &[], &q.curated_items());
        assert_eq!(ready[0].clearance_max, "SECRET");
    }

    #[test]
    fn test_collect_for_training_only_ready_domains() {
        let store = Arc::new(HumanChallengeStore::in_memory().unwrap());
        add_votes(&store, 4, &["rust"]);
        add_votes(&store, 1, &["python"]);
        let gate = CurationGate::new(store, thresholds(3)).without_queue();
        let (votes, _log) = gate.collect_for_training("b1", "P2").unwrap();
        assert_eq!(votes.len(), 4);
    }
}
