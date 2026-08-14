#[cfg(feature = "dao-evm")]
use alloy::{
    network::{Ethereum, EthereumWallet},
    primitives::Address,
    providers::ProviderBuilder,
    signers::local::PrivateKeySigner,
    sol,
};

#[cfg(feature = "dao-evm")]
sol!(
    #[sol(rpc)]
    contract XavierDAO {
        event ProposalCreated(bytes32 indexed clusterId, string title, string description, address indexed creator, uint256 deadline);
        event VoteCast(bytes32 indexed clusterId, address indexed voter, bool approve, uint256 votingPower, bool isCouncil);
        event ProposalExecuted(bytes32 indexed clusterId);

        function createProposal(bytes32 clusterId, string calldata title, string calldata description) external;
        function castVote(bytes32 clusterId, bool approve, uint256 votingPower, bool isCouncil) external;
        function executeProposal(bytes32 clusterId) external;
        function getProposalStatus(bytes32 clusterId) external view returns (
            bool approved,
            uint256 userVotesYes,
            uint256 userVotesNo,
            uint256 councilVotesYes,
            uint256 councilVotesNo,
            bool vetoed,
            bool executed
        );
        function vetoProposal(bytes32 clusterId, string calldata reason) external;
        function overruleVeto(bytes32 clusterId) external;
    }
);

use serde::{Deserialize, Serialize};

/// Configuration for on-chain EVM integration.
/// Feature-gated behind `cfg(feature = "dao-evm")` or under test.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvmDaoConfig {
    pub rpc_url: String,
    #[cfg(feature = "dao-evm")]
    pub contract_address: Address,
    #[cfg(not(feature = "dao-evm"))]
    pub contract_address: String,
    pub chain_id: u64,
    pub private_key: String,
}

#[derive(Debug, Clone)]
pub struct OnchainDaoClient {
    pub config: EvmDaoConfig,
}

impl OnchainDaoClient {
    pub fn new(config: EvmDaoConfig) -> Self {
        Self { config }
    }

    /// Formats the cluster_id to a 32-byte array (padding or truncating as needed)
    pub fn format_cluster_id(&self, cluster_id: &str) -> [u8; 32] {
        let mut cluster_id_bytes = [0u8; 32];
        let bytes = cluster_id.as_bytes();
        let len = bytes.len().min(32);
        cluster_id_bytes[..len].copy_from_slice(&bytes[..len]);
        cluster_id_bytes
    }

    /// Validates the configuration and parameters
    pub fn validate_params(&self, cluster_id: &str) -> Result<(), String> {
        if self.config.rpc_url.is_empty() {
            return Err("RPC URL cannot be empty".to_string());
        }
        if cluster_id.is_empty() {
            return Err("Cluster ID cannot be empty".to_string());
        }
        Ok(())
    }

    /// Helper to construct full URL for querying RPC
    pub fn construct_rpc_query_url(&self, path: &str) -> Result<String, String> {
        if self.config.rpc_url.is_empty() {
            return Err("Empty RPC URL".to_string());
        }
        let base = self.config.rpc_url.trim_end_matches('/');
        Ok(format!("{}/{}", base, path.trim_start_matches('/')))
    }

    /// Converts a 32-byte cluster_id back to a UTF-8 string by trimming trailing null bytes.
    pub fn parse_cluster_id(&self, cluster_id_bytes: &[u8; 32]) -> String {
        let len = cluster_id_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(32);
        String::from_utf8_lossy(&cluster_id_bytes[..len]).to_string()
    }

    /// Synchronizes events from the XavierDAO smart contract into the local GovernanceDao state.
    #[cfg(feature = "dao-evm")]
    pub async fn sync_from_chain(
        &self,
        dao: &mut crate::governance::dao::GovernanceDao,
        from_block: u64,
    ) -> anyhow::Result<usize> {
        use alloy::{
            network::Ethereum,
            providers::{Provider, ProviderBuilder},
            rpc::types::eth::Filter,
            sol_types::SolEventInterface,
        };

        let provider = ProviderBuilder::new()
            .network::<Ethereum>()
            .connect_http(self.config.rpc_url.parse::<url::Url>()?);

        let filter = Filter::new()
            .address(self.config.contract_address)
            .from_block(from_block);

        let logs = provider.get_logs(&filter).await?;
        let mut count = 0;

        for log in logs {
            if let Ok(event) = XavierDAO::XavierDAOEvents::decode_log(&log.inner) {
                match event.data {
                    XavierDAO::XavierDAOEvents::ProposalCreated(e) => {
                        let cluster_id = self.parse_cluster_id(&e.clusterId.into());
                        let creator = format!("{:#x}", e.creator);
                        let deadline = e.deadline.to::<u64>();
                        let options = vec!["Approve".to_string(), "Reject".to_string()];

                        let proposal = crate::governance::dao::Proposal {
                            id: cluster_id.clone(),
                            title: e.title,
                            description: e.description,
                            options,
                            deadline,
                            creator,
                            status: crate::governance::dao::ProposalStatus::Active,
                            votes: std::collections::HashMap::new(),
                        };
                        dao.proposals.insert(cluster_id, proposal);
                        count += 1;
                    }
                    XavierDAO::XavierDAOEvents::VoteCast(e) => {
                        let cluster_id = self.parse_cluster_id(&e.clusterId.into());
                        let voter = format!("{:#x}", e.voter);
                        let option = if e.approve {
                            "Approve".to_string()
                        } else {
                            "Reject".to_string()
                        };
                        let credits = e.votingPower.to::<u64>();

                        if let Some(proposal) = dao.proposals.get_mut(&cluster_id) {
                            proposal.votes.insert(voter, (option, credits));
                            count += 1;
                        }
                    }
                    XavierDAO::XavierDAOEvents::ProposalExecuted(e) => {
                        let cluster_id = self.parse_cluster_id(&e.clusterId.into());
                        if let Some(proposal) = dao.proposals.get_mut(&cluster_id) {
                            proposal.status = crate::governance::dao::ProposalStatus::Executed;
                            count += 1;
                        }
                    }
                }
            }
        }

        Ok(count)
    }

    /// Fallback implementation when `dao-evm` is disabled.
    #[cfg(not(feature = "dao-evm"))]
    pub async fn sync_from_chain(
        &self,
        _dao: &mut crate::governance::dao::GovernanceDao,
        _from_block: u64,
    ) -> anyhow::Result<usize> {
        Ok(0)
    }

    /// Submits a proposal to the XavierDAO contract.
    #[cfg(feature = "dao-evm")]
    pub async fn propose(
        &self,
        cluster_id: &str,
        title: &str,
        description: &str,
    ) -> anyhow::Result<()> {
        let signer: PrivateKeySigner = self.config.private_key.parse()?;
        let wallet = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new()
            .network::<Ethereum>()
            .wallet(wallet)
            .connect_http(self.config.rpc_url.parse::<url::Url>()?);

        let contract = XavierDAO::new(self.config.contract_address, provider);

        let cluster_id_bytes = self.format_cluster_id(cluster_id);

        let tx = contract.createProposal(
            cluster_id_bytes.into(),
            title.to_string(),
            description.to_string(),
        );
        let _receipt = tx.send().await?;
        Ok(())
    }

    /// Casts a vote on-chain with XP voting power / council flag.
    #[cfg(feature = "dao-evm")]
    pub async fn vote(
        &self,
        cluster_id: &str,
        approve: bool,
        voting_power: u64,
        is_council: bool,
    ) -> anyhow::Result<()> {
        let signer: PrivateKeySigner = self.config.private_key.parse()?;
        let wallet = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new()
            .network::<Ethereum>()
            .wallet(wallet)
            .connect_http(self.config.rpc_url.parse::<url::Url>()?);

        let contract = XavierDAO::new(self.config.contract_address, provider);

        let cluster_id_bytes = self.format_cluster_id(cluster_id);

        let tx = contract.castVote(
            cluster_id_bytes.into(),
            approve,
            alloy::primitives::U256::from(voting_power),
            is_council,
        );
        let _receipt = tx.send().await?;
        Ok(())
    }

    /// Vetoes a proposal on-chain.
    #[cfg(feature = "dao-evm")]
    pub async fn veto(&self, cluster_id: &str, reason: &str) -> anyhow::Result<()> {
        let signer: PrivateKeySigner = self.config.private_key.parse()?;
        let wallet = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new()
            .network::<Ethereum>()
            .wallet(wallet)
            .connect_http(self.config.rpc_url.parse::<url::Url>()?);

        let contract = XavierDAO::new(self.config.contract_address, provider);

        let cluster_id_bytes = self.format_cluster_id(cluster_id);

        let tx = contract.vetoProposal(cluster_id_bytes.into(), reason.to_string());
        let _receipt = tx.send().await?;
        Ok(())
    }

    /// Community overrules a veto on-chain.
    #[cfg(feature = "dao-evm")]
    pub async fn overrule(&self, cluster_id: &str) -> anyhow::Result<()> {
        let signer: PrivateKeySigner = self.config.private_key.parse()?;
        let wallet = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new()
            .network::<Ethereum>()
            .wallet(wallet)
            .connect_http(self.config.rpc_url.parse::<url::Url>()?);

        let contract = XavierDAO::new(self.config.contract_address, provider);

        let cluster_id_bytes = self.format_cluster_id(cluster_id);

        let tx = contract.overruleVeto(cluster_id_bytes.into());
        let _receipt = tx.send().await?;
        Ok(())
    }

    /// Executes an approved proposal on-chain.
    #[cfg(feature = "dao-evm")]
    pub async fn execute(&self, cluster_id: &str) -> anyhow::Result<()> {
        let signer: PrivateKeySigner = self.config.private_key.parse()?;
        let wallet = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new()
            .network::<Ethereum>()
            .wallet(wallet)
            .connect_http(self.config.rpc_url.parse::<url::Url>()?);

        let contract = XavierDAO::new(self.config.contract_address, provider);

        let cluster_id_bytes = self.format_cluster_id(cluster_id);

        let tx = contract.executeProposal(cluster_id_bytes.into());
        let _receipt = tx.send().await?;
        Ok(())
    }

    /// Gets the proposal status from on-chain.
    #[cfg(feature = "dao-evm")]
    pub async fn get_proposal_status(
        &self,
        cluster_id: &str,
    ) -> anyhow::Result<(bool, u64, u64, u64, u64, bool, bool)> {
        let provider = ProviderBuilder::new()
            .network::<Ethereum>()
            .connect_http(self.config.rpc_url.parse::<url::Url>()?);

        let contract = XavierDAO::new(self.config.contract_address, provider);

        let cluster_id_bytes = self.format_cluster_id(cluster_id);

        let status = contract
            .getProposalStatus(cluster_id_bytes.into())
            .call()
            .await?;
        Ok((
            status.approved,
            status.userVotesYes.to::<u64>(),
            status.userVotesNo.to::<u64>(),
            status.councilVotesYes.to::<u64>(),
            status.councilVotesNo.to::<u64>(),
            status.vetoed,
            status.executed,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::governance::GovernanceProposal;

    #[cfg(feature = "dao-evm")]
    fn create_test_config(rpc_url: &str) -> EvmDaoConfig {
        EvmDaoConfig {
            rpc_url: rpc_url.to_string(),
            contract_address: Address::ZERO,
            chain_id: 80002,
            private_key: "0xabc123".to_string(),
        }
    }

    #[cfg(not(feature = "dao-evm"))]
    fn create_test_config(rpc_url: &str) -> EvmDaoConfig {
        EvmDaoConfig {
            rpc_url: rpc_url.to_string(),
            contract_address: "0x0000000000000000000000000000000000000000".to_string(),
            chain_id: 80002,
            private_key: "0xabc123".to_string(),
        }
    }

    #[test]
    fn test_evm_dao_config_serialization() {
        let config = create_test_config("https://polygon-amoy.g.allthatnode.com");

        let serialized = serde_json::to_string(&config).unwrap();
        assert!(serialized.contains("rpc_url"));
        assert!(serialized.contains("polygon-amoy"));
        assert!(serialized.contains("chain_id"));
        assert!(serialized.contains("80002"));

        let deserialized: EvmDaoConfig = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.rpc_url, config.rpc_url);
        assert_eq!(deserialized.chain_id, config.chain_id);
    }

    #[test]
    fn test_governance_proposal_serialization() {
        let proposal = GovernanceProposal {
            cluster_id: "cluster-test-123".to_string(),
            title: "Test Proposal".to_string(),
            description: "A test description".to_string(),
            upvotes: 120,
            downvotes: 10,
            is_approved_for_pr: true,
            assigned_maintainer: Some("voter-alpha".to_string()),
        };

        let serialized = serde_json::to_string(&proposal).unwrap();
        assert!(serialized.contains("cluster_id"));
        assert!(serialized.contains("cluster-test-123"));
        assert!(serialized.contains("assigned_maintainer"));
        assert!(serialized.contains("voter-alpha"));

        let deserialized: GovernanceProposal = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.cluster_id, proposal.cluster_id);
        assert_eq!(deserialized.upvotes, proposal.upvotes);
    }

    #[test]
    fn test_evm_dao_config_validation() {
        let invalid_config = EvmDaoConfig {
            rpc_url: "".to_string(),
            #[cfg(feature = "dao-evm")]
            contract_address: Address::ZERO,
            #[cfg(not(feature = "dao-evm"))]
            contract_address: "0x0".to_string(),
            chain_id: 1,
            private_key: "".to_string(),
        };

        let client = OnchainDaoClient::new(invalid_config);
        let result = client.validate_params("CLUSTER_ID_1");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "RPC URL cannot be empty");
    }

    #[test]
    fn test_client_methods_invalid_params() {
        let config = create_test_config("https://localhost:8545");

        let client = OnchainDaoClient::new(config);
        let result = client.validate_params("");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Cluster ID cannot be empty");
    }

    #[test]
    fn test_parse_cluster_id() {
        let config = create_test_config("https://localhost:8545");
        let client = OnchainDaoClient::new(config);

        let formatted = client.format_cluster_id("CLUSTER_SYNC_123");
        let parsed = client.parse_cluster_id(&formatted);
        assert_eq!(parsed, "CLUSTER_SYNC_123");
    }

    #[tokio::test]
    async fn test_sync_from_chain_disabled_fallback() {
        let config = create_test_config("https://localhost:8545");
        let client = OnchainDaoClient::new(config);
        let mut dao = crate::governance::dao::GovernanceDao::new();

        let count = client.sync_from_chain(&mut dao, 0).await.unwrap_or(0);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_format_cluster_id_logic() {
        let config = create_test_config("https://localhost:8545");

        let client = OnchainDaoClient::new(config);

        // Under 32 bytes should be zero-padded
        let short_id = "short";
        let formatted = client.format_cluster_id(short_id);
        assert_eq!(formatted.len(), 32);
        assert_eq!(&formatted[0..5], b"short");
        assert_eq!(formatted[5], 0);

        // Exactly 32 bytes should be preserved
        let exact_id = "12345678901234567890123456789012";
        let formatted_exact = client.format_cluster_id(exact_id);
        assert_eq!(formatted_exact.len(), 32);
        assert_eq!(&formatted_exact[..], exact_id.as_bytes());

        // Over 32 bytes should be truncated
        let long_id = "123456789012345678901234567890123456";
        let formatted_long = client.format_cluster_id(long_id);
        assert_eq!(formatted_long.len(), 32);
        assert_eq!(&formatted_long[..], &long_id.as_bytes()[0..32]);
    }

    #[test]
    fn test_rpc_url_construction_logic() {
        let config = create_test_config("https://localhost:8545/");

        let client = OnchainDaoClient::new(config);

        let url = client.construct_rpc_query_url("/v1/query").unwrap();
        assert_eq!(url, "https://localhost:8545/v1/query");

        let url_no_slash = client.construct_rpc_query_url("v1/query").unwrap();
        assert_eq!(url_no_slash, "https://localhost:8545/v1/query");

        let empty_config = EvmDaoConfig {
            rpc_url: "".to_string(),
            #[cfg(feature = "dao-evm")]
            contract_address: Address::ZERO,
            #[cfg(not(feature = "dao-evm"))]
            contract_address: "0x0000000000000000000000000000000000000000".to_string(),
            chain_id: 1,
            private_key: "0xabc".to_string(),
        };
        let client_empty = OnchainDaoClient::new(empty_config);
        assert!(client_empty.construct_rpc_query_url("v1").is_err());
    }
}
