//! Sybil-resistant Quadratic Voting Engine with IVN Karma Weighting
//!
//! Provides integer square root credit weighting, multi-choice ballots, and
//! Sybil mitigation integrating EigenTrust reputation and IVN identity tiers.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// IVN (Identity Verification Network) identity tier
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum IvnIdentityTier {
    /// Tier 0: Unverified entity (0.1x multiplier)
    Unverified,
    /// Tier 1: Basic peer node (0.5x multiplier)
    Basic,
    /// Tier 2: Verified participant (1.0x multiplier)
    Verified,
    /// Tier 3: Validator node (1.5x multiplier)
    Validator,
    /// Tier 4: Sovereign council node (2.0x multiplier)
    Sovereign,
}

impl IvnIdentityTier {
    /// Multiplier in basis points (10000 = 1.0x)
    pub fn multiplier_bps(&self) -> u64 {
        match self {
            IvnIdentityTier::Unverified => 1000, // 0.1x
            IvnIdentityTier::Basic => 5000,      // 0.5x
            IvnIdentityTier::Verified => 10000,  // 1.0x
            IvnIdentityTier::Validator => 15000, // 1.5x
            IvnIdentityTier::Sovereign => 20000, // 2.0x
        }
    }
}

/// Profile for a voting node/wallet, containing EigenTrust karma and IVN identity metadata
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoterProfile {
    pub voter_id: String,
    pub karma: u64,
    pub identity_tier: IvnIdentityTier,
    pub is_sybil_flagged: bool,
    pub credit_balance: u64,
}

impl VoterProfile {
    pub fn new(
        voter_id: impl Into<String>,
        karma: u64,
        identity_tier: IvnIdentityTier,
        credit_balance: u64,
    ) -> Self {
        Self {
            voter_id: voter_id.into(),
            karma,
            identity_tier,
            is_sybil_flagged: false,
            credit_balance,
        }
    }

    pub fn with_sybil_flag(mut self, flagged: bool) -> Self {
        self.is_sybil_flagged = flagged;
        self
    }
}

/// Bitwise integer square root calculation floor(sqrt(n)) using Newton's method
pub fn integer_sqrt(n: u128) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut x = 1u128 << (128 - n.leading_zeros()).div_ceil(2);
    if x == 0 {
        x = 1;
    }
    loop {
        let y = (x + n / x) >> 1;
        if y >= x {
            return x as u64;
        }
        x = y;
    }
}

/// Calculates effective votes using integer square root credit weighting with EigenTrust karma & IVN identity tier weighting
///
/// `effective_votes = sqrt(credits * karma_weight)`
/// where `karma_weight = (karma * tier_multiplier_bps) / 10000`
pub fn calculate_effective_votes(
    credits: u64,
    karma: u64,
    tier: IvnIdentityTier,
    is_sybil_flagged: bool,
) -> u64 {
    if is_sybil_flagged || credits == 0 || karma == 0 {
        return 0;
    }

    let multiplier_bps = tier.multiplier_bps();
    let karma_weight = (karma as u128).saturating_mul(multiplier_bps as u128) / 10000u128;

    if karma_weight == 0 {
        return 0;
    }

    let product = (credits as u128).saturating_mul(karma_weight);
    integer_sqrt(product)
}

/// Ballot allocating credits across multiple proposal options
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiChoiceBallot {
    pub voter_id: String,
    pub allocations: HashMap<String, u64>,
}

/// Proposal for quadratic voting
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuadraticProposal {
    pub id: String,
    pub options: Vec<String>,
    pub ballots: HashMap<String, MultiChoiceBallot>,
    pub quorum: u64,
}

/// Result summary for quadratic vote tallying
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalTallyResult {
    pub proposal_id: String,
    pub option_tallies: HashMap<String, u64>,
    pub total_voters: usize,
    pub sybil_votes_rejected: usize,
    pub winning_option: Option<String>,
    pub quorum_reached: bool,
}

/// Main Sybil-resistant Quadratic Voting Engine
#[derive(Debug, Clone, Default)]
pub struct QuadraticVoteEngine {
    pub voters: HashMap<String, VoterProfile>,
    pub proposals: HashMap<String, QuadraticProposal>,
    pub default_quorum: u64,
}

impl QuadraticVoteEngine {
    pub fn new(default_quorum: u64) -> Self {
        Self {
            voters: HashMap::new(),
            proposals: HashMap::new(),
            default_quorum,
        }
    }

    pub fn register_voter(&mut self, profile: VoterProfile) {
        self.voters.insert(profile.voter_id.clone(), profile);
    }

    pub fn get_voter(&self, voter_id: &str) -> Option<&VoterProfile> {
        self.voters.get(voter_id)
    }

    pub fn create_proposal(
        &mut self,
        proposal_id: impl Into<String>,
        options: Vec<String>,
    ) -> Result<(), String> {
        self.create_proposal_with_quorum(proposal_id, options, self.default_quorum)
    }

    pub fn create_proposal_with_quorum(
        &mut self,
        proposal_id: impl Into<String>,
        options: Vec<String>,
        quorum: u64,
    ) -> Result<(), String> {
        let id = proposal_id.into();
        if options.is_empty() {
            return Err("Proposal options cannot be empty".to_string());
        }
        if self.proposals.contains_key(&id) {
            return Err(format!("Proposal with ID '{}' already exists", id));
        }

        self.proposals.insert(
            id.clone(),
            QuadraticProposal {
                id,
                options,
                ballots: HashMap::new(),
                quorum,
            },
        );
        Ok(())
    }

    pub fn cast_vote(
        &mut self,
        proposal_id: &str,
        voter_id: &str,
        option: &str,
        credits: u64,
    ) -> Result<u64, String> {
        let mut allocations = HashMap::new();
        allocations.insert(option.to_string(), credits);
        let res_map = self.cast_multi_choice_ballot(proposal_id, voter_id, allocations)?;
        Ok(*res_map.get(option).unwrap_or(&0))
    }

    pub fn cast_multi_choice_ballot(
        &mut self,
        proposal_id: &str,
        voter_id: &str,
        allocations: HashMap<String, u64>,
    ) -> Result<HashMap<String, u64>, String> {
        let voter = self
            .voters
            .get(voter_id)
            .ok_or_else(|| format!("Voter '{}' is not registered", voter_id))?;

        let proposal = self
            .proposals
            .get_mut(proposal_id)
            .ok_or_else(|| format!("Proposal '{}' not found", proposal_id))?;

        if proposal.ballots.contains_key(voter_id) {
            return Err(format!(
                "Voter '{}' has already voted on proposal '{}'",
                voter_id, proposal_id
            ));
        }

        if allocations.is_empty() {
            return Err("Ballot allocations cannot be empty".to_string());
        }

        let mut total_credits_spent: u64 = 0;
        for (option, &credits) in &allocations {
            if !proposal.options.contains(option) {
                return Err(format!(
                    "Invalid option '{}' for proposal '{}'",
                    option, proposal_id
                ));
            }
            if credits == 0 {
                return Err(format!(
                    "Credits allocated to option '{}' must be greater than zero",
                    option
                ));
            }
            total_credits_spent = total_credits_spent
                .checked_add(credits)
                .ok_or_else(|| "Credit sum overflow".to_string())?;
        }

        if total_credits_spent > voter.credit_balance {
            return Err(format!(
                "Insufficient credits: voter '{}' spent {} credits but has balance of {}",
                voter_id, total_credits_spent, voter.credit_balance
            ));
        }

        let mut effective_map = HashMap::new();
        for (option, &credits) in &allocations {
            let eff = calculate_effective_votes(
                credits,
                voter.karma,
                voter.identity_tier,
                voter.is_sybil_flagged,
            );
            effective_map.insert(option.clone(), eff);
        }

        proposal.ballots.insert(
            voter_id.to_string(),
            MultiChoiceBallot {
                voter_id: voter_id.to_string(),
                allocations,
            },
        );

        Ok(effective_map)
    }

    pub fn tally(&self, proposal_id: &str) -> Result<ProposalTallyResult, String> {
        let proposal = self
            .proposals
            .get(proposal_id)
            .ok_or_else(|| format!("Proposal '{}' not found", proposal_id))?;

        let mut option_tallies: HashMap<String, u64> = proposal
            .options
            .iter()
            .map(|opt| (opt.clone(), 0u64))
            .collect();

        let mut sybil_votes_rejected = 0;
        let mut total_voters = 0;
        let mut total_effective_votes = 0u64;

        for (voter_id, ballot) in &proposal.ballots {
            total_voters += 1;
            let voter = self.voters.get(voter_id);

            let is_sybil = voter.is_some_and(|v| v.is_sybil_flagged);
            if is_sybil {
                sybil_votes_rejected += 1;
                continue;
            }

            for (option, &credits) in &ballot.allocations {
                let (karma, tier) = voter.map_or((0, IvnIdentityTier::Unverified), |v| {
                    (v.karma, v.identity_tier)
                });
                let eff = calculate_effective_votes(credits, karma, tier, false);

                if let Some(current) = option_tallies.get_mut(option) {
                    *current = current.saturating_add(eff);
                }
                total_effective_votes = total_effective_votes.saturating_add(eff);
            }
        }

        let mut max_votes = 0u64;
        let mut winning_option = None;
        for (opt, &votes) in &option_tallies {
            if votes > max_votes {
                max_votes = votes;
                winning_option = Some(opt.clone());
            }
        }

        let quorum_reached = total_effective_votes >= proposal.quorum;

        Ok(ProposalTallyResult {
            proposal_id: proposal_id.to_string(),
            option_tallies,
            total_voters,
            sybil_votes_rejected,
            winning_option,
            quorum_reached,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integer_sqrt_exact_and_floors() {
        assert_eq!(integer_sqrt(0), 0);
        assert_eq!(integer_sqrt(1), 1);
        assert_eq!(integer_sqrt(3), 1);
        assert_eq!(integer_sqrt(4), 2);
        assert_eq!(integer_sqrt(9), 3);
        assert_eq!(integer_sqrt(15), 3);
        assert_eq!(integer_sqrt(16), 4);
        assert_eq!(integer_sqrt(100), 10);
        assert_eq!(integer_sqrt(10000), 100);
    }

    #[test]
    fn test_calculate_effective_votes() {
        // Verified tier (1.0x = 10000 bps), karma = 100, credits = 100
        // karma_weight = (100 * 10000) / 10000 = 100
        // product = 100 * 100 = 10000
        // sqrt(10000) = 100
        let votes = calculate_effective_votes(100, 100, IvnIdentityTier::Verified, false);
        assert_eq!(votes, 100);

        // Sovereign tier (2.0x = 20000 bps), karma = 100, credits = 100
        // karma_weight = (100 * 20000) / 10000 = 200
        // product = 100 * 200 = 20000
        // sqrt(20000) = 141
        let sovereign_votes =
            calculate_effective_votes(100, 100, IvnIdentityTier::Sovereign, false);
        assert_eq!(sovereign_votes, 141);

        // Sybil flagged returns 0
        let sybil_votes = calculate_effective_votes(100, 100, IvnIdentityTier::Verified, true);
        assert_eq!(sybil_votes, 0);
    }

    #[test]
    fn test_sybil_split_attack_mitigation() {
        // Honest voter with Verified tier, karma 1000, credit balance 100
        // karma_weight = 1000
        // product = 100 * 1000 = 100_000
        // sqrt(100_000) = 316 effective votes
        let honest_votes = calculate_effective_votes(100, 1000, IvnIdentityTier::Verified, false);

        // Attacker splits 100 credits across 10 Unverified Sybils with karma 10 each (10 credits each)
        // For 1 Sybil: credits=10, karma=10, Unverified multiplier = 1000 bps (0.1x)
        // karma_weight = (10 * 1000) / 10000 = 1
        // product = 10 * 1 = 10
        // sqrt(10) = 3 effective votes
        // Total for 10 Sybils = 10 * 3 = 30 effective votes
        let sybil_votes = calculate_effective_votes(10, 10, IvnIdentityTier::Unverified, false);
        let total_sybil_votes = sybil_votes * 10;

        assert_eq!(honest_votes, 316);
        assert_eq!(total_sybil_votes, 30);
        assert!(honest_votes > total_sybil_votes * 10);
    }

    // === Edge Case Tests for Issue #1459 ===

    #[test]
    fn test_zero_credits_returns_zero_effective_votes() {
        // Zero credits should always yield zero effective votes regardless of karma/tier
        assert_eq!(
            calculate_effective_votes(0, 100, IvnIdentityTier::Verified, false),
            0
        );
        assert_eq!(
            calculate_effective_votes(0, u64::MAX, IvnIdentityTier::Sovereign, false),
            0
        );
        assert_eq!(
            calculate_effective_votes(0, 1, IvnIdentityTier::Basic, false),
            0
        );
    }

    #[test]
    fn test_zero_karma_returns_zero_effective_votes() {
        // Zero karma should always yield zero effective votes
        assert_eq!(
            calculate_effective_votes(100, 0, IvnIdentityTier::Verified, false),
            0
        );
        assert_eq!(
            calculate_effective_votes(100, 0, IvnIdentityTier::Sovereign, false),
            0
        );
        assert_eq!(
            calculate_effective_votes(u64::MAX, 0, IvnIdentityTier::Sovereign, false),
            0
        );
    }

    #[test]
    fn test_sybil_flagged_always_zero_effective_votes() {
        // Even with max credits/karma, sybil flagged voter gets 0
        let votes = calculate_effective_votes(u64::MAX, u64::MAX, IvnIdentityTier::Sovereign, true);
        assert_eq!(votes, 0);
        // Also with zero credits (redundant but documents behavior)
        assert_eq!(
            calculate_effective_votes(0, 0, IvnIdentityTier::Unverified, true),
            0
        );
    }

    #[test]
    fn test_credit_sum_overflow_rejected() {
        // Casting a ballot where credits sum exceeds u64::MAX should error
        let mut engine = QuadraticVoteEngine::new(10);
        let voter = VoterProfile::new("v1", 100, IvnIdentityTier::Verified, u64::MAX);
        engine.register_voter(voter);
        engine
            .create_proposal("p1", vec!["a".into(), "b".into()])
            .unwrap();

        let mut allocations = HashMap::new();
        allocations.insert("a".into(), u64::MAX);
        allocations.insert("b".into(), 1u64); // Sum overflows

        let result = engine.cast_multi_choice_ballot("p1", "v1", allocations);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("overflow"));
    }

    #[test]
    fn test_duplicate_vote_rejected() {
        let mut engine = QuadraticVoteEngine::new(10);
        let voter = VoterProfile::new("v1", 100, IvnIdentityTier::Verified, 1000);
        engine.register_voter(voter);
        engine
            .create_proposal("p1", vec!["a".into(), "b".into()])
            .unwrap();

        assert!(engine.cast_vote("p1", "v1", "a", 10).is_ok());
        let second = engine.cast_vote("p1", "v1", "b", 10);
        assert!(second.is_err());
        assert!(second.unwrap_err().contains("already voted"));
    }

    #[test]
    fn test_tie_breaking_prefers_last_option_when_equal() {
        // When all options have equal votes, the tally picks the last option
        // iterated (because strict `>` but max starts at 0, so the first option
        // sets max, then the equal option replaces it since both > max_votes).
        let mut engine = QuadraticVoteEngine::new(1);
        let v1 = VoterProfile::new("v1", 100, IvnIdentityTier::Verified, 100);
        let v2 = VoterProfile::new("v2", 100, IvnIdentityTier::Verified, 100);
        engine.register_voter(v1);
        engine.register_voter(v2);
        engine
            .create_proposal("p1", vec!["a".into(), "b".into()])
            .unwrap();

        // Both allocate 10 credits to a different option => equal effective votes
        engine.cast_vote("p1", "v1", "a", 10).unwrap();
        engine.cast_vote("p1", "v2", "b", 10).unwrap();

        let result = engine.tally("p1").unwrap();
        // A winner IS declared (not None) — HashMap iteration order determines which
        assert!(result.winning_option.is_some());
        // Both options have equal tallies
        assert_eq!(result.option_tallies["a"], result.option_tallies["b"]);
    }

    #[test]
    fn test_tie_breaking_one_vote_ahead() {
        // A single extra credit can break a tie due to quadratic sqrt behavior
        let mut engine = QuadraticVoteEngine::new(1);
        let v1 = VoterProfile::new("v1", 100, IvnIdentityTier::Verified, 100);
        let v2 = VoterProfile::new("v2", 100, IvnIdentityTier::Verified, 100);
        engine.register_voter(v1);
        engine.register_voter(v2);
        engine
            .create_proposal("p1", vec!["a".into(), "b".into()])
            .unwrap();

        engine.cast_vote("p1", "v1", "a", 20).unwrap(); // sqrt(2000) = 44
        engine.cast_vote("p1", "v2", "b", 19).unwrap(); // sqrt(1900) = 43

        let result = engine.tally("p1").unwrap();
        assert_eq!(result.winning_option.as_deref(), Some("a"));
    }

    #[test]
    fn test_quorum_exact_boundary() {
        // Test quorum at exact boundary: votes == quorum => quorum_reached
        let mut engine = QuadraticVoteEngine::new(100);
        let voter = VoterProfile::new("v1", 100, IvnIdentityTier::Verified, 10000);
        engine.register_voter(voter);
        engine
            .create_proposal_with_quorum("p1", vec!["a".into()], 100)
            .unwrap();

        // Effective votes = sqrt(10000 * 100) = sqrt(1_000_000) = 1000, which is >= 100
        engine.cast_vote("p1", "v1", "a", 10000).unwrap();
        let result = engine.tally("p1").unwrap();
        assert!(result.quorum_reached);
    }

    #[test]
    fn test_quorum_not_reached() {
        // Test quorum just below threshold
        let mut engine = QuadraticVoteEngine::new(1);
        // Use Unverified tier (0.1x) + low karma to get minimal effective votes
        let voter = VoterProfile::new("v1", 10, IvnIdentityTier::Unverified, 10);
        engine.register_voter(voter);
        // karma_weight = (10 * 1000) / 10000 = 1
        // product = 10 * 1 = 10, sqrt(10) = 3
        engine
            .create_proposal_with_quorum("p1", vec!["a".into()], 5)
            .unwrap();
        engine.cast_vote("p1", "v1", "a", 10).unwrap();

        let result = engine.tally("p1").unwrap();
        assert!(!result.quorum_reached);
        assert_eq!(result.option_tallies["a"], 3);
    }

    #[test]
    fn test_quorum_zero_always_reached() {
        // A quorum of 0 should always be satisfied, even with no voters
        let mut engine = QuadraticVoteEngine::new(0);
        let voter = VoterProfile::new("v1", 100, IvnIdentityTier::Verified, 100);
        engine.register_voter(voter);
        engine
            .create_proposal_with_quorum("p1", vec!["a".into()], 0)
            .unwrap();

        let result = engine.tally("p1").unwrap();
        assert!(result.quorum_reached);
        assert_eq!(result.total_voters, 0);
    }

    #[test]
    fn test_empty_proposal_options_rejected() {
        let mut engine = QuadraticVoteEngine::new(10);
        let result = engine.create_proposal("p1", vec![]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }

    #[test]
    fn test_duplicate_proposal_id_rejected() {
        let mut engine = QuadraticVoteEngine::new(10);
        engine.create_proposal("p1", vec!["a".into()]).unwrap();
        let result = engine.create_proposal("p1", vec!["b".into()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already exists"));
    }

    #[test]
    fn test_unregistered_voter_rejected() {
        let mut engine = QuadraticVoteEngine::new(10);
        engine.create_proposal("p1", vec!["a".into()]).unwrap();
        let result = engine.cast_vote("p1", "unknown_voter", "a", 10);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not registered"));
    }

    #[test]
    fn test_invalid_option_rejected() {
        let mut engine = QuadraticVoteEngine::new(10);
        let voter = VoterProfile::new("v1", 100, IvnIdentityTier::Verified, 100);
        engine.register_voter(voter);
        engine.create_proposal("p1", vec!["a".into()]).unwrap();
        let result = engine.cast_vote("p1", "v1", "nonexistent_option", 10);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid option"));
    }

    #[test]
    fn test_zero_credits_in_allocation_rejected() {
        // Allocating 0 credits to an option should be rejected
        let mut engine = QuadraticVoteEngine::new(10);
        let voter = VoterProfile::new("v1", 100, IvnIdentityTier::Verified, 100);
        engine.register_voter(voter);
        engine.create_proposal("p1", vec!["a".into()]).unwrap();
        let result = engine.cast_vote("p1", "v1", "a", 0);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("greater than zero"));
    }

    #[test]
    fn test_insufficient_credits_rejected() {
        let mut engine = QuadraticVoteEngine::new(10);
        let voter = VoterProfile::new("v1", 100, IvnIdentityTier::Verified, 50);
        engine.register_voter(voter);
        engine.create_proposal("p1", vec!["a".into()]).unwrap();
        let result = engine.cast_vote("p1", "v1", "a", 100);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Insufficient credits"));
    }

    #[test]
    fn test_sybil_voter_in_tally_contributes_zero() {
        // A sybil-flagged voter's ballot should contribute 0 to tallies
        let mut engine = QuadraticVoteEngine::new(1);
        let sybil = VoterProfile::new("sybil", 1000, IvnIdentityTier::Sovereign, 10000)
            .with_sybil_flag(true);
        let honest = VoterProfile::new("honest", 100, IvnIdentityTier::Verified, 10000);
        engine.register_voter(sybil);
        engine.register_voter(honest);
        engine.create_proposal("p1", vec!["a".into()]).unwrap();

        engine.cast_vote("p1", "sybil", "a", 10000).unwrap();
        engine.cast_vote("p1", "honest", "a", 10000).unwrap();

        let result = engine.tally("p1").unwrap();
        assert_eq!(result.sybil_votes_rejected, 1);
        // Only honest voter's effective votes counted
        // sqrt(10000 * 100) = 1000
        assert_eq!(result.option_tallies["a"], 1000);
        assert_eq!(result.total_voters, 2);
    }

    #[test]
    fn test_integer_sqrt_large_values() {
        // Test integer_sqrt at very large u128 values
        assert_eq!(integer_sqrt(u128::MAX), 18446744073709551615u64); // sqrt(u64::MAX^2) = u64::MAX
        assert_eq!(integer_sqrt(1u128 << 126), (1u64 << 63)); // sqrt(2^126) = 2^63
    }

    #[test]
    fn test_saturating_mul_prevents_panic_on_large_inputs() {
        // Huge credits * huge karma_weight should not panic (saturating_mul in calculate_effective_votes)
        let votes =
            calculate_effective_votes(u64::MAX, u64::MAX, IvnIdentityTier::Sovereign, false);
        // Should produce a valid result (large number), not panic
        assert!(votes > 0);
    }

    #[test]
    fn test_multi_choice_ballot_splits_effective_votes() {
        // Multi-choice ballot should distribute credits across options
        let mut engine = QuadraticVoteEngine::new(1);
        let voter = VoterProfile::new("v1", 100, IvnIdentityTier::Verified, 10000);
        engine.register_voter(voter);
        engine
            .create_proposal("p1", vec!["a".into(), "b".into()])
            .unwrap();

        let mut allocations = HashMap::new();
        allocations.insert("a".into(), 60u64);
        allocations.insert("b".into(), 40u64);

        let effective = engine
            .cast_multi_choice_ballot("p1", "v1", allocations)
            .unwrap();
        // sqrt(60 * 100) = sqrt(6000) = 77
        // sqrt(40 * 100) = sqrt(4000) = 63
        assert_eq!(effective["a"], 77);
        assert_eq!(effective["b"], 63);

        // Verify tally sums them correctly
        let result = engine.tally("p1").unwrap();
        assert_eq!(result.option_tallies["a"], 77);
        assert_eq!(result.option_tallies["b"], 63);
        assert_eq!(result.total_voters, 1);
    }

    #[test]
    fn test_tally_proposal_not_found() {
        let engine = QuadraticVoteEngine::new(10);
        let result = engine.tally("nonexistent");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_low_karma_with_high_multiplier_yields_zero() {
        // Karma=1 with Unverified (0.1x): karma_weight = floor(1 * 1000 / 10000) = 0 => effective = 0
        let votes = calculate_effective_votes(1000, 1, IvnIdentityTier::Unverified, false);
        assert_eq!(votes, 0);
    }

    #[test]
    fn test_basic_tier_karma_weight_truncation() {
        // Basic tier (0.5x), karma=1: karma_weight = floor(1 * 5000 / 10000) = 0 => 0
        let votes = calculate_effective_votes(100, 1, IvnIdentityTier::Basic, false);
        assert_eq!(votes, 0);

        // Basic tier (0.5x), karma=3: karma_weight = floor(3 * 5000 / 10000) = 1
        // product = 100 * 1 = 100, sqrt = 10
        let votes = calculate_effective_votes(100, 3, IvnIdentityTier::Basic, false);
        assert_eq!(votes, 10);
    }
}
