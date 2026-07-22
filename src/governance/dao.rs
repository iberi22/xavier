use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ProposalStatus {
    Active,
    Executed,
    Canceled,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Proposal {
    pub id: String,
    pub title: String,
    pub description: String,
    pub options: Vec<String>,
    pub deadline: u64, // Epoch timestamp in seconds
    pub creator: String,
    pub status: ProposalStatus,
    pub votes: HashMap<String, (String, u64)>, // voter -> (option, credits)
}

pub struct GovernanceDao {
    pub proposals: HashMap<String, Proposal>,
    pub current_user: String,
    next_id: u64,
    mock_time: Option<u64>,
}

impl Default for GovernanceDao {
    fn default() -> Self {
        Self::new()
    }
}

impl GovernanceDao {
    /// Creates a new instance of the Governance DAO.
    pub fn new() -> Self {
        Self {
            proposals: HashMap::new(),
            current_user: "creator_1".to_string(),
            next_id: 1,
            mock_time: None,
        }
    }

    /// Builder method to set the active/current user context.
    pub fn with_user(mut self, user: &str) -> Self {
        self.current_user = user.to_string();
        self
    }

    /// Sets the current user context.
    pub fn set_user(&mut self, user: &str) {
        self.current_user = user.to_string();
    }

    /// Overrides the system time with a mock timestamp for testing purposes.
    pub fn set_mock_time(&mut self, time: u64) {
        self.mock_time = Some(time);
    }

    /// Clears the mocked timestamp fallback.
    pub fn clear_mock_time(&mut self) {
        self.mock_time = None;
    }

    /// Retrieves current epoch time (supporting mocks).
    fn get_now(&self) -> u64 {
        if let Some(t) = self.mock_time {
            t
        } else {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
        }
    }

    /// Creates a new proposal using the default/current active user as creator.
    pub fn create_proposal(
        &mut self,
        title: String,
        description: String,
        options: Vec<String>,
        deadline: u64,
    ) -> Result<String, String> {
        let creator = self.current_user.clone();
        self.create_proposal_by(title, description, options, deadline, creator)
    }

    /// Creates a new proposal with a specified creator.
    pub fn create_proposal_by(
        &mut self,
        title: String,
        description: String,
        options: Vec<String>,
        deadline: u64,
        creator: String,
    ) -> Result<String, String> {
        if options.is_empty() {
            return Err("Options list cannot be empty".to_string());
        }

        let now = self.get_now();
        if deadline <= now {
            return Err("Deadline must be in the future".to_string());
        }

        let proposal_id = format!("prop_{}", self.next_id);
        self.next_id += 1;

        let proposal = Proposal {
            id: proposal_id.clone(),
            title,
            description,
            options,
            deadline,
            creator,
            status: ProposalStatus::Active,
            votes: HashMap::new(),
        };

        self.proposals.insert(proposal_id.clone(), proposal);
        Ok(proposal_id)
    }

    /// Casts a vote on an active proposal.
    pub fn vote(
        &mut self,
        proposal_id: &str,
        voter: String,
        option: String,
        credits: u64,
    ) -> Result<(), String> {
        let now = self.get_now();

        let proposal = self
            .proposals
            .get_mut(proposal_id)
            .ok_or_else(|| "Proposal not found".to_string())?;

        if proposal.status != ProposalStatus::Active {
            return Err("Proposal is not active".to_string());
        }

        if now >= proposal.deadline {
            return Err("Voting deadline has passed".to_string());
        }

        if !proposal.options.contains(&option) {
            return Err("Invalid voting option".to_string());
        }

        if credits == 0 {
            return Err("Credits must be greater than zero".to_string());
        }

        if proposal.votes.contains_key(&voter) {
            return Err("Double vote rejected".to_string());
        }

        proposal.votes.insert(voter, (option, credits));
        Ok(())
    }

    /// Executes a proposal after its deadline has passed.
    pub fn execute_proposal(&mut self, proposal_id: &str) -> Result<(), String> {
        let now = self.get_now();

        let proposal = self
            .proposals
            .get_mut(proposal_id)
            .ok_or_else(|| "Proposal not found".to_string())?;

        if proposal.status != ProposalStatus::Active {
            return Err("Proposal is not active".to_string());
        }

        if now < proposal.deadline {
            return Err("Cannot execute proposal before deadline".to_string());
        }

        proposal.status = ProposalStatus::Executed;
        Ok(())
    }

    /// Cancels a proposal before the deadline by the default/current user.
    pub fn cancel_proposal(&mut self, proposal_id: &str) -> Result<(), String> {
        let current_user = self.current_user.clone();
        self.cancel_proposal_by(proposal_id, &current_user)
    }

    /// Cancels a proposal before the deadline by a specified caller.
    pub fn cancel_proposal_by(&mut self, proposal_id: &str, caller: &str) -> Result<(), String> {
        let now = self.get_now();

        let proposal = self
            .proposals
            .get_mut(proposal_id)
            .ok_or_else(|| "Proposal not found".to_string())?;

        if proposal.status != ProposalStatus::Active {
            return Err("Proposal is not active".to_string());
        }

        if caller != proposal.creator {
            return Err("Only the creator can cancel this proposal".to_string());
        }

        if now >= proposal.deadline {
            return Err("Cannot cancel after deadline".to_string());
        }

        proposal.status = ProposalStatus::Canceled;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proposal_full_lifecycle() {
        let mut dao = GovernanceDao::new();
        let base_time = 1700000000;
        dao.set_mock_time(base_time);

        let title = "Upgrade Core Protocol".to_string();
        let description = "Proposal to upgrade consensus rules".to_string();
        let options = vec!["Approve".to_string(), "Reject".to_string()];
        let deadline = base_time + 86400; // 1 day in the future

        // 1. Create Proposal
        let proposal_id = dao
            .create_proposal(title, description, options, deadline)
            .unwrap();

        // 2. Vote
        dao.vote(&proposal_id, "voter_1".to_string(), "Approve".to_string(), 10)
            .unwrap();
        dao.vote(&proposal_id, "voter_2".to_string(), "Reject".to_string(), 5)
            .unwrap();

        // Check proposal details
        let proposal = dao.proposals.get(&proposal_id).unwrap();
        assert_eq!(proposal.status, ProposalStatus::Active);
        assert_eq!(proposal.votes.len(), 2);

        // 3. Try executing before deadline (should fail)
        let exec_early = dao.execute_proposal(&proposal_id);
        assert!(exec_early.is_err());

        // Move past deadline
        dao.set_mock_time(base_time + 90000);

        // 4. Execute Proposal
        dao.execute_proposal(&proposal_id).unwrap();

        // Check state
        let proposal = dao.proposals.get(&proposal_id).unwrap();
        assert_eq!(proposal.status, ProposalStatus::Executed);
    }

    #[test]
    fn proposal_cannot_execute_before_deadline() {
        let mut dao = GovernanceDao::new();
        let base_time = 1700000000;
        dao.set_mock_time(base_time);

        let title = "Immediate Execution Test".to_string();
        let description = "We should not execute this early".to_string();
        let options = vec!["Yes".to_string(), "No".to_string()];
        let deadline = base_time + 1000;

        let proposal_id = dao
            .create_proposal(title, description, options, deadline)
            .unwrap();

        let result = dao.execute_proposal(&proposal_id);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "Cannot execute proposal before deadline".to_string()
        );
    }

    #[test]
    fn double_vote_rejected() {
        let mut dao = GovernanceDao::new();
        let base_time = 1700000000;
        dao.set_mock_time(base_time);

        let title = "Double Vote Test".to_string();
        let description = "Ensure duplicate votes are blocked".to_string();
        let options = vec!["OptionA".to_string(), "OptionB".to_string()];
        let deadline = base_time + 3600;

        let proposal_id = dao
            .create_proposal(title, description, options, deadline)
            .unwrap();

        // First vote succeeds
        dao.vote(&proposal_id, "voter_alice".to_string(), "OptionA".to_string(), 100)
            .unwrap();

        // Second vote from same voter fails
        let second_vote = dao.vote(&proposal_id, "voter_alice".to_string(), "OptionB".to_string(), 50);
        assert!(second_vote.is_err());
        assert_eq!(
            second_vote.unwrap_err(),
            "Double vote rejected".to_string()
        );
    }

    #[test]
    fn cancel_proposal_restrictions() {
        let mut dao = GovernanceDao::new();
        let base_time = 1700000000;
        dao.set_mock_time(base_time);

        let title = "Cancel Test".to_string();
        let description = "Only creator can cancel".to_string();
        let options = vec!["A".to_string(), "B".to_string()];
        let deadline = base_time + 3600;

        dao.set_user("creator_alice");
        let proposal_id = dao
            .create_proposal(title, description, options, deadline)
            .unwrap();

        // Non-creator cannot cancel
        let cancel_wrong = dao.cancel_proposal_by(&proposal_id, "attacker_bob");
        assert!(cancel_wrong.is_err());
        assert_eq!(
            cancel_wrong.unwrap_err(),
            "Only the creator can cancel this proposal".to_string()
        );

        // Creator can cancel
        dao.cancel_proposal(&proposal_id).unwrap();
        let proposal = dao.proposals.get(&proposal_id).unwrap();
        assert_eq!(proposal.status, ProposalStatus::Canceled);
    }
}
