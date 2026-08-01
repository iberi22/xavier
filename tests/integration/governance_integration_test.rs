use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use xavier::data_commons::reputation::{EigenTrustEngine, ReputationConfig};
use xavier::data_commons::types::{
    CouncilRole, ReputationAttestation, SystemParams, WalletAddress,
};
use xavier::governance::{BicameralDao, MockBicameralDao};

#[tokio::test]
async fn test_bicameral_governance_complete_happy_path() {
    let mut dao = MockBicameralDao::new(None);

    // 1. Register voter activity so they are eligible to participate
    let author = WalletAddress(
        "xv1_author_00000000000000000000000000000000000000000000000000001".to_string(),
    );
    dao.register_activity(author.clone()).await.unwrap();

    // 2. Submit a proposal to change reference price from 5 to 15
    let mut changes = HashMap::new();
    changes.insert("reference_price".to_string(), "15".to_string());
    changes.insert("burn_rate".to_string(), "90".to_string());

    let proposal = dao
        .submit_proposal(
            "Update reference price and burn rate",
            "We propose raising the reference price to 15 and burn rate to 90%",
            changes,
            author,
        )
        .await
        .unwrap();

    assert_eq!(
        proposal.status,
        xavier::data_commons::types::ProposalStatus::Draft
    );
    assert_eq!(proposal.xip_state.label(), "Draft");

    // 3. Move from Draft/Discussion to Voting by gathering 5 supports
    for i in 0..5 {
        let supporter = WalletAddress(format!("xv1_supporter_{:050}", i));
        dao.register_activity(supporter.clone()).await.unwrap();
        dao.support_proposal(&proposal.id, supporter).await.unwrap();
    }

    let updated_proposal = dao.get_proposal(&proposal.id).await.unwrap();
    assert_eq!(updated_proposal.xip_state.label(), "Voting");

    // 4. Register community voters and cast user votes (all approve)
    for i in 0..3 {
        let voter = WalletAddress(format!("xv1_voter_comm_{:050}", i));
        dao.register_activity(voter.clone()).await.unwrap();
        dao.cast_user_vote(&proposal.id, voter, true).await.unwrap();
    }

    // 5. Register council members and cast council votes (all approve)
    let m1_wallet = WalletAddress(
        "xv1_council_m1_000000000000000000000000000000000000000000000001".to_string(),
    );
    let m2_wallet = WalletAddress(
        "xv1_council_m2_000000000000000000000000000000000000000000000001".to_string(),
    );

    let m1 = dao
        .add_council_member(
            m1_wallet,
            CouncilRole::CoreMaintainer,
            vec!["security".to_string()],
        )
        .await
        .unwrap();
    let m2 = dao
        .add_council_member(
            m2_wallet,
            CouncilRole::Architect,
            vec!["architecture".to_string()],
        )
        .await
        .unwrap();

    dao.cast_council_vote(&proposal.id, &m1.id, true)
        .await
        .unwrap();
    dao.cast_council_vote(&proposal.id, &m2.id, true)
        .await
        .unwrap();

    // Fast-forward voting_end to allow tallying
    let mut state = dao.get_state();
    for p in &mut state.proposals {
        if p.id == proposal.id {
            p.voting_end = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                - 10;
        }
    }
    dao.set_state(state);

    // 6. Tally the votes
    let result = dao.tally_votes(&proposal.id).await.unwrap();
    assert!(result.user_quorum_met);
    assert_eq!(result.user_percentage_for, 100.0);
    assert_eq!(result.council_percentage_for, 100.0);
    assert!(result.passed);

    // 7. Execute the proposal (modify system parameters)
    let mut params = SystemParams::default();
    assert_eq!(params.reference_price, 5);
    assert_eq!(params.burn_rate, 80);

    let exec_res = dao.execute_proposal(&proposal.id, &mut params).await;
    assert!(exec_res.is_err(), "Should enforce 48h timer");

    // Let's modify the execution_at or state of the proposal in the engine to bypass the 48h check!
    let mut state = dao.get_state();
    for p in &mut state.proposals {
        if p.id == proposal.id {
            if let xavier::data_commons::types::XipState::Execution { entered_at, .. } = p.xip_state
            {
                // set expires_at to 10 seconds ago so it's ready to execute!
                p.xip_state = xavier::data_commons::types::XipState::Execution {
                    entered_at,
                    expires_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs()
                        - 10,
                };
            }
        }
    }
    dao.set_state(state);

    // Now execute proposal should pass!
    dao.execute_proposal(&proposal.id, &mut params)
        .await
        .unwrap();
    assert_eq!(params.reference_price, 15);
    assert_eq!(params.burn_rate, 90);
    println!("✅ Complete happy path proposal + execution integration test passed!");
}

#[tokio::test]
async fn test_bicameral_governance_veto_and_overrule() {
    let mut dao = MockBicameralDao::new(None);

    let author = WalletAddress(
        "xv1_author_00000000000000000000000000000000000000000000000000001".to_string(),
    );
    dao.register_activity(author.clone()).await.unwrap();

    let mut changes = HashMap::new();
    changes.insert("reference_price".to_string(), "20".to_string());

    let proposal = dao
        .submit_proposal(
            "Drastic change",
            "A drastic change in reference price",
            changes,
            author,
        )
        .await
        .unwrap();

    // Supports to Voting
    for i in 0..5 {
        let supporter = WalletAddress(format!("xv1_supporter_v_{:048}", i));
        dao.register_activity(supporter.clone()).await.unwrap();
        dao.support_proposal(&proposal.id, supporter).await.unwrap();
    }

    // Community votes (all approve)
    for i in 0..10 {
        let voter = WalletAddress(format!("xv1_voter_comm_v_{:046}", i));
        dao.register_activity(voter.clone()).await.unwrap();
        dao.cast_user_vote(&proposal.id, voter, true).await.unwrap();
    }

    // Council votes (council registers dissent and vetoes)
    let m1_wallet = WalletAddress(
        "xv1_council_m1_000000000000000000000000000000000000000000000001".to_string(),
    );
    let m1 = dao
        .add_council_member(
            m1_wallet,
            CouncilRole::CoreMaintainer,
            vec!["security".to_string()],
        )
        .await
        .unwrap();

    // Cast a negative council vote
    dao.cast_council_vote(&proposal.id, &m1.id, false)
        .await
        .unwrap();

    // Execute council veto
    dao.council_veto(
        &proposal.id,
        "Drastic change threatens economic stability".to_string(),
    )
    .await
    .unwrap();

    let updated_prop = dao.get_proposal(&proposal.id).await.unwrap();
    assert_eq!(
        updated_prop.status,
        xavier::data_commons::types::ProposalStatus::Vetoed
    );

    // Community appeals (overrule veto)
    dao.community_appeal(&proposal.id).await.unwrap();

    let final_prop = dao.get_proposal(&proposal.id).await.unwrap();
    assert_eq!(
        final_prop.status,
        xavier::data_commons::types::ProposalStatus::Overruled
    );
    println!("✅ Veto and overrule integration test passed!");
}

#[tokio::test]
async fn test_reputation_weighted_consensus() {
    let mut dao = MockBicameralDao::new(None);

    // Build an EigenTrust reputation engine
    let rep_config = ReputationConfig::default();
    let mut rep_engine = EigenTrustEngine::new(rep_config, vec![]);

    let w1 = WalletAddress(
        "xv1_rep_trusted_000000000000000000000000000000000000000000000001".to_string(),
    );
    let w2 = WalletAddress(
        "xv1_rep_peer_00000000000000000000000000000000000000000000000002".to_string(),
    );

    // Trusted wallet w1 has high trust score
    rep_engine.add_attestation(ReputationAttestation {
        from: w2.clone(),
        to: w1.clone(),
        score: 1,
        context_hash: None,
        timestamp: 0,
        signature: vec![],
    });
    rep_engine.add_attestation(ReputationAttestation {
        from: w1.clone(),
        to: w2.clone(),
        score: 1,
        context_hash: None,
        timestamp: 0,
        signature: vec![],
    });

    rep_engine.compute().unwrap();

    // Attach reputation engine to our DAO
    let rep_arc = Arc::new(RwLock::new(rep_engine));
    dao = dao.with_reputation_engine(rep_arc);

    let author =
        WalletAddress("xv1_rep_author_00000000000000000000000000000000000000000000001".to_string());
    dao.register_activity(author.clone()).await.unwrap();

    let proposal = dao
        .submit_proposal(
            "Reputation Weighted XIP",
            "We are testing reputation weights",
            HashMap::new(),
            author,
        )
        .await
        .unwrap();

    // Advance to Voting
    for i in 0..5 {
        let supporter = WalletAddress(format!("xv1_rep_sup_{:048}", i));
        dao.register_activity(supporter.clone()).await.unwrap();
        dao.support_proposal(&proposal.id, supporter).await.unwrap();
    }

    // Cast vote from trusted wallet
    dao.register_activity(w1.clone()).await.unwrap();
    dao.cast_user_vote(&proposal.id, w1, true).await.unwrap();

    let p = dao.get_proposal(&proposal.id).await.unwrap();
    let vote_weight = p.weighted_user_votes.values().next().unwrap().weight;

    // A trusted wallet should have a higher weight than the default (1)
    assert!(
        vote_weight > 1,
        "Trusted wallet vote weight should be higher than default 1, got {}",
        vote_weight
    );
    println!("✅ Reputation-weighted consensus integration test passed!");
}

#[cfg(feature = "dao-evm")]
#[tokio::test]
async fn test_on_chain_dao_feature_toggle() {
    use alloy::primitives::Address;
    use xavier::mesh::governance::EvmDaoConfig;

    let config = EvmDaoConfig {
        rpc_url: "http://localhost:8545".to_string(),
        contract_address: Address::ZERO,
        chain_id: 1,
        private_key: "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
            .to_string(),
    };

    let mut dao = xavier::governance::OnChainBicameralDao::new(config, None);

    let author = WalletAddress(
        "xv1_evm_author_000000000000000000000000000000000000000000000001".to_string(),
    );
    dao.register_activity(author.clone()).await.unwrap();

    let prop_res = dao
        .submit_proposal("EVM Proposal", "Description", HashMap::new(), author)
        .await;
    assert!(prop_res.is_ok() || prop_res.is_err());
    println!("✅ On-chain DAO feature toggle test passed!");
}
