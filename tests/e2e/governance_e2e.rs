#![allow(clippy::field_reassign_with_default)]
use std::collections::HashMap;
use xavier::data_commons::{
    governance::{DynamicQuorum, GovernanceConfig, GovernanceEngine},
    types::{CouncilRole, ProposalStatus, SystemParams, WalletAddress},
};

#[test]
fn test_governance_dao_complete_lifecycle_e2e() {
    // 1. Setup the Governance Engine with 0-day discussion/voting period to allow instant voting close
    let config = GovernanceConfig {
        discussion_period_days: 0,
        voting_period_days: 0,
        execution_timer_hours: 0, // 0 to bypass timer check in test
        user_quorum_minimum: 10.0,
        council_quorum_minimum: 50.0,
        min_supports: 3,
        user_weight: 50.0,
        council_weight: 50.0,
        council_veto_threshold: 66.0,
        community_overrule_threshold: 75.0,
        voting_activity_window_days: 7,
        expulsion_threshold: 66.0,
        dynamic_quorum: Some(DynamicQuorum::new(0.10, 0.50)),
    };

    let mut engine = GovernanceEngine::new(config);

    // 2. Register voter and creator activity
    let author = WalletAddress("xv1_author_e2e".to_string());
    engine.register_activity(author.clone());

    let mut supporters = Vec::new();
    for i in 1..=3 {
        let supporter = WalletAddress(format!("xv1_supporter_{}", i));
        engine.register_activity(supporter.clone());
        supporters.push(supporter);
    }

    let mut user_voters = Vec::new();
    for i in 1..=10 {
        let voter = WalletAddress(format!("xv1_voter_{}", i));
        engine.register_activity(voter.clone());
        user_voters.push(voter);
    }

    // 3. Create proposal
    let mut changes = HashMap::new();
    changes.insert("reference_price".to_string(), "100".to_string());
    changes.insert("burn_rate".to_string(), "15".to_string());

    let proposal = engine
        .create_proposal(
            "XIP-2026-01: Update Token Economics".to_string(),
            "Proposal description".to_string(),
            changes,
            author,
        )
        .expect("Failed to create proposal");

    assert_eq!(proposal.status, ProposalStatus::Draft);

    // 4. Support proposal to move to Voting
    for supporter in &supporters {
        engine
            .support_proposal(&proposal.id, supporter)
            .expect("Failed to support proposal");
    }

    let updated_prop = engine
        .get_proposal(&proposal.id)
        .expect("Proposal not found");
    assert_eq!(updated_prop.status, ProposalStatus::Voting);

    // 5. Cast user votes (100% YES)
    for voter in &user_voters {
        engine
            .user_vote(&proposal.id, voter, true, vec![], vec![])
            .expect("Failed to cast user vote");
    }

    // 6. Add council members and cast council votes (100% YES)
    let c1 = engine.add_council_member(
        WalletAddress("xv1_council_1".to_string()),
        CouncilRole::CoreMaintainer,
        vec!["architecture".to_string()],
    );
    engine
        .council_vote(&proposal.id, &c1.id, true)
        .expect("Failed to cast council vote");

    // 7. Tally votes (instantly ends voting since voting_period_days is 0)
    let result = engine
        .tally_votes(&proposal.id)
        .expect("Failed to tally final votes");

    assert!(result.passed);
    assert_eq!(result.user_percentage_for, 100.0);
    assert_eq!(result.council_votes_for, 1);

    // 8. Execute proposal changes on SystemParams
    let mut params = SystemParams::default();
    params.reference_price = 10;
    params.burn_rate = 5;

    engine
        .execute_proposal(&proposal.id, &mut params)
        .expect("Failed to execute approved proposal");

    assert_eq!(params.reference_price, 100);
    assert_eq!(params.burn_rate, 15);

    let final_prop = engine.get_proposal(&proposal.id).unwrap();
    assert_eq!(final_prop.status, ProposalStatus::Executed);
}

#[test]
fn test_dynamic_quorum_adjustments() {
    let dq = DynamicQuorum::new(0.20, 0.60);

    // Low participation (< 30%): lowers quorum by 20%
    assert!((dq.effective_user_quorum(0.10) - 0.16).abs() < 0.001);
    assert!((dq.effective_council_quorum(0.15) - 0.48).abs() < 0.001);

    // High participation (> 80%): raises quorum by 10%
    assert!((dq.effective_user_quorum(0.85) - 0.22).abs() < 0.001);
    assert!((dq.effective_council_quorum(0.90) - 0.66).abs() < 0.001);

    // Medium participation (30% - 80%): keeps base
    assert!((dq.effective_user_quorum(0.50) - 0.20).abs() < 0.001);
    assert!((dq.effective_council_quorum(0.50) - 0.60).abs() < 0.001);
}
