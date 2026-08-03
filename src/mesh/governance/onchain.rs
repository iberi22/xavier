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

#[cfg(feature = "dao-evm")]
#[derive(Debug, Clone)]
pub struct OnchainDaoClient {
    pub config: EvmDaoConfig,
}

#[cfg(feature = "dao-evm")]
impl OnchainDaoClient {
    pub fn new(config: EvmDaoConfig) -> Self {
        Self { config }
    }

    /// Submits a proposal to the XavierDAO contract.
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

        let mut cluster_id_bytes = [0u8; 32];
        let bytes = cluster_id.as_bytes();
        let len = bytes.len().min(32);
        cluster_id_bytes[..len].copy_from_slice(&bytes[..len]);

        let tx = contract.createProposal(
            cluster_id_bytes.into(),
            title.to_string(),
            description.to_string(),
        );
        let _receipt = tx.send().await?;
        Ok(())
    }

    /// Casts a vote on-chain with XP voting power / council flag.
    pub async fn vote(&self, cluster_id: &str, approve: bool, voting_power: u64, is_council: bool) -> anyhow::Result<()> {
        let signer: PrivateKeySigner = self.config.private_key.parse()?;
        let wallet = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new()
            .network::<Ethereum>()
            .wallet(wallet)
            .connect_http(self.config.rpc_url.parse::<url::Url>()?);

        let contract = XavierDAO::new(self.config.contract_address, provider);

        let mut cluster_id_bytes = [0u8; 32];
        let bytes = cluster_id.as_bytes();
        let len = bytes.len().min(32);
        cluster_id_bytes[..len].copy_from_slice(&bytes[..len]);

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
    pub async fn veto(&self, cluster_id: &str, reason: &str) -> anyhow::Result<()> {
        let signer: PrivateKeySigner = self.config.private_key.parse()?;
        let wallet = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new()
            .network::<Ethereum>()
            .wallet(wallet)
            .connect_http(self.config.rpc_url.parse::<url::Url>()?);

        let contract = XavierDAO::new(self.config.contract_address, provider);

        let mut cluster_id_bytes = [0u8; 32];
        let bytes = cluster_id.as_bytes();
        let len = bytes.len().min(32);
        cluster_id_bytes[..len].copy_from_slice(&bytes[..len]);

        let tx = contract.vetoProposal(cluster_id_bytes.into(), reason.to_string());
        let _receipt = tx.send().await?;
        Ok(())
    }

    /// Community overrules a veto on-chain.
    pub async fn overrule(&self, cluster_id: &str) -> anyhow::Result<()> {
        let signer: PrivateKeySigner = self.config.private_key.parse()?;
        let wallet = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new()
            .network::<Ethereum>()
            .wallet(wallet)
            .connect_http(self.config.rpc_url.parse::<url::Url>()?);

        let contract = XavierDAO::new(self.config.contract_address, provider);

        let mut cluster_id_bytes = [0u8; 32];
        let bytes = cluster_id.as_bytes();
        let len = bytes.len().min(32);
        cluster_id_bytes[..len].copy_from_slice(&bytes[..len]);

        let tx = contract.overruleVeto(cluster_id_bytes.into());
        let _receipt = tx.send().await?;
        Ok(())
    }

    /// Executes an approved proposal on-chain.
    pub async fn execute(&self, cluster_id: &str) -> anyhow::Result<()> {
        let signer: PrivateKeySigner = self.config.private_key.parse()?;
        let wallet = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new()
            .network::<Ethereum>()
            .wallet(wallet)
            .connect_http(self.config.rpc_url.parse::<url::Url>()?);

        let contract = XavierDAO::new(self.config.contract_address, provider);

        let mut cluster_id_bytes = [0u8; 32];
        let bytes = cluster_id.as_bytes();
        let len = bytes.len().min(32);
        cluster_id_bytes[..len].copy_from_slice(&bytes[..len]);

        let tx = contract.executeProposal(cluster_id_bytes.into());
        let _receipt = tx.send().await?;
        Ok(())
    }

    /// Gets the proposal status from on-chain.
    pub async fn get_proposal_status(&self, cluster_id: &str) -> anyhow::Result<(bool, u64, u64, u64, u64, bool, bool)> {
        let provider = ProviderBuilder::new()
            .network::<Ethereum>()
            .connect_http(self.config.rpc_url.parse::<url::Url>()?);

        let contract = XavierDAO::new(self.config.contract_address, provider);

        let mut cluster_id_bytes = [0u8; 32];
        let bytes = cluster_id.as_bytes();
        let len = bytes.len().min(32);
        cluster_id_bytes[..len].copy_from_slice(&bytes[..len]);

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
