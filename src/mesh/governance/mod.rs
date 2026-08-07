pub mod onchain;

#[cfg(any(feature = "dao-evm", test))]
pub use onchain::{EvmDaoConfig, OnchainDaoClient};

#[cfg(feature = "dao-evm")]
use alloy::primitives::Address;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    pub wallets: HashMap<String, crate::mesh::tokenomics::wallet::Wallet>, // Maps NodeID/Wallet to Wallet
    /// Optional EVM on-chain config. When Some, votes go on-chain via alloy.
    #[cfg(feature = "dao-evm")]
    pub evm_config: Option<EvmDaoConfig>,
}

impl DaoGovernanceSystem {
    /// New.
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
            wallets: HashMap::new(),
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
    pub async fn submit_proposal(&mut self, cluster_id: &str, title: &str, description: &str) {
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

            #[cfg(feature = "dao-evm")]
            if let Some(_) = &self.evm_config {
                let _ = self
                    .submit_proposal_evm(cluster_id, title, description)
                    .await;
            }
        }
    }

    #[cfg(feature = "dao-evm")]
    async fn submit_proposal_evm(
        &self,
        cluster_id: &str,
        title: &str,
        description: &str,
    ) -> anyhow::Result<()> {
        let config = self
            .evm_config
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("EVM config missing"))?;
        let client = OnchainDaoClient::new(config.clone());
        client.propose(cluster_id, title, description).await
    }

    /// Casts a vote weighted by the voter's tokenomics XP wallet balance.
    pub async fn cast_vote(
        &mut self,
        cluster_id: &str,
        voter: &str,
        approve: bool,
        is_council: bool,
    ) -> Result<(), String> {
        let voting_power = if let Some(wallet) = self.wallets.get(voter) {
            wallet.get_effective_balance()
        } else {
            1
        };

        {
            let proposal = self
                .active_proposals
                .get_mut(cluster_id)
                .ok_or_else(|| "Proposal not found".to_string())?;

            let weight = if is_council { 1 } else { voting_power };
            if approve {
                proposal.upvotes += weight;
            } else {
                proposal.downvotes += weight;
            }
        }

        #[cfg(feature = "dao-evm")]
        if let Some(_) = &self.evm_config {
            let _ = self
                .cast_vote_evm(cluster_id, approve, voting_power, is_council)
                .await;
        }

        self.evaluate_consensus(cluster_id);
        Ok(())
    }

    /// Synchronizes the local state with the on-chain status of all active proposals.
    /// Requires feature `dao-evm` enabled.
    #[cfg(feature = "dao-evm")]
    pub async fn sync_from_chain(&mut self) -> anyhow::Result<()> {
        let config = self
            .evm_config
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("EVM config missing"))?;
        let client = OnchainDaoClient::new(config.clone());

        for (cluster_id, proposal) in self.active_proposals.iter_mut() {
            match client.get_proposal_status(cluster_id).await {
                Ok((
                    approved,
                    upvotes_yes,
                    upvotes_no,
                    council_yes,
                    council_no,
                    _vetoed,
                    _executed,
                )) => {
                    proposal.is_approved_for_pr = approved;
                    // Aggregate upvotes and downvotes from user & council
                    proposal.upvotes = upvotes_yes + council_yes;
                    proposal.downvotes = upvotes_no + council_no;
                }
                Err(e) => {
                    tracing::error!("Failed to sync proposal {} from chain: {:?}", cluster_id, e)
                }
            }
        }

        Ok(())
    }

    #[cfg(feature = "dao-evm")]
    async fn cast_vote_evm(
        &self,
        cluster_id: &str,
        approve: bool,
        voting_power: u64,
        is_council: bool,
    ) -> anyhow::Result<()> {
        let config = self
            .evm_config
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("EVM config missing"))?;
        let client = OnchainDaoClient::new(config.clone());
        client
            .vote(cluster_id, approve, voting_power, is_council)
            .await
    }

    /// Executes an approved proposal on-chain.
    /// Requires feature `dao-evm` enabled.
    #[cfg(feature = "dao-evm")]
    pub async fn execute_proposal_onchain(&self, cluster_id: &str) -> anyhow::Result<()> {
        let config = self
            .evm_config
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("EVM config missing"))?;
        let client = OnchainDaoClient::new(config.clone());
        client.execute(cluster_id).await
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
                        let maintainers: Vec<(&String, &u64)> =
                            self.maintainer_registry.iter().collect();
                        let weights: Vec<u64> = maintainers
                            .iter()
                            .map(|(_, &score)| std::cmp::max(1, score))
                            .collect();

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

    #[tokio::test]
    async fn test_dao_governance_consensus() {
        let mut dao = DaoGovernanceSystem::new();
        dao.submit_proposal(
            "CLUSTER_P2P",
            "Fix Sync Race",
            "P2P network race condition detected.",
        )
        .await;

        // Cast 4 upvotes (not enough quorum)
        for i in 0..4 {
            let voter = format!("voter_{}", i);
            dao.cast_vote("CLUSTER_P2P", &voter, true, false)
                .await
                .unwrap();
        }
        let prop = dao.active_proposals.get("CLUSTER_P2P").unwrap();
        assert!(!prop.is_approved_for_pr); // Quorum is 5

        // Cast 1 downvote (total 5 votes: 4 up, 1 down = 80%)
        dao.cast_vote("CLUSTER_P2P", "voter_4", false, false)
            .await
                .unwrap();

        let prop = dao.active_proposals.get("CLUSTER_P2P").unwrap();
        assert!(prop.is_approved_for_pr); // Reached 80% with 5 votes!
        assert!(prop.assigned_maintainer.is_some()); // Ensure a winner was randomly picked
        println!("Winner: {:?}", prop.assigned_maintainer);
    }

    #[tokio::test]
    async fn test_dao_governance_rejection() {
        let mut dao = DaoGovernanceSystem::new();
        dao.submit_proposal("CLUSTER_UI", "Change Button Color", "Minor UI tweak.")
            .await;

        // Cast 3 upvotes and 3 downvotes (50%, below 80% threshold)
        for i in 0..3 {
            let voter_y = format!("voter_y_{}", i);
            let voter_n = format!("voter_n_{}", i);
            dao.cast_vote("CLUSTER_UI", &voter_y, true, false)
                .await
                .unwrap();
            dao.cast_vote("CLUSTER_UI", &voter_n, false, false)
                .await
                .unwrap();
        }

        let prop = dao.active_proposals.get("CLUSTER_UI").unwrap();
        assert!(!prop.is_approved_for_pr);
    }

    #[cfg(feature = "dao-evm")]
    #[tokio::test]
    async fn test_governance_dao_submit_vote_evm() {
        let config = EvmDaoConfig {
            rpc_url: "http://localhost:8545".to_string(),
            contract_address: Address::ZERO,
            chain_id: 1,
            private_key: "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
                .to_string(),
        };

        let mut dao = DaoGovernanceSystem::with_evm(config);
        dao.submit_proposal("EVM_1", "Test EVM", "Desc").await;
        assert!(dao.active_proposals.contains_key("EVM_1"));
    }

    #[cfg(feature = "dao-evm")]
    #[tokio::test]
    async fn test_governance_dao_sync_from_chain() {
        let config = EvmDaoConfig {
            rpc_url: "http://localhost:8545".to_string(),
            contract_address: Address::ZERO,
            chain_id: 1,
            private_key: "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
                .to_string(),
        };

        let mut dao = DaoGovernanceSystem::with_evm(config);
        dao.submit_proposal("EVM_SYNC", "Sync Test", "Desc").await;

        let _ = dao.sync_from_chain().await;
        assert!(dao.active_proposals.contains_key("EVM_SYNC"));
    }

    #[cfg(feature = "dao-evm")]
    #[tokio::test]
    async fn test_governance_dao_execute_proposal_onchain() {
        let config = EvmDaoConfig {
            rpc_url: "http://localhost:8545".to_string(),
            contract_address: Address::ZERO,
            chain_id: 80002, // Polygon Amoy
            private_key: "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
                .to_string(),
        };

        let dao = DaoGovernanceSystem::with_evm(config);
        let err = dao.execute_proposal_onchain("EVM_EXEC").await;
        assert!(err.is_err());
    }

    #[cfg(feature = "dao-evm")]
    #[tokio::test]
    async fn test_onchain_client_direct() {
        let config = EvmDaoConfig {
            rpc_url: "http://localhost:8545".to_string(),
            contract_address: Address::ZERO,
            chain_id: 80002, // Polygon Amoy
            private_key: "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
                .to_string(),
        };

        let client = OnchainDaoClient::new(config);
        let err1 = client.propose("EVM_1", "Test", "Desc").await;
        assert!(err1.is_err());

        let err2 = client.vote("EVM_1", true, 100, false).await;
        assert!(err2.is_err());

        let err3 = client.get_proposal_status("EVM_1").await;
        assert!(err3.is_err());
    }

    #[tokio::test]
    async fn test_dao_governance_wallet_integration() {
        use crate::mesh::node::NodeId;
        use crate::mesh::tokenomics::wallet::{TransactionKind, Wallet};

        let mut dao = DaoGovernanceSystem::new();
        dao.submit_proposal("CLUSTER_BOUNTY", "Bounty Proposal", "Proposal description")
            .await;

        let node_id = NodeId::parse("xv1-testnode0000000").unwrap();
        let mut wallet = Wallet::new(node_id);
        wallet.credit(500, TransactionKind::Reward, "Initial reward");

        // Insert wallet in the DAO's wallet map for this voter
        dao.wallets.insert("DEV_XP_VOTER".to_string(), wallet);

        // Cast vote
        dao.cast_vote("CLUSTER_BOUNTY", "DEV_XP_VOTER", true, false)
            .await
            .unwrap();

        let prop = dao.active_proposals.get("CLUSTER_BOUNTY").unwrap();
        assert_eq!(prop.upvotes, 500); // voting power resolved from XP wallet balance!
    }
}
