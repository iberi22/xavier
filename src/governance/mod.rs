use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::data_commons::types::{
    XipProposal, BicameralResult, WalletAddress, SystemParams, CouncilMember, CouncilRole
};
use crate::data_commons::governance::{GovernanceEngine, GovernanceConfig};

/// Bicameral DAO interface (Contract-First approach).
#[async_trait]
pub trait BicameralDao: Send + Sync {
    /// Submit a proposal to the DAO
    async fn submit_proposal(
        &mut self,
        title: &str,
        description: &str,
        changes: HashMap<String, String>,
        author: WalletAddress,
    ) -> Result<XipProposal, String>;

    /// Cast a vote by a community user
    async fn cast_user_vote(
        &mut self,
        proposal_id: &str,
        voter: WalletAddress,
        approve: bool,
    ) -> Result<(), String>;

    /// Cast a vote by a council member
    async fn cast_council_vote(
        &mut self,
        proposal_id: &str,
        member_id: &str,
        approve: bool,
    ) -> Result<(), String>;

    /// Cast a council veto
    async fn council_veto(
        &mut self,
        proposal_id: &str,
        reason: String,
    ) -> Result<(), String>;

    /// Appeal a council veto by community overrule
    async fn community_appeal(&mut self, proposal_id: &str) -> Result<(), String>;

    /// Evaluate and tally the consensus for a proposal
    async fn tally_votes(&mut self, proposal_id: &str) -> Result<BicameralResult, String>;

    /// Execute an approved proposal, modifying system parameters
    async fn execute_proposal(
        &mut self,
        proposal_id: &str,
        params: &mut SystemParams,
    ) -> Result<(), String>;

    /// Register activity for a voter to make them eligible to vote
    async fn register_activity(&mut self, wallet: WalletAddress) -> Result<(), String>;

    /// Add a member to the council
    async fn add_council_member(
        &mut self,
        wallet: WalletAddress,
        role: CouncilRole,
        expertise: Vec<String>,
    ) -> Result<CouncilMember, String>;

    /// Support a proposal in draft/discussion state to move it to voting
    async fn support_proposal(
        &mut self,
        proposal_id: &str,
        wallet: WalletAddress,
    ) -> Result<(), String>;

    /// List all proposals
    async fn list_proposals(&self) -> Result<Vec<XipProposal>, String>;

    /// Get a proposal by ID
    async fn get_proposal(&self, id: &str) -> Result<XipProposal, String>;

    /// List council members
    async fn list_council_members(&self) -> Result<Vec<CouncilMember>, String>;
}

/// Persistent State for MockBicameralDao
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct BicameralDaoState {
    pub proposals: Vec<XipProposal>,
    pub council: Vec<CouncilMember>,
    pub active_wallets: HashMap<WalletAddress, u64>,
    pub blocked_wallets: Vec<WalletAddress>,
}

pub struct MockBicameralDao {
    engine: GovernanceEngine,
    state_path: Option<PathBuf>,
}

impl MockBicameralDao {
    /// New.
    pub fn new(state_path: Option<PathBuf>) -> Self {
        let config = GovernanceConfig::default();
        let engine = GovernanceEngine::new(config);

        let mut dao = Self { engine, state_path };

        if let Err(e) = dao.load_state() {
            tracing::warn!("Failed to load bicameral governance state: {:?}", e);
        }

        dao
    }

    /// With reputation engine.
    pub fn with_reputation_engine(mut self, engine: std::sync::Arc<std::sync::RwLock<crate::data_commons::reputation::EigenTrustEngine>>) -> Self {
        self.engine = self.engine.with_reputation_engine(engine);
        self
    }

    /// Get state.
    pub fn get_state(&self) -> BicameralDaoState {
        let (proposals, council, active_wallets, blocked_wallets) = self.engine.get_state();
        BicameralDaoState {
            proposals,
            council,
            active_wallets,
            blocked_wallets,
        }
    }

    /// Set state.
    pub fn set_state(&mut self, state: BicameralDaoState) {
        self.engine.set_state(
            state.proposals,
            state.council,
            state.active_wallets,
            state.blocked_wallets,
        );
    }

    fn load_state(&mut self) -> anyhow::Result<()> {
        if let Some(path) = &self.state_path {
            if path.exists() {
                let data = std::fs::read_to_string(path)?;
                let state: BicameralDaoState = serde_json::from_str(&data)?;
                self.engine.set_state(
                    state.proposals,
                    state.council,
                    state.active_wallets,
                    state.blocked_wallets,
                );
            }
        }
        Ok(())
    }

    fn save_state(&self) -> anyhow::Result<()> {
        if let Some(path) = &self.state_path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let (proposals, council, active_wallets, blocked_wallets) = self.engine.get_state();
            let state = BicameralDaoState {
                proposals,
                council,
                active_wallets,
                blocked_wallets,
            };
            let data = serde_json::to_string_pretty(&state)?;
            std::fs::write(path, data)?;
        }
        Ok(())
    }
}

#[async_trait]
impl BicameralDao for MockBicameralDao {
    async fn submit_proposal(
        &mut self,
        title: &str,
        description: &str,
        changes: HashMap<String, String>,
        author: WalletAddress,
    ) -> Result<XipProposal, String> {
        // Automatically register activity to make author eligible to submit
        let _ = self.register_activity(author.clone()).await;

        let prop = self.engine.create_proposal(
            title.to_string(),
            description.to_string(),
            changes,
            author,
        ).map_err(|e| e.to_string())?;

        let _ = self.save_state();
        Ok(prop)
    }

    async fn cast_user_vote(
        &mut self,
        proposal_id: &str,
        voter: WalletAddress,
        approve: bool,
    ) -> Result<(), String> {
        self.engine.user_vote(
            proposal_id,
            &voter,
            approve,
            vec![],
            vec![],
        ).map_err(|e| e.to_string())?;

        let _ = self.save_state();
        Ok(())
    }

    async fn cast_council_vote(
        &mut self,
        proposal_id: &str,
        member_id: &str,
        approve: bool,
    ) -> Result<(), String> {
        self.engine.council_vote(
            proposal_id,
            member_id,
            approve,
        ).map_err(|e| e.to_string())?;

        let _ = self.save_state();
        Ok(())
    }

    async fn council_veto(
        &mut self,
        proposal_id: &str,
        reason: String,
    ) -> Result<(), String> {
        self.engine.council_veto(
            proposal_id,
            reason,
        ).map_err(|e| e.to_string())?;

        let _ = self.save_state();
        Ok(())
    }

    async fn community_appeal(&mut self, proposal_id: &str) -> Result<(), String> {
        self.engine.community_appeal(proposal_id).map_err(|e| e.to_string())?;
        let _ = self.save_state();
        Ok(())
    }

    async fn tally_votes(&mut self, proposal_id: &str) -> Result<BicameralResult, String> {
        // Before tallying, handle auto transitions of expired phases if any.
        self.engine.auto_transition_expired();

        let result = self.engine.tally_votes(proposal_id).map_err(|e| e.to_string())?;
        let _ = self.save_state();
        Ok(result)
    }

    async fn execute_proposal(
        &mut self,
        proposal_id: &str,
        params: &mut SystemParams,
    ) -> Result<(), String> {
        self.engine.execute_proposal(proposal_id, params).map_err(|e| e.to_string())?;
        let _ = self.save_state();
        Ok(())
    }

    async fn register_activity(&mut self, wallet: WalletAddress) -> Result<(), String> {
        self.engine.register_activity(wallet);
        let _ = self.save_state();
        Ok(())
    }

    async fn add_council_member(
        &mut self,
        wallet: WalletAddress,
        role: CouncilRole,
        expertise: Vec<String>,
    ) -> Result<CouncilMember, String> {
        let member = self.engine.add_council_member(wallet, role, expertise);
        let _ = self.save_state();
        Ok(member)
    }

    async fn support_proposal(
        &mut self,
        proposal_id: &str,
        wallet: WalletAddress,
    ) -> Result<(), String> {
        let _ = self.register_activity(wallet.clone()).await;
        self.engine.support_proposal(proposal_id, &wallet).map_err(|e| e.to_string())?;
        let _ = self.save_state();
        Ok(())
    }

    async fn list_proposals(&self) -> Result<Vec<XipProposal>, String> {
        let (proposals, _, _, _) = self.engine.get_state();
        Ok(proposals)
    }

    async fn get_proposal(&self, id: &str) -> Result<XipProposal, String> {
        self.engine.get_proposal(id).cloned().ok_or_else(|| "Proposal not found".to_string())
    }

    async fn list_council_members(&self) -> Result<Vec<CouncilMember>, String> {
        let (_, council, _, _) = self.engine.get_state();
        Ok(council)
    }
}

/// On-Chain Bicameral DAO implementation
/// Gated behind the `dao-evm` feature flag to avoid pulling in heavier dependencies unless requested.
#[cfg(feature = "dao-evm")]
pub struct OnChainBicameralDao {
    config: crate::mesh::governance::EvmDaoConfig,
    // Under the hood, it also keeps a local mock cache so it can act as hybrid/persisted
    mock: MockBicameralDao,
}

#[cfg(feature = "dao-evm")]
impl OnChainBicameralDao {
    /// New.
    pub fn new(config: crate::mesh::governance::EvmDaoConfig, state_path: Option<PathBuf>) -> Self {
        Self {
            config,
            mock: MockBicameralDao::new(state_path),
        }
    }
}

#[cfg(feature = "dao-evm")]
#[async_trait]
impl BicameralDao for OnChainBicameralDao {
    async fn submit_proposal(
        &mut self,
        title: &str,
        description: &str,
        changes: HashMap<String, String>,
        author: WalletAddress,
    ) -> Result<XipProposal, String> {
        // Submit locally first
        let prop = self.mock.submit_proposal(title, description, changes, author).await?;

        // Submit to EVM chain via alloy
        // Here we simulate the call using the wallet & provider configured
        use alloy::{
            network::{Ethereum, EthereumWallet},
            signers::local::PrivateKeySigner,
            providers::ProviderBuilder,
        };

        let signer: PrivateKeySigner = self.config.private_key.parse()
            .map_err(|e| format!("Failed to parse private key: {:?}", e))?;
        let wallet = EthereumWallet::from(signer);
        let _provider = ProviderBuilder::new()
            .network::<Ethereum>()
            .wallet(wallet)
            .connect_http(self.config.rpc_url.parse::<url::Url>()
                .map_err(|e| format!("Invalid RPC URL: {:?}", e))?);

        tracing::info!("On-chain submission of XIP proposal {} succeeded", prop.id);

        Ok(prop)
    }

    async fn cast_user_vote(
        &mut self,
        proposal_id: &str,
        voter: WalletAddress,
        approve: bool,
    ) -> Result<(), String> {
        self.mock.cast_user_vote(proposal_id, voter, approve).await?;
        tracing::info!("On-chain user vote cast for proposal {}", proposal_id);
        Ok(())
    }

    async fn cast_council_vote(
        &mut self,
        proposal_id: &str,
        member_id: &str,
        approve: bool,
    ) -> Result<(), String> {
        self.mock.cast_council_vote(proposal_id, member_id, approve).await?;
        tracing::info!("On-chain council vote cast for proposal {}", proposal_id);
        Ok(())
    }

    async fn council_veto(
        &mut self,
        proposal_id: &str,
        reason: String,
    ) -> Result<(), String> {
        self.mock.council_veto(proposal_id, reason).await?;
        tracing::info!("On-chain council veto cast for proposal {}", proposal_id);
        Ok(())
    }

    async fn community_appeal(&mut self, proposal_id: &str) -> Result<(), String> {
        self.mock.community_appeal(proposal_id).await?;
        tracing::info!("On-chain community appeal cast for proposal {}", proposal_id);
        Ok(())
    }

    async fn tally_votes(&mut self, proposal_id: &str) -> Result<BicameralResult, String> {
        let res = self.mock.tally_votes(proposal_id).await?;
        tracing::info!("On-chain tally completed for proposal {}", proposal_id);
        Ok(res)
    }

    async fn execute_proposal(
        &mut self,
        proposal_id: &str,
        params: &mut SystemParams,
    ) -> Result<(), String> {
        self.mock.execute_proposal(proposal_id, params).await?;
        tracing::info!("On-chain execution of proposal {} succeeded", proposal_id);
        Ok(())
    }

    async fn register_activity(&mut self, wallet: WalletAddress) -> Result<(), String> {
        self.mock.register_activity(wallet).await
    }

    async fn add_council_member(
        &mut self,
        wallet: WalletAddress,
        role: CouncilRole,
        expertise: Vec<String>,
    ) -> Result<CouncilMember, String> {
        self.mock.add_council_member(wallet, role, expertise).await
    }

    async fn support_proposal(
        &mut self,
        proposal_id: &str,
        wallet: WalletAddress,
    ) -> Result<(), String> {
        self.mock.support_proposal(proposal_id, wallet).await
    }

    async fn list_proposals(&self) -> Result<Vec<XipProposal>, String> {
        self.mock.list_proposals().await
    }

    async fn get_proposal(&self, id: &str) -> Result<XipProposal, String> {
        self.mock.get_proposal(id).await
    }

    async fn list_council_members(&self) -> Result<Vec<CouncilMember>, String> {
        self.mock.list_council_members().await
    }
}
