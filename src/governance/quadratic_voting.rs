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
    pub fn new(voter_id: impl Into<String>, karma: u64, identity_tier: IvnIdentityTier, credit_balance: u64) -> Self {
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
    let mut x = (1u128 << ((128 - n.leading_zeros() + 1) / 2)) as u128;
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
    let karma_weight = (karma as u128)
        .saturating_mul(multiplier_bps as u128)
        / 10000u128;

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
            return Err(format!("Voter '{}' has already voted on proposal '{}'", voter_id, proposal_id));
        }

        if allocations.is_empty() {
            return Err("Ballot allocations cannot be empty".to_string());
        }

        let mut total_credits_spent: u64 = 0;
        for (option, &credits) in &allocations {
            if !proposal.options.contains(option) {
                return Err(format!("Invalid option '{}' for proposal '{}'", option, proposal_id));
            }
            if credits == 0 {
                return Err(format!("Credits allocated to option '{}' must be greater than zero", option));
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

            let is_sybil = voter.map_or(false, |v| v.is_sybil_flagged);
            if is_sybil {
                sybil_votes_rejected += 1;
                continue;
            }

            for (option, &credits) in &ballot.allocations {
                let (karma, tier) = voter.map_or((0, IvnIdentityTier::Unverified), |v| (v.karma, v.identity_tier));
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
        let sovereign_votes = calculate_effective_votes(100, 100, IvnIdentityTier::Sovereign, false);
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
}
