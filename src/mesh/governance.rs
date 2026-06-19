#[cfg(feature = "dao-evm")]
use alloy::primitives::Address;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for on-chain EVM integration.
/// Feature-gated behind `cfg(feature = "dao-evm")`.
#[cfg(feature = "dao-evm")]
#[derive(Debug, Clone)]
pub struct EvmDaoConfig {
    pub rpc_url: String,
    pub contract_address: Address,
    pub chain_id: u64,
    pub private_key: String,
}

/// Represents a grouped Issue (Epic) waiting for community consensus.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GovernanceProposal {
    pub cluster_id: String,
    pub title: String,
    pub description: String,
    pub upvotes: u64,
    pub downvotes: u64,
    pub is_approved_for_pr: bool,
    pub assigned_maintainer: Option<String>,
}

pub struct DaoGovernanceSystem {
    // In production, this syncs with GitHub API or Solana Smart Contract state
    pub active_proposals: HashMap<String, GovernanceProposal>,
    pub required_approval_threshold: f64, // 0.0 to 1.0 (e.g., 0.8 = 80% approval)
    pub minimum_quorum: u64,              // Minimum total votes required
    pub maintainer_registry: HashMap<String, u64>, // Maps NodeID/Wallet to their Trust Score
    /// Optional EVM on-chain config. When Some, votes go on-chain via alloy.
    #[cfg(feature = "dao-evm")]
    pub evm_config: Option<EvmDaoConfig>,
}

impl DaoGovernanceSystem {
    pub fn new() -> Self {
        let mut registry = HashMap::new();
        // Base trust scores for the mock DAO
        registry.insert("JULES_AGENT".to_string(), 950);
        registry.insert("DEV_ALPHA".to_string(), 300);
        registry.insert("DEV_BETA".to_string(), 50);

        Self {
            active_proposals: HashMap::new(),
            required_approval_threshold: 0.80, // 80% consensus required
            minimum_quorum: 5,                 // At least 5 maintainers must vote
            maintainer_registry: registry,
            #[cfg(feature = "dao-evm")]
            evm_config: None,
        }
    }

    /// Creates a new DAO governance system with on-chain EVM integration.
    /// Requires feature `dao-evm` enabled.
    #[cfg(feature = "dao-evm")]
    pub fn with_evm(config: EvmDaoConfig) -> Self {
        let mut system = Self::new();
        system.evm_config = Some(config);
        system
    }

    /// Submits a newly clustered anomaly to the governance board.
    pub fn submit_proposal(&mut self, cluster_id: &str, title: &str, description: &str) {
        if !self.active_proposals.contains_key(cluster_id) {
            self.active_proposals.insert(
                cluster_id.to_string(),
                GovernanceProposal {
                    cluster_id: cluster_id.to_string(),
                    title: title.to_string(),
                    description: description.to_string(),
                    upvotes: 0,
                    downvotes: 0,
                    is_approved_for_pr: false,
                    assigned_maintainer: None,
                },
            );
        }
    }

    /// Simulates a vote cast via GitHub Reaction (👍 or 👎).
    pub fn cast_vote(&mut self, cluster_id: &str, approve: bool) -> Result<(), String> {
        let proposal = self
            .active_proposals
            .get_mut(cluster_id)
            .ok_or_else(|| "Proposal not found".to_string())?;

        if approve {
            proposal.upvotes += 1;
        } else {
            proposal.downvotes += 1;
        }

        self.evaluate_consensus(cluster_id);
        Ok(())
    }

    /// Checks if the proposal has met the democratic threshold to unlock PRs.
    fn evaluate_consensus(&mut self, cluster_id: &str) {
        use rand::distributions::WeightedIndex;
        use rand::prelude::*;

        if let Some(proposal) = self.active_proposals.get_mut(cluster_id) {
            let total_votes = proposal.upvotes + proposal.downvotes;
            if total_votes >= self.minimum_quorum && !proposal.is_approved_for_pr {
                let approval_ratio = proposal.upvotes as f64 / total_votes as f64;
                if approval_ratio >= self.required_approval_threshold {
                    proposal.is_approved_for_pr = true;
                    
                    // Assign randomly using a Weighted Lottery based on Trust Score
                    if !self.maintainer_registry.is_empty() {
                        let maintainers: Vec<(&String, &u64)> = self.maintainer_registry.iter().collect();
                        let weights: Vec<u64> = maintainers.iter().map(|(_, &score)| std::cmp::max(1, score)).collect();
                        
                        if let Ok(dist) = WeightedIndex::new(&weights) {
                            let mut rng = thread_rng();
                            let winner = maintainers[dist.sample(&mut rng)].0;
                            proposal.assigned_maintainer = Some(winner.to_string());
                        }
                    }
                }
            }
        }
    }
}

impl Default for DaoGovernanceSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dao_governance_consensus() {
        let mut dao = DaoGovernanceSystem::new();
        dao.submit_proposal("CLUSTER_P2P", "Fix Sync Race", "P2P network race condition detected.");
        
        // Cast 4 upvotes (not enough quorum)
        for _ in 0..4 {
            dao.cast_vote("CLUSTER_P2P", true).unwrap();
        }
        let prop = dao.active_proposals.get("CLUSTER_P2P").unwrap();
        assert!(!prop.is_approved_for_pr); // Quorum is 5

        // Cast 1 downvote (total 5 votes: 4 up, 1 down = 80%)
        dao.cast_vote("CLUSTER_P2P", false).unwrap();
        
        let prop = dao.active_proposals.get("CLUSTER_P2P").unwrap();
        assert!(prop.is_approved_for_pr); // Reached 80% with 5 votes!
        assert!(prop.assigned_maintainer.is_some()); // Ensure a winner was randomly picked
        println!("Winner: {:?}", prop.assigned_maintainer);
    }

    #[test]
    fn test_dao_governance_rejection() {
        let mut dao = DaoGovernanceSystem::new();
        dao.submit_proposal("CLUSTER_UI", "Change Button Color", "Minor UI tweak.");
        
        // Cast 3 upvotes and 3 downvotes (50%, below 80% threshold)
        for _ in 0..3 { dao.cast_vote("CLUSTER_UI", true).unwrap(); }
        for _ in 0..3 { dao.cast_vote("CLUSTER_UI", false).unwrap(); }
        
        let prop = dao.active_proposals.get("CLUSTER_UI").unwrap();
        assert!(!prop.is_approved_for_pr);
    }
}
