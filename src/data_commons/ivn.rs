//! # Identity Verification Network (IVN) Core Module
//!
//! Provides karma-weighted validator selection, dynamic quorum vote evaluation,
//! and validator sanctions for decentralized identity verification.

use crate::data_commons::governance::DynamicQuorum;
use crate::data_commons::reputation::EigenTrustEngine;
use crate::data_commons::types::*;
use rand::distributions::WeightedIndex;
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;
use thiserror::Error;

static EXCLUSION_STORE: RwLock<Option<HashMap<String, u64>>> = RwLock::new(None);

/// Record an exclusion window for a node/wallet address until `until_timestamp` (Unix timestamp)
pub fn record_exclusion(node_id: &WalletAddress, until_timestamp: u64) {
    let mut store = EXCLUSION_STORE.write().unwrap();
    let map = store.get_or_insert_with(HashMap::new);
    map.insert(node_id.0.clone(), until_timestamp);
}

/// Check if a node/wallet address is currently excluded at the system time
pub fn is_excluded(node_id: &WalletAddress) -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    is_excluded_at(node_id, now)
}

/// Check if a node/wallet address is excluded at a specific timestamp
pub fn is_excluded_at(node_id: &WalletAddress, current_time: u64) -> bool {
    let store = EXCLUSION_STORE.read().unwrap();
    if let Some(map) = store.as_ref() {
        if let Some(&until) = map.get(&node_id.0) {
            return current_time < until;
        }
    }
    false
}

/// Clear all registered exclusions (useful for testing)
pub fn clear_exclusions() {
    let mut store = EXCLUSION_STORE.write().unwrap();
    if let Some(map) = store.as_mut() {
        map.clear();
    }
}

/// IVN configuration parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IvnConfig {
    /// Number of validators required per request (default: 5)
    pub validators_per_request: usize,
    /// Minimum karma required to serve as a validator (default: 300)
    pub karma_min_validator: u64,
    /// Quorum ratio required for verdict approval (default: 0.8 = 80%)
    pub quorum_ratio: f64,
    /// Power exponent for karma weighting (default: 2.0, weight = karma^2)
    pub karma_pow: f64,
    /// Retry cooling period in days for applicant lie (default: 180)
    pub retry_days: u32,
    /// Karma penalty for false positives (default: -10)
    pub penalty_false_positive: i64,
    /// Karma penalty for intentional lying (default: -50)
    pub penalty_lie: i64,
    /// Exclusion period in days following a sanction (default: 90)
    pub exclusion_days: u32,
    /// Bonus karma awarded to verified applicant (default: +20)
    pub bonus_karma_verified: i64,
    /// Bonus karma awarded to validator for correct vote (default: +5)
    pub bonus_karma_validator_ok: i64,
    /// Bonus karma awarded for abstention (default: +1)
    pub bonus_karma_abstain: i64,
    /// Alias for penalty_false_positive (-10)
    pub penalty_karma_false_positive: i64,
    /// Alias for penalty_lie (-50)
    pub penalty_karma_lie: i64,
}

impl Default for IvnConfig {
    fn default() -> Self {
        Self {
            validators_per_request: 5,
            karma_min_validator: 300,
            quorum_ratio: 0.8,
            karma_pow: 2.0,
            retry_days: 180,
            penalty_false_positive: -10,
            penalty_lie: -50,
            exclusion_days: 90,
            bonus_karma_verified: 20,
            bonus_karma_validator_ok: 5,
            bonus_karma_abstain: 1,
            penalty_karma_false_positive: -10,
            penalty_karma_lie: -50,
        }
    }
}

/// Errors occurring during IVN operations
#[derive(Debug, Error, PartialEq, Eq)]
pub enum IvnError {
    #[error("Insufficient eligible validators in node pool: found {found}, required {required}")]
    InsufficientValidators { found: usize, required: usize },
    #[error("Invalid selection weights or zero total weight")]
    InvalidWeights,
    #[error("No votes provided for evaluation")]
    EmptyVotes,
}

/// Candidate node in the network pool eligible for validator selection
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidatorCandidate {
    pub node_id: WalletAddress,
    pub wallet: WalletAddress,
    pub karma: u64,
    pub seed: String,
}

/// Karma-weighted validator selector
pub struct ValidatorSelection {
    pub config: IvnConfig,
}

impl ValidatorSelection {
    /// Create a new selection instance with custom config
    pub fn new(config: IvnConfig) -> Self {
        Self { config }
    }

    /// Select `validators_per_request` validators using default config
    pub fn select_validators<R: Rng + ?Sized>(
        node_pool: &[ValidatorCandidate],
        exclude_seed: &str,
        rng: &mut R,
    ) -> Result<Vec<ValidatorCandidate>, IvnError> {
        Self::new(IvnConfig::default()).select_validators_with_config(node_pool, exclude_seed, rng)
    }

    /// Select `validators_per_request` validators using this instance's config
    pub fn select_validators_with_config<R: Rng + ?Sized>(
        &self,
        node_pool: &[ValidatorCandidate],
        exclude_seed: &str,
        rng: &mut R,
    ) -> Result<Vec<ValidatorCandidate>, IvnError> {
        // Filter out nodes below karma_min_validator or sharing exclude_seed (self-dealing check) or currently excluded
        let mut eligible: Vec<ValidatorCandidate> = node_pool
            .iter()
            .filter(|candidate| {
                candidate.karma >= self.config.karma_min_validator
                    && candidate.seed != exclude_seed
                    && !is_excluded(&candidate.node_id)
                    && !is_excluded(&candidate.wallet)
            })
            .cloned()
            .collect();

        if eligible.len() < self.config.validators_per_request {
            return Err(IvnError::InsufficientValidators {
                found: eligible.len(),
                required: self.config.validators_per_request,
            });
        }

        let mut selected = Vec::with_capacity(self.config.validators_per_request);

        // Weighted sampling without replacement using karma^karma_pow
        for _ in 0..self.config.validators_per_request {
            let weights: Vec<f64> = eligible
                .iter()
                .map(|c| (c.karma as f64).powf(self.config.karma_pow))
                .collect();

            let dist = WeightedIndex::new(&weights).map_err(|_| IvnError::InvalidWeights)?;
            let chosen_idx = dist.sample(rng);
            selected.push(eligible.remove(chosen_idx));
        }

        Ok(selected)
    }
}

/// Individual vote choice cast by a validator
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Vote {
    Check,
    Reject,
    Abstain,
}

/// Final verdict status derived from validator vote evaluation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerdictStatus {
    Passed,
    Rejected,
    QuorumNotMet,
}

/// Evaluated verdict result
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Verdict {
    pub status: VerdictStatus,
    pub check_count: usize,
    pub reject_count: usize,
    pub abstain_count: usize,
    pub total_votes: usize,
    pub approval_ratio: f64,
    pub effective_quorum: f64,
}

impl Verdict {
    pub fn is_passed(&self) -> bool {
        self.status == VerdictStatus::Passed
    }
}

/// Verdict evaluation engine using dynamic quorum thresholds
pub struct VerdictEngine;

impl VerdictEngine {
    /// Evaluate a set of votes using default quorum ratio (0.8)
    pub fn evaluate_votes(votes: &[Vote], quorum: f64) -> Verdict {
        Self::evaluate_votes_with_dynamic_quorum(votes, quorum, None)
    }

    /// Evaluate votes considering DynamicQuorum adjustment if provided
    pub fn evaluate_votes_with_dynamic_quorum(
        votes: &[Vote],
        quorum: f64,
        dynamic_quorum: Option<(&DynamicQuorum, f64)>,
    ) -> Verdict {
        let total_votes = votes.len();
        if total_votes == 0 {
            return Verdict {
                status: VerdictStatus::QuorumNotMet,
                check_count: 0,
                reject_count: 0,
                abstain_count: 0,
                total_votes: 0,
                approval_ratio: 0.0,
                effective_quorum: quorum,
            };
        }

        let effective_quorum = match dynamic_quorum {
            Some((dq, participation_rate)) => dq.effective_user_quorum(participation_rate),
            None => quorum,
        };

        let mut check_count = 0;
        let mut reject_count = 0;
        let mut abstain_count = 0;

        for vote in votes {
            match vote {
                Vote::Check => check_count += 1,
                Vote::Reject => reject_count += 1,
                Vote::Abstain => abstain_count += 1,
            }
        }

        let approval_ratio = check_count as f64 / total_votes as f64;

        let status = if approval_ratio >= effective_quorum {
            VerdictStatus::Passed
        } else if abstain_count > 0 {
            // Abstentions prevented reaching the required approval quorum threshold
            VerdictStatus::QuorumNotMet
        } else {
            VerdictStatus::Rejected
        };

        Verdict {
            status,
            check_count,
            reject_count,
            abstain_count,
            total_votes,
            approval_ratio,
            effective_quorum,
        }
    }
}

/// Summary of rewards applied for an IVN verification round
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KarmaRewardSummary {
    pub applicant_delta: i64,
    pub validator_deltas: Vec<(WalletAddress, i64)>,
}

/// Summary of sanctions applied for an IVN verification round
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KarmaSanctionSummary {
    pub fp_validator_sanctions: Vec<(WalletAddress, SanctionResult)>,
    pub liar_sanction: Option<(WalletAddress, SanctionResult)>,
}

/// Apply rewards to applicant and validators based on the evaluated verdict.
/// Updates EigenTrustEngine karma scores directly.
pub fn apply_rewards(
    engine: &mut EigenTrustEngine,
    verdict: &Verdict,
    applicant: &WalletAddress,
    validator_votes: &[(WalletAddress, Vote)],
) -> KarmaRewardSummary {
    apply_rewards_with_config(&IvnConfig::default(), engine, verdict, applicant, validator_votes)
}

/// Apply rewards using a custom IvnConfig
pub fn apply_rewards_with_config(
    config: &IvnConfig,
    engine: &mut EigenTrustEngine,
    verdict: &Verdict,
    applicant: &WalletAddress,
    validator_votes: &[(WalletAddress, Vote)],
) -> KarmaRewardSummary {
    let mut applicant_delta = 0i64;
    if verdict.is_passed() {
        applicant_delta = config.bonus_karma_verified; // +20
        engine.adjust_karma(applicant, applicant_delta);
    }

    let mut validator_deltas = Vec::with_capacity(validator_votes.len());

    for (validator, vote) in validator_votes {
        let delta = match vote {
            Vote::Check => {
                if verdict.is_passed() {
                    config.bonus_karma_validator_ok // +5
                } else {
                    0
                }
            }
            Vote::Reject => {
                if !verdict.is_passed() {
                    config.bonus_karma_validator_ok // +5
                } else {
                    0
                }
            }
            Vote::Abstain => config.bonus_karma_abstain, // +1
        };

        if delta != 0 {
            engine.adjust_karma(validator, delta);
        }
        validator_deltas.push((validator.clone(), delta));
    }

    KarmaRewardSummary {
        applicant_delta,
        validator_deltas,
    }
}

/// Apply sanctions to false positive validators (-10 karma, 90d exclusion)
/// and/or lying applicants (-50 karma, 180d retry wait window).
/// Integrates directly with EigenTrustEngine and records exclusion windows.
pub fn apply_sanctions(
    engine: &mut EigenTrustEngine,
    fp_validators: &[WalletAddress],
    liar: Option<&WalletAddress>,
    current_time: u64,
) -> KarmaSanctionSummary {
    apply_sanctions_with_config(&IvnConfig::default(), engine, fp_validators, liar, current_time)
}

/// Apply sanctions with a custom IvnConfig
pub fn apply_sanctions_with_config(
    config: &IvnConfig,
    engine: &mut EigenTrustEngine,
    fp_validators: &[WalletAddress],
    liar: Option<&WalletAddress>,
    current_time: u64,
) -> KarmaSanctionSummary {
    let mut fp_validator_sanctions = Vec::with_capacity(fp_validators.len());

    for val in fp_validators {
        let sanction = sanction_validator_with_config(config, 1, false);
        engine.adjust_karma(val, sanction.karma_penalty); // -10 karma
        let until = current_time + (sanction.exclusion_days as u64 * 86_400);
        record_exclusion(val, until);
        fp_validator_sanctions.push((val.clone(), sanction));
    }

    let mut liar_sanction = None;
    if let Some(applicant) = liar {
        let sanction = SanctionResult {
            karma_penalty: config.penalty_karma_lie, // -50
            exclusion_days: config.retry_days,        // 180d
        };
        engine.adjust_karma(applicant, sanction.karma_penalty); // -50 karma
        let until = current_time + (sanction.exclusion_days as u64 * 86_400);
        record_exclusion(applicant, until);
        liar_sanction = Some((applicant.clone(), sanction));
    }

    KarmaSanctionSummary {
        fp_validator_sanctions,
        liar_sanction,
    }
}

/// Result of a sanction applied to a validator
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SanctionResult {
    pub karma_penalty: i64,
    pub exclusion_days: u32,
}

/// Sanction a validator for false positive / misbehavior
pub fn sanction_validator(fp_count: u32) -> SanctionResult {
    let config = IvnConfig::default();
    sanction_validator_with_config(&config, fp_count, false)
}

/// Sanction a validator with custom configuration and lie check
pub fn sanction_validator_with_config(
    config: &IvnConfig,
    fp_count: u32,
    is_lie: bool,
) -> SanctionResult {
    let fp_penalty = config.penalty_false_positive * (fp_count as i64);
    let lie_penalty = if is_lie { config.penalty_lie } else { 0 };
    let total_penalty = fp_penalty + lie_penalty;

    SanctionResult {
        karma_penalty: total_penalty,
        exclusion_days: config.exclusion_days,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    #[test]
    fn test_ivn_config_defaults() {
        let config = IvnConfig::default();
        assert_eq!(config.validators_per_request, 5);
        assert_eq!(config.karma_min_validator, 300);
        assert_eq!(config.quorum_ratio, 0.8);
        assert_eq!(config.karma_pow, 2.0);
        assert_eq!(config.retry_days, 180);
        assert_eq!(config.penalty_false_positive, -10);
        assert_eq!(config.penalty_lie, -50);
        assert_eq!(config.exclusion_days, 90);
        assert_eq!(config.bonus_karma_verified, 20);
        assert_eq!(config.bonus_karma_validator_ok, 5);
        assert_eq!(config.bonus_karma_abstain, 1);
    }

    #[test]
    fn test_validator_selection_filters_karma_and_seed() {
        let mut rng = StdRng::seed_from_u64(42);

        let pool = vec![
            ValidatorCandidate {
                node_id: WalletAddress("xv1_node1".into()),
                wallet: WalletAddress("xv1_w1".into()),
                karma: 500,
                seed: "seed_a".into(),
            },
            ValidatorCandidate {
                node_id: WalletAddress("xv1_node2".into()),
                wallet: WalletAddress("xv1_w2".into()),
                karma: 200, // < 300 karma_min_validator -> excluded
                seed: "seed_b".into(),
            },
            ValidatorCandidate {
                node_id: WalletAddress("xv1_node3".into()),
                wallet: WalletAddress("xv1_w3".into()),
                karma: 600,
                seed: "shared_applicant_seed".into(), // excluded by seed
            },
            ValidatorCandidate {
                node_id: WalletAddress("xv1_node4".into()),
                wallet: WalletAddress("xv1_w4".into()),
                karma: 400,
                seed: "seed_c".into(),
            },
            ValidatorCandidate {
                node_id: WalletAddress("xv1_node5".into()),
                wallet: WalletAddress("xv1_w5".into()),
                karma: 700,
                seed: "seed_d".into(),
            },
            ValidatorCandidate {
                node_id: WalletAddress("xv1_node6".into()),
                wallet: WalletAddress("xv1_w6".into()),
                karma: 800,
                seed: "seed_e".into(),
            },
            ValidatorCandidate {
                node_id: WalletAddress("xv1_node7".into()),
                wallet: WalletAddress("xv1_w7".into()),
                karma: 900,
                seed: "seed_f".into(),
            },
        ];

        let selected =
            ValidatorSelection::select_validators(&pool, "shared_applicant_seed", &mut rng)
                .unwrap();

        assert_eq!(selected.len(), 5);
        for v in &selected {
            assert!(v.karma >= 300);
            assert_ne!(v.seed, "shared_applicant_seed");
        }
    }

    #[test]
    fn test_verdict_engine_evaluation() {
        let votes_pass = vec![
            Vote::Check,
            Vote::Check,
            Vote::Check,
            Vote::Check,
            Vote::Reject,
        ];
        let verdict_pass = VerdictEngine::evaluate_votes(&votes_pass, 0.8);
        assert_eq!(verdict_pass.status, VerdictStatus::Passed);
        assert!(verdict_pass.is_passed());

        let votes_fail = vec![
            Vote::Check,
            Vote::Check,
            Vote::Check,
            Vote::Reject,
            Vote::Reject,
        ];
        let verdict_fail = VerdictEngine::evaluate_votes(&votes_fail, 0.8);
        assert_eq!(verdict_fail.status, VerdictStatus::Rejected);
        assert!(!verdict_fail.is_passed());
    }

    #[test]
    fn test_sanction_validator_calculation() {
        let sanction = sanction_validator(2);
        assert_eq!(sanction.karma_penalty, -20);
        assert_eq!(sanction.exclusion_days, 90);

        let config = IvnConfig::default();
        let sanction_lie = sanction_validator_with_config(&config, 1, true);
        assert_eq!(sanction_lie.karma_penalty, -60);
    }
}
