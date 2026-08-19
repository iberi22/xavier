use std::collections::HashMap;
use xavier::governance::quadratic_voting::{
    calculate_effective_votes, integer_sqrt, IvnIdentityTier, QuadraticVoteEngine, VoterProfile,
};

#[test]
fn test_basic_quadratic_tally_single_choice() {
    let mut engine = QuadraticVoteEngine::new(50);

    // Register voters
    engine.register_voter(VoterProfile::new("voter_alice", 100, IvnIdentityTier::Verified, 100));
    engine.register_voter(VoterProfile::new("voter_bob", 200, IvnIdentityTier::Validator, 100));

    // Create proposal
    let options = vec!["Option1".to_string(), "Option2".to_string()];
    engine.create_proposal("prop_001", options.clone()).unwrap();

    // Cast votes
    engine.cast_vote("prop_001", "voter_alice", "Option1", 64).unwrap();
    engine.cast_vote("prop_001", "voter_bob", "Option2", 100).unwrap();

    // Tally
    let tally = engine.tally("prop_001").unwrap();

    assert_eq!(tally.proposal_id, "prop_001");
    assert_eq!(tally.total_voters, 2);
    assert_eq!(tally.sybil_votes_rejected, 0);

    // Alice: credits=64, karma=100, Verified (1.0x) -> karma_weight=100 -> product=6400 -> sqrt=80
    assert_eq!(*tally.option_tallies.get("Option1").unwrap(), 80);

    // Bob: credits=100, karma=200, Validator (1.5x) -> karma_weight=300 -> product=30000 -> sqrt=173
    assert_eq!(*tally.option_tallies.get("Option2").unwrap(), 173);

    assert_eq!(tally.winning_option, Some("Option2".to_string()));
    assert!(tally.quorum_reached);
}

#[test]
fn test_multi_choice_ballot_tally() {
    let mut engine = QuadraticVoteEngine::new(100);

    // Register voter
    engine.register_voter(VoterProfile::new("voter_charlie", 400, IvnIdentityTier::Sovereign, 200));

    // Create proposal
    let options = vec!["Build".to_string(), "Audit".to_string(), "Market".to_string()];
    engine.create_proposal("prop_multi", options).unwrap();

    // Multi-choice allocation (Total credits = 64 + 36 + 25 = 125 <= balance of 200)
    let mut allocations = HashMap::new();
    allocations.insert("Build".to_string(), 64);
    allocations.insert("Audit".to_string(), 36);
    allocations.insert("Market".to_string(), 25);

    let effective = engine.cast_multi_choice_ballot("prop_multi", "voter_charlie", allocations).unwrap();

    // Sovereign tier (2.0x = 20000 bps), karma = 400 -> karma_weight = (400 * 20000) / 10000 = 800
    // Build: sqrt(64 * 800) = sqrt(51200) = 226
    // Audit: sqrt(36 * 800) = sqrt(28800) = 169
    // Market: sqrt(25 * 800) = sqrt(20000) = 141
    assert_eq!(*effective.get("Build").unwrap(), 226);
    assert_eq!(*effective.get("Audit").unwrap(), 169);
    assert_eq!(*effective.get("Market").unwrap(), 141);

    let tally = engine.tally("prop_multi").unwrap();
    assert_eq!(tally.winning_option, Some("Build".to_string()));
}

#[test]
fn test_integer_sqrt_weighting_accuracy() {
    assert_eq!(integer_sqrt(0), 0);
    assert_eq!(integer_sqrt(144), 12);
    assert_eq!(integer_sqrt(1000000), 1000);

    // Unverified (0.1x), karma 100, credits 100 -> karma_weight = 10 -> product = 1000 -> sqrt = 31
    assert_eq!(calculate_effective_votes(100, 100, IvnIdentityTier::Unverified, false), 31);

    // Basic (0.5x), karma 100, credits 100 -> karma_weight = 50 -> product = 5000 -> sqrt = 70
    assert_eq!(calculate_effective_votes(100, 100, IvnIdentityTier::Basic, false), 70);

    // Verified (1.0x), karma 100, credits 100 -> karma_weight = 100 -> product = 10000 -> sqrt = 100
    assert_eq!(calculate_effective_votes(100, 100, IvnIdentityTier::Verified, false), 100);

    // Validator (1.5x), karma 100, credits 100 -> karma_weight = 150 -> product = 15000 -> sqrt = 122
    assert_eq!(calculate_effective_votes(100, 100, IvnIdentityTier::Validator, false), 122);

    // Sovereign (2.0x), karma 100, credits 100 -> karma_weight = 200 -> product = 20000 -> sqrt = 141
    assert_eq!(calculate_effective_votes(100, 100, IvnIdentityTier::Sovereign, false), 141);
}

#[test]
fn test_sybil_split_attack_resilience() {
    let mut engine = QuadraticVoteEngine::new(10);

    // Legitimate user: Verified tier, karma 500, credit balance 100
    engine.register_voter(VoterProfile::new("honest_user", 500, IvnIdentityTier::Verified, 100));

    // Attacker creates 5 Sybil accounts: Unverified tier, karma 20 each, credit balance 20 each
    for i in 0..5 {
        let sybil_id = format!("sybil_{}", i);
        engine.register_voter(VoterProfile::new(sybil_id, 20, IvnIdentityTier::Unverified, 20));
    }

    let options = vec!["HonestChoice".to_string(), "SybilChoice".to_string()];
    engine.create_proposal("prop_sybil_test", options).unwrap();

    // Honest user casts 100 credits on HonestChoice
    engine.cast_vote("prop_sybil_test", "honest_user", "HonestChoice", 100).unwrap();

    // 5 Sybils cast 20 credits each on SybilChoice
    for i in 0..5 {
        let sybil_id = format!("sybil_{}", i);
        engine.cast_vote("prop_sybil_test", &sybil_id, "SybilChoice", 20).unwrap();
    }

    let tally = engine.tally("prop_sybil_test").unwrap();

    // Honest: credits=100, karma=500, Verified -> karma_weight=500 -> product=50000 -> sqrt=223
    let honest_votes = *tally.option_tallies.get("HonestChoice").unwrap();
    assert_eq!(honest_votes, 223);

    // Each Sybil: credits=20, karma=20, Unverified (0.1x) -> karma_weight = 2 -> product = 40 -> sqrt = 6
    // 5 Sybils * 6 votes = 30 total votes
    let sybil_total_votes = *tally.option_tallies.get("SybilChoice").unwrap();
    assert_eq!(sybil_total_votes, 30);

    // The honest choice wins by a huge margin despite equal total credits spent
    assert_eq!(tally.winning_option, Some("HonestChoice".to_string()));
    assert!(honest_votes > sybil_total_votes * 7);
}

#[test]
fn test_sybil_flagged_rejection() {
    let mut engine = QuadraticVoteEngine::new(10);

    // Legitimate user
    engine.register_voter(VoterProfile::new("honest_alice", 200, IvnIdentityTier::Verified, 100));

    // Flagged Sybil
    let sybil_profile = VoterProfile::new("sybil_bob", 200, IvnIdentityTier::Verified, 100).with_sybil_flag(true);
    engine.register_voter(sybil_profile);

    let options = vec!["Yes".to_string(), "No".to_string()];
    engine.create_proposal("prop_flag_test", options).unwrap();

    engine.cast_vote("prop_flag_test", "honest_alice", "Yes", 50).unwrap();
    engine.cast_vote("prop_flag_test", "sybil_bob", "No", 50).unwrap();

    let tally = engine.tally("prop_flag_test").unwrap();

    assert_eq!(tally.total_voters, 2);
    assert_eq!(tally.sybil_votes_rejected, 1);
    assert_eq!(*tally.option_tallies.get("Yes").unwrap(), 100);
    assert_eq!(*tally.option_tallies.get("No").unwrap(), 0);
    assert_eq!(tally.winning_option, Some("Yes".to_string()));
}

#[test]
fn test_budget_and_double_voting_constraints() {
    let mut engine = QuadraticVoteEngine::new(10);

    engine.register_voter(VoterProfile::new("voter_dave", 100, IvnIdentityTier::Verified, 50));

    let options = vec!["OptionA".to_string(), "OptionB".to_string()];
    engine.create_proposal("prop_budget", options).unwrap();

    // Attempting to spend 60 credits when balance is 50 fails
    let err_budget = engine.cast_vote("prop_budget", "voter_dave", "OptionA", 60);
    assert!(err_budget.is_err());
    assert!(err_budget.unwrap_err().contains("Insufficient credits"));

    // Valid vote succeeds
    engine.cast_vote("prop_budget", "voter_dave", "OptionA", 40).unwrap();

    // Second vote attempt on same proposal fails
    let err_double = engine.cast_vote("prop_budget", "voter_dave", "OptionB", 10);
    assert!(err_double.is_err());
    assert!(err_double.unwrap_err().contains("already voted"));
}
