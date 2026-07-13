#[cfg(feature = "dao-evm")]
use alloy::{
    network::{Ethereum, EthereumWallet},
    primitives::Address,
    providers::{Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
    sol,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(feature = "dao-evm")]
sol!(
    #[sol(rpc)]
    interface IXavierDAO {
        function createProposal(bytes32 clusterId, string calldata title, string calldata description) external;
        function castVote(bytes32 clusterId, bool approve) external;
        function getProposalStatus(bytes32 clusterId) external view returns (bool approved, uint64 upvotes, uint64 downvotes);
    }
);

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
    pub voter_count: u64,
    pub is_approved_for_pr: bool,
    pub assigned_maintainer: Option<String>,
}

/// Maximum vote weight cap (5% of total staked)
pub const VOTE_WEIGHT_CAP_BPS: u64 = 500;

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
                    voter_count: 0,
                    is_approved_for_pr: false,
                    assigned_maintainer: None,
                },
            );

            #[cfg(feature = "dao-evm")]
            if let Some(config) = &self.evm_config {
                let _ = self
                    .submit_proposal_evm(cluster_id, title, description)
                    .await;
            }
        }
    }

    #[cfg(feature = "dao-evm")]
    async fn submit_proposal_evm(
        &self,
        _cluster_id: &str,
        _title: &str,
        _description: &str,
    ) -> anyhow::Result<()> {
        /* Placeholder for EVM integration until alloy configuration is stable
        let config = self.evm_config.as_ref().ok_or_else(|| anyhow::anyhow!("EVM config missing"))?;
        let signer: PrivateKeySigner = config.private_key.parse()?;
        let wallet = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new()
            .network::<Ethereum>()
            .wallet(wallet)
            .connect_http(config.rpc_url.parse()?).await?;

        let contract = IXavierDAO::new(config.contract_address, provider);

        let mut cluster_id_bytes = [0u8; 32];
        let bytes = _cluster_id.as_bytes();
        let len = bytes.len().min(32);
        cluster_id_bytes[..len].copy_from_slice(&bytes[..len]);

        let tx = contract.createProposal(cluster_id_bytes.into(), _title.to_string(), _description.to_string());
        let _receipt = tx.send().await?;
        */

        Ok(())
    }

    /// Calculates the vote weight based on stake with a quadratic cap.
    /// Formula: vote_weight = min(sqrt(stake_bps * 100), 500 bps)
    pub fn calculate_vote_weight(&self, stake_bps: u64) -> u64 {
        let weight = ((stake_bps as f64 * 100.0).sqrt() as u64).min(VOTE_WEIGHT_CAP_BPS);
        weight
    }

    /// Simulates a vote cast via GitHub Reaction (👍 or 👎).
    pub async fn cast_vote(
        &mut self,
        cluster_id: &str,
        approve: bool,
        stake_bps: u64,
    ) -> Result<(), String> {
        let weight = self.calculate_vote_weight(stake_bps);

        {
            let proposal = self
                .active_proposals
                .get_mut(cluster_id)
                .ok_or_else(|| "Proposal not found".to_string())?;

            if approve {
                proposal.upvotes += weight;
            } else {
                proposal.downvotes += weight;
            }
            proposal.voter_count += 1;
        }

        #[cfg(feature = "dao-evm")]
        if let Some(config) = &self.evm_config {
            let _ = self.cast_vote_evm(cluster_id, approve).await;
        }

        self.evaluate_consensus(cluster_id);
        Ok(())
    }

    #[cfg(feature = "dao-evm")]
    async fn cast_vote_evm(&self, _cluster_id: &str, _approve: bool) -> anyhow::Result<()> {
        /* Placeholder for EVM integration until alloy configuration is stable
        let config = self.evm_config.as_ref().ok_or_else(|| anyhow::anyhow!("EVM config missing"))?;
        let signer: PrivateKeySigner = config.private_key.parse()?;
        let wallet = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new()
            .network::<Ethereum>()
            .wallet(wallet)
            .connect_http(config.rpc_url.parse()?).await?;

        let contract = IXavierDAO::new(config.contract_address, provider);

        let mut cluster_id_bytes = [0u8; 32];
        let bytes = _cluster_id.as_bytes();
        let len = bytes.len().min(32);
        cluster_id_bytes[..len].copy_from_slice(&bytes[..len]);

        let tx = contract.castVote(cluster_id_bytes.into(), _approve);
        let _receipt = tx.send().await?;
        */

        Ok(())
    }

    /// Checks if the proposal has met the democratic threshold to unlock PRs.
    fn evaluate_consensus(&mut self, cluster_id: &str) {
        use rand::distributions::WeightedIndex;
        use rand::prelude::*;

        if let Some(proposal) = self.active_proposals.get_mut(cluster_id) {
            if proposal.voter_count >= self.minimum_quorum && !proposal.is_approved_for_pr {
                let total_votes = proposal.upvotes + proposal.downvotes;
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

        // Cast 4 upvotes (stake_bps=1 -> weight=10; not enough quorum)
        for _ in 0..4 {
            dao.cast_vote("CLUSTER_P2P", true, 1).await.unwrap();
        }
        let prop = dao.active_proposals.get("CLUSTER_P2P").unwrap();
        assert!(!prop.is_approved_for_pr); // Quorum is 5

        // Cast 1 downvote (stake_bps=1 -> weight=10; total 50 votes: 40 up, 10 down = 80%)
        dao.cast_vote("CLUSTER_P2P", false, 1).await.unwrap();

        let prop = dao.active_proposals.get("CLUSTER_P2P").unwrap();
        assert!(prop.is_approved_for_pr); // Reached 80% with 50 weighted votes!
        assert!(prop.assigned_maintainer.is_some()); // Ensure a winner was randomly picked
        println!("Winner: {:?}", prop.assigned_maintainer);
    }

    #[tokio::test]
    async fn test_dao_governance_rejection() {
        let mut dao = DaoGovernanceSystem::new();
        dao.submit_proposal("CLUSTER_UI", "Change Button Color", "Minor UI tweak.")
            .await;

        // Cast 3 upvotes and 3 downvotes (stake_bps=1 -> weight=10; 50%, below 80% threshold)
        for _ in 0..3 {
            dao.cast_vote("CLUSTER_UI", true, 1).await.unwrap();
        }
        for _ in 0..3 {
            dao.cast_vote("CLUSTER_UI", false, 1).await.unwrap();
        }

        let prop = dao.active_proposals.get("CLUSTER_UI").unwrap();
        assert!(!prop.is_approved_for_pr);
    }
}
