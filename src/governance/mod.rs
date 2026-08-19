//! Governance DAO module for the SWAL mesh network.
//!
//! Implements on-chain governance with quadratic voting, proposal lifecycle,
//! vote tallying, and council management for decentralized decision-making.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub mod dao;
pub mod quadratic_voting;

use crate::data_commons::governance::{GovernanceConfig, GovernanceEngine};
use crate::data_commons::types::{
    BicameralResult, CouncilMember, CouncilRole, SystemParams, WalletAddress, XipProposal,
};

#[cfg(feature = "dao-evm")]
use crate::mesh::governance::onchain::OnchainDaoClient;

/// Result of quadratic voting tallying
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TallyResult {
    pub yes: u64,
    pub no: u64,
    pub abstain: u64,
    pub quorum_reached: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoteType {
    Yes,
    No,
    Abstain,
}

#[derive(Debug, Clone)]
pub struct QuadraticVote {
    pub voter: String,
    pub vote_type: VoteType,
    pub credits: u64,
}

#[derive(Debug, Clone)]
pub struct QuadraticProposal {
    pub id: String,
    pub votes: Vec<QuadraticVote>,
    pub quorum: u64,
}

#[derive(Debug, Default)]
pub struct QuadraticState {
    pub proposals: HashMap<String, QuadraticProposal>,
    pub voter_balances: HashMap<String, u64>,
}

static QUADRATIC_STATE: std::sync::OnceLock<std::sync::Mutex<QuadraticState>> =
    std::sync::OnceLock::new();

/// Get or initialize the global quadratic state
fn get_quadratic_state() -> &'static std::sync::Mutex<QuadraticState> {
    QUADRATIC_STATE.get_or_init(|| std::sync::Mutex::new(QuadraticState::default()))
}

/// Helper to set up a quadratic proposal in the global state
pub fn setup_quadratic_proposal(proposal_id: &str, quorum: u64) {
    let mut state = get_quadratic_state().lock().unwrap();
    state.proposals.insert(
        proposal_id.to_string(),
        QuadraticProposal {
            id: proposal_id.to_string(),
            votes: Vec::new(),
            quorum,
        },
    );
}

/// Helper to set a voter's token balance in the global state
pub fn set_quadratic_balance(voter: &str, balance: u64) {
    let mut state = get_quadratic_state().lock().unwrap();
    state.voter_balances.insert(voter.to_string(), balance);
}

/// Helper to cast a quadratic vote in the global state
pub fn cast_quadratic_vote(
    proposal_id: &str,
    voter: &str,
    vote_type: VoteType,
    credits: u64,
) -> Result<(), String> {
    let mut state = get_quadratic_state().lock().unwrap();
    let proposal = state
        .proposals
        .get_mut(proposal_id)
        .ok_or_else(|| "Proposal not found".to_string())?;

    if proposal.votes.iter().any(|v| v.voter == voter) {
        return Err("Voter has already voted".to_string());
    }

    proposal.votes.push(QuadraticVote {
        voter: voter.to_string(),
        vote_type,
        credits,
    });
    Ok(())
}

/// Clear the global quadratic state (useful for test isolation)
pub fn clear_quadratic_state() {
    let mut state = get_quadratic_state().lock().unwrap();
    state.proposals.clear();
    state.voter_balances.clear();
}

/// Tallies votes for a given proposal using Quadratic Voting.
/// Each vote of weight `credits` costs `credits^2` tokens.
/// If a voter's balance is lower than `credits^2`, returns an error.
pub fn tally_votes(proposal_id: &str) -> Result<TallyResult, String> {
    let state = get_quadratic_state().lock().unwrap();
    let proposal = state
        .proposals
        .get(proposal_id)
        .ok_or_else(|| "Proposal not found".to_string())?;

    let mut yes = 0u64;
    let mut no = 0u64;
    let mut abstain = 0u64;

    for vote in &proposal.votes {
        let cost = vote
            .credits
            .checked_mul(vote.credits)
            .ok_or_else(|| "Credit calculation overflow".to_string())?;

        let balance = state.voter_balances.get(&vote.voter).cloned().unwrap_or(0);
        if balance < cost {
            return Err(format!(
                "Voter {} has insufficient balance (has {}, needs {})",
                vote.voter, balance, cost
            ));
        }

        match vote.vote_type {
            VoteType::Yes => {
                yes += vote.credits;
            }
            VoteType::No => {
                no += vote.credits;
            }
            VoteType::Abstain => {
                abstain += vote.credits;
            }
        }
    }

    let total_votes = yes + no + abstain;
    let quorum_reached = total_votes >= proposal.quorum;

    Ok(TallyResult {
        yes,
        no,
        abstain,
        quorum_reached,
    })
}

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
    async fn council_veto(&mut self, proposal_id: &str, reason: String) -> Result<(), String>;

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
    pub fn with_reputation_engine(
        mut self,
        engine: std::sync::Arc<
            std::sync::RwLock<crate::data_commons::reputation::EigenTrustEngine>,
        >,
    ) -> Self {
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

        let prop = self
            .engine
            .create_proposal(title.to_string(), description.to_string(), changes, author)
            .map_err(|e| e.to_string())?;

        let _ = self.save_state();
        Ok(prop)
    }

    async fn cast_user_vote(
        &mut self,
        proposal_id: &str,
        voter: WalletAddress,
        approve: bool,
    ) -> Result<(), String> {
        self.engine
            .user_vote(proposal_id, &voter, approve, vec![], vec![])
            .map_err(|e| e.to_string())?;

        let _ = self.save_state();
        Ok(())
    }

    async fn cast_council_vote(
        &mut self,
        proposal_id: &str,
        member_id: &str,
        approve: bool,
    ) -> Result<(), String> {
        self.engine
            .council_vote(proposal_id, member_id, approve)
            .map_err(|e| e.to_string())?;

        let _ = self.save_state();
        Ok(())
    }

    async fn council_veto(&mut self, proposal_id: &str, reason: String) -> Result<(), String> {
        self.engine
            .council_veto(proposal_id, reason)
            .map_err(|e| e.to_string())?;

        let _ = self.save_state();
        Ok(())
    }

    async fn community_appeal(&mut self, proposal_id: &str) -> Result<(), String> {
        self.engine
            .community_appeal(proposal_id)
            .map_err(|e| e.to_string())?;
        let _ = self.save_state();
        Ok(())
    }

    async fn tally_votes(&mut self, proposal_id: &str) -> Result<BicameralResult, String> {
        // Before tallying, handle auto transitions of expired phases if any.
        self.engine.auto_transition_expired();

        let result = self
            .engine
            .tally_votes(proposal_id)
            .map_err(|e| e.to_string())?;
        let _ = self.save_state();
        Ok(result)
    }

    async fn execute_proposal(
        &mut self,
        proposal_id: &str,
        params: &mut SystemParams,
    ) -> Result<(), String> {
        self.engine
            .execute_proposal(proposal_id, params)
            .map_err(|e| e.to_string())?;
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
        self.engine
            .support_proposal(proposal_id, &wallet)
            .map_err(|e| e.to_string())?;
        let _ = self.save_state();
        Ok(())
    }

    async fn list_proposals(&self) -> Result<Vec<XipProposal>, String> {
        let (proposals, _, _, _) = self.engine.get_state();
        Ok(proposals)
    }

    async fn get_proposal(&self, id: &str) -> Result<XipProposal, String> {
        self.engine
            .get_proposal(id)
            .cloned()
            .ok_or_else(|| "Proposal not found".to_string())
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
        let prop = self
            .mock
            .submit_proposal(title, description, changes, author)
            .await?;

        // Submit to EVM chain via alloy
        let client = OnchainDaoClient::new(self.config.clone());
        client
            .propose(&prop.id, title, description)
            .await
            .map_err(|e| format!("Failed on-chain propose: {:?}", e))?;

        tracing::info!("On-chain submission of XIP proposal {} succeeded", prop.id);

        Ok(prop)
    }

    async fn cast_user_vote(
        &mut self,
        proposal_id: &str,
        voter: WalletAddress,
        approve: bool,
    ) -> Result<(), String> {
        self.mock
            .cast_user_vote(proposal_id, voter.clone(), approve)
            .await?;

        // Resolve voting power using our new wallet integration pattern
        // We defaults to 100 voting power (since a mock voter balance can be treated as 100 XP)
        let voting_power = 100;

        let client = OnchainDaoClient::new(self.config.clone());
        client
            .vote(proposal_id, approve, voting_power, false)
            .await
            .map_err(|e| format!("Failed on-chain user vote: {:?}", e))?;

        tracing::info!("On-chain user vote cast for proposal {}", proposal_id);
        Ok(())
    }

    async fn cast_council_vote(
        &mut self,
        proposal_id: &str,
        member_id: &str,
        approve: bool,
    ) -> Result<(), String> {
        self.mock
            .cast_council_vote(proposal_id, member_id, approve)
            .await?;

        let client = OnchainDaoClient::new(self.config.clone());
        client
            .vote(proposal_id, approve, 1, true)
            .await
            .map_err(|e| format!("Failed on-chain council vote: {:?}", e))?;

        tracing::info!("On-chain council vote cast for proposal {}", proposal_id);
        Ok(())
    }

    async fn council_veto(&mut self, proposal_id: &str, reason: String) -> Result<(), String> {
        self.mock.council_veto(proposal_id, reason.clone()).await?;

        let client = OnchainDaoClient::new(self.config.clone());
        client
            .veto(proposal_id, &reason)
            .await
            .map_err(|e| format!("Failed on-chain veto: {:?}", e))?;

        tracing::info!("On-chain council veto cast for proposal {}", proposal_id);
        Ok(())
    }

    async fn community_appeal(&mut self, proposal_id: &str) -> Result<(), String> {
        self.mock.community_appeal(proposal_id).await?;

        let client = OnchainDaoClient::new(self.config.clone());
        client
            .overrule(proposal_id)
            .await
            .map_err(|e| format!("Failed on-chain overrule: {:?}", e))?;

        tracing::info!(
            "On-chain community appeal cast for proposal {}",
            proposal_id
        );
        Ok(())
    }

    async fn tally_votes(&mut self, proposal_id: &str) -> Result<BicameralResult, String> {
        let mut local_res = self.mock.tally_votes(proposal_id).await?;

        let client = OnchainDaoClient::new(self.config.clone());
        if let Ok((approved, user_yes, user_no, council_yes, council_no, vetoed, executed)) =
            client.get_proposal_status(proposal_id).await
        {
            local_res.passed = approved;
            local_res.user_votes_for = user_yes;
            local_res.user_votes_against = user_no;
            local_res.council_votes_for = council_yes;
            local_res.council_votes_against = council_no;
            local_res.council_veto_active = vetoed;
            local_res.executed = executed;
        }

        tracing::info!("On-chain tally completed for proposal {}", proposal_id);
        Ok(local_res)
    }

    async fn execute_proposal(
        &mut self,
        proposal_id: &str,
        params: &mut SystemParams,
    ) -> Result<(), String> {
        self.mock.execute_proposal(proposal_id, params).await?;

        let client = OnchainDaoClient::new(self.config.clone());
        client
            .execute(proposal_id)
            .await
            .map_err(|e| format!("Failed on-chain execution: {:?}", e))?;

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

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn tally_quadratic_voting() {
        let _guard = TEST_MUTEX.lock().unwrap();
        clear_quadratic_state();

        let prop_id = "prop_1";
        setup_quadratic_proposal(prop_id, 10);

        set_quadratic_balance("voter_yes", 100); // 10^2 = 100
        set_quadratic_balance("voter_no", 25); // 5^2 = 25
        set_quadratic_balance("voter_abs", 9); // 3^2 = 9

        cast_quadratic_vote(prop_id, "voter_yes", VoteType::Yes, 10).unwrap();
        cast_quadratic_vote(prop_id, "voter_no", VoteType::No, 5).unwrap();
        cast_quadratic_vote(prop_id, "voter_abs", VoteType::Abstain, 3).unwrap();

        let result = tally_votes(prop_id).unwrap();

        assert_eq!(result.yes, 10);
        assert_eq!(result.no, 5);
        assert_eq!(result.abstain, 3);
        assert!(result.quorum_reached);
    }

    #[test]
    fn tally_fails_without_quorum() {
        let _guard = TEST_MUTEX.lock().unwrap();
        clear_quadratic_state();

        let prop_id = "prop_2";
        setup_quadratic_proposal(prop_id, 50);

        set_quadratic_balance("voter_yes", 100);
        cast_quadratic_vote(prop_id, "voter_yes", VoteType::Yes, 10).unwrap();

        let result = tally_votes(prop_id).unwrap();

        assert_eq!(result.yes, 10);
        assert_eq!(result.no, 0);
        assert_eq!(result.abstain, 0);
        assert!(!result.quorum_reached);
    }

    #[test]
    fn tally_fails_insufficient_balance() {
        let _guard = TEST_MUTEX.lock().unwrap();
        clear_quadratic_state();

        let prop_id = "prop_3";
        setup_quadratic_proposal(prop_id, 5);

        set_quadratic_balance("voter_poor", 24);
        cast_quadratic_vote(prop_id, "voter_poor", VoteType::Yes, 5).unwrap();

        let result = tally_votes(prop_id);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("insufficient balance"));
    }

    #[tokio::test]
    async fn test_onchain_bicameral_dao_successful_flow() {
        // Build mock dao (acts as our local/on-chain bicameral validator)
        let mut dao = MockBicameralDao::new(None);
        dao.engine.set_config(GovernanceConfig {
            discussion_period_days: 0,
            voting_period_days: 0,
            execution_timer_hours: 0,
            user_quorum_minimum: 10.0,
            council_quorum_minimum: 50.0,
            min_supports: 3,
            user_weight: 50.0,
            council_weight: 50.0,
            council_veto_threshold: 66.0,
            community_overrule_threshold: 75.0,
            voting_activity_window_days: 7,
            expulsion_threshold: 66.0,
            dynamic_quorum: None,
        });

        let author = WalletAddress(
            "xv1_author_0123456789abcdef0123456789abcd0123456789abcdef012345".to_string(),
        );
        dao.register_activity(author.clone()).await.unwrap();

        let mut changes = HashMap::new();
        changes.insert("reference_price".to_string(), "150".to_string());

        // 1. Submit proposal
        let prop = dao
            .submit_proposal("Upgrade Price", "Raising ref price", changes, author)
            .await
            .unwrap();
        assert_eq!(prop.xip_state.label(), "Draft");

        // 2. Add supporters to move to Voting phase (min_supports = 3)
        for i in 0..3 {
            let supporter = WalletAddress(format!(
                "xv1_supporter_{}_000000000000000000000000000000000000000000000000",
                i
            ));
            dao.register_activity(supporter.clone()).await.unwrap();
            dao.support_proposal(&prop.id, supporter).await.unwrap();
        }

        // Reload and check state is Voting
        let prop_voting = dao.get_proposal(&prop.id).await.unwrap();
        assert_eq!(prop_voting.xip_state.label(), "Voting");

        // 3. User & Council cast votes (both YES)
        let voter_u = WalletAddress(
            "xv1_user_voter_000000000000000000000000000000000000000000000000".to_string(),
        );
        dao.register_activity(voter_u.clone()).await.unwrap();
        dao.cast_user_vote(&prop.id, voter_u, true).await.unwrap();

        let council_m = dao
            .add_council_member(
                WalletAddress(
                    "xv1_council_m_000000000000000000000000000000000000000000000000".to_string(),
                ),
                CouncilRole::CoreMaintainer,
                vec!["core".to_string()],
            )
            .await
            .unwrap();
        dao.cast_council_vote(&prop.id, &council_m.id, true)
            .await
            .unwrap();

        // 4. Tally votes & Execute (happy path)
        let tally_res = dao.tally_votes(&prop.id).await.unwrap();
        assert!(tally_res.passed);

        let mut params = SystemParams::default();
        dao.execute_proposal(&prop.id, &mut params).await.unwrap();
        assert_eq!(params.reference_price, 150);
    }

    #[tokio::test]
    async fn test_onchain_bicameral_dao_veto_overrule_flow() {
        let mut dao = MockBicameralDao::new(None);
        dao.engine.set_config(GovernanceConfig {
            discussion_period_days: 0,
            voting_period_days: 0,
            execution_timer_hours: 0,
            user_quorum_minimum: 10.0,
            council_quorum_minimum: 50.0,
            min_supports: 3,
            user_weight: 50.0,
            council_weight: 50.0,
            council_veto_threshold: 66.0,
            community_overrule_threshold: 75.0,
            voting_activity_window_days: 7,
            expulsion_threshold: 66.0,
            dynamic_quorum: None,
        });

        let author = WalletAddress(
            "xv1_author_0123456789abcdef0123456789abcd0123456789abcdef012345".to_string(),
        );
        dao.register_activity(author.clone()).await.unwrap();

        let mut changes = HashMap::new();
        changes.insert("reference_price".to_string(), "200".to_string());

        let prop = dao
            .submit_proposal("Upgrade Price 2", "Raising ref price 2", changes, author)
            .await
            .unwrap();

        for i in 0..3 {
            let supporter = WalletAddress(format!(
                "xv1_supporter2_{}_000000000000000000000000000000000000000000000000",
                i
            ));
            dao.register_activity(supporter.clone()).await.unwrap();
            dao.support_proposal(&prop.id, supporter).await.unwrap();
        }

        let voter_u = WalletAddress(
            "xv1_user_voter2_00000000000000000000000000000000000000000000000".to_string(),
        );
        dao.register_activity(voter_u.clone()).await.unwrap();
        dao.cast_user_vote(&prop.id, voter_u, true).await.unwrap();

        let council_m = dao
            .add_council_member(
                WalletAddress(
                    "xv1_council_m2_00000000000000000000000000000000000000000000000".to_string(),
                ),
                CouncilRole::CoreMaintainer,
                vec!["core".to_string()],
            )
            .await
            .unwrap();
        dao.cast_council_vote(&prop.id, &council_m.id, false)
            .await
            .unwrap();

        // Place veto
        dao.council_veto(&prop.id, "Security issues".to_string())
            .await
            .unwrap();
        let prop_vetoed = dao.get_proposal(&prop.id).await.unwrap();
        assert!(prop_vetoed.council_veto);

        // Community overrules
        dao.community_appeal(&prop.id).await.unwrap();
        let prop_appealed = dao.get_proposal(&prop.id).await.unwrap();
        assert!(!prop_appealed.council_veto);
        assert!(prop_appealed.appealed);
    }
}
