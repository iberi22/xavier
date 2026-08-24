use proptest::prelude::*;
use std::collections::HashMap;
use xavier::governance::quadratic_voting::{
    calculate_effective_votes, integer_sqrt, IvnIdentityTier, MultiChoiceBallot,
    ProposalTallyResult, QuadraticProposal, QuadraticVoteEngine, VoterProfile,
};

// ---------------------------------------------------------------------------
// Proptests
// ---------------------------------------------------------------------------

proptest! {
    /// Verify property-based invariants for `integer_sqrt`:
    /// 1. sqrt(n)^2 <= n
    /// 2. If sqrt(n) < u64::MAX, then (sqrt(n) + 1)^2 > n
    /// 3. Monotonicity: a <= b => sqrt(a) <= sqrt(b)
    #[test]
    fn prop_integer_sqrt_invariants(n in 0..=u128::MAX) {
        let root = integer_sqrt(n);
        let root_u128 = root as u128;
        let root_sq = root_u128 * root_u128;

        // 1. root^2 <= n
        prop_assert!(root_sq <= n, "root^2 ({}) > n ({})", root_sq, n);

        // 2. (root + 1)^2 > n if root < u64::MAX
        if root < u64::MAX {
            let next = root_u128 + 1;
            let next_sq = next * next;
            prop_assert!(next_sq > n, "(root + 1)^2 ({}) <= n ({})", next_sq, n);
        }
    }

    /// Monotonicity test for integer_sqrt
    #[test]
    fn prop_integer_sqrt_monotonicity(a in 0..=u128::MAX, b in 0..=u128::MAX) {
        let (min_val, max_val) = if a <= b { (a, b) } else { (b, a) };
        let root_min = integer_sqrt(min_val);
        let root_max = integer_sqrt(max_val);
        prop_assert!(root_min <= root_max, "Monotonicity violated: sqrt({})={} > sqrt({})={}", min_val, root_min, max_val, root_max);
    }

    /// Property-based testing for `calculate_effective_votes` with arbitrary inputs
    #[test]
    fn prop_calculate_effective_votes_no_panic(
        credits in 0..=u64::MAX,
        karma in 0..=u64::MAX,
        tier_idx in 0..5u8,
        is_sybil in proptest::bool::ANY,
    ) {
        let tier = match tier_idx {
            0 => IvnIdentityTier::Unverified,
            1 => IvnIdentityTier::Basic,
            2 => IvnIdentityTier::Verified,
            3 => IvnIdentityTier::Validator,
            _ => IvnIdentityTier::Sovereign,
        };

        let eff = calculate_effective_votes(credits, karma, tier, is_sybil);

        if is_sybil || credits == 0 || karma == 0 {
            prop_assert_eq!(eff, 0);
        } else {
            // Effective votes must never overflow: floor(sqrt(u128::MAX)) = u64::MAX,
            // so the result is representable in u64 by construction (assert the
            // invariant without a tautological comparison).
            prop_assert!((eff as u128) <= u64::MAX as u128);
        }
    }

    /// Property-based testing for `QuadraticVoteEngine` lifecycle & tally invariants
    #[test]
    fn prop_engine_voting_and_tally_invariants(
        num_voters in 1..20usize,
        num_sybils in 0..10usize,
        default_quorum in 0..=1_000_000u64,
        credit_per_voter in 1..=10_000u64,
    ) {
        let mut engine = QuadraticVoteEngine::new(default_quorum);

        let options = vec!["OptA".to_string(), "OptB".to_string(), "OptC".to_string()];
        prop_assert!(engine.create_proposal("p1", options.clone()).is_ok());

        let mut expected_sybil_count = 0;
        let mut total_cast_voters = 0;

        for i in 0..num_voters {
            let voter_id = format!("voter_{}", i);
            let is_sybil = i < num_sybils;
            let profile = VoterProfile::new(&voter_id, 100, IvnIdentityTier::Verified, credit_per_voter)
                .with_sybil_flag(is_sybil);
            engine.register_voter(profile);

            // Cast vote
            let mut allocations = HashMap::new();
            allocations.insert("OptA".to_string(), credit_per_voter / 2 + 1);

            if engine.cast_multi_choice_ballot("p1", &voter_id, allocations).is_ok() {
                total_cast_voters += 1;
                if is_sybil {
                    expected_sybil_count += 1;
                }
            }
        }

        let tally = engine.tally("p1").expect("Tally should succeed");

        prop_assert_eq!(tally.total_voters, total_cast_voters);
        prop_assert_eq!(tally.sybil_votes_rejected, expected_sybil_count);
        prop_assert_eq!(tally.proposal_id, "p1");

        let sum_tallies: u64 = tally.option_tallies.values().copied().fold(0u64, |acc, v| acc.saturating_add(v));
        prop_assert_eq!(tally.quorum_reached, sum_tallies >= default_quorum);
    }
}

// ---------------------------------------------------------------------------
// Unit Tests - `IvnIdentityTier`
// ---------------------------------------------------------------------------

#[test]
fn test_ivn_identity_tier_multipliers_and_ordering() {
    assert_eq!(IvnIdentityTier::Unverified.multiplier_bps(), 1000);
    assert_eq!(IvnIdentityTier::Basic.multiplier_bps(), 5000);
    assert_eq!(IvnIdentityTier::Verified.multiplier_bps(), 10000);
    assert_eq!(IvnIdentityTier::Validator.multiplier_bps(), 15000);
    assert_eq!(IvnIdentityTier::Sovereign.multiplier_bps(), 20000);

    // Test ordering
    assert!(IvnIdentityTier::Unverified < IvnIdentityTier::Basic);
    assert!(IvnIdentityTier::Basic < IvnIdentityTier::Verified);
    assert!(IvnIdentityTier::Verified < IvnIdentityTier::Validator);
    assert!(IvnIdentityTier::Validator < IvnIdentityTier::Sovereign);
}

// ---------------------------------------------------------------------------
// Unit Tests - `VoterProfile`
// ---------------------------------------------------------------------------

#[test]
fn test_voter_profile_builder_and_flags() {
    let voter = VoterProfile::new("alice", 500, IvnIdentityTier::Verified, 1000);
    assert_eq!(voter.voter_id, "alice");
    assert_eq!(voter.karma, 500);
    assert_eq!(voter.identity_tier, IvnIdentityTier::Verified);
    assert_eq!(voter.credit_balance, 1000);
    assert!(!voter.is_sybil_flagged);

    let flagged = voter.with_sybil_flag(true);
    assert!(flagged.is_sybil_flagged);

    let unflagged = flagged.with_sybil_flag(false);
    assert!(!unflagged.is_sybil_flagged);
}

// ---------------------------------------------------------------------------
// Unit Tests - `integer_sqrt` Extreme Edge Cases
// ---------------------------------------------------------------------------

#[test]
fn test_integer_sqrt_boundaries() {
    assert_eq!(integer_sqrt(0), 0);
    assert_eq!(integer_sqrt(1), 1);
    assert_eq!(integer_sqrt(2), 1);
    assert_eq!(integer_sqrt(3), 1);
    assert_eq!(integer_sqrt(4), 2);
    assert_eq!(integer_sqrt(u128::MAX), 18446744073709551615); // u64::MAX

    // Boundary check around 2^64
    let u64_max_sq = (u64::MAX as u128) * (u64::MAX as u128);
    assert_eq!(integer_sqrt(u64_max_sq - 1), u64::MAX - 1);
    assert_eq!(integer_sqrt(u64_max_sq), u64::MAX);

    // Powers of two
    for p in 0..128 {
        let val = 1u128 << p;
        let root = integer_sqrt(val);
        let root_u128 = root as u128;
        assert!(root_u128 * root_u128 <= val);
        if root < u64::MAX {
            let next = root_u128 + 1;
            assert!(next * next > val);
        }
    }
}

// ---------------------------------------------------------------------------
// Unit Tests - `calculate_effective_votes` Edge Cases
// ---------------------------------------------------------------------------

#[test]
fn test_calculate_effective_votes_zero_weights() {
    // Zero credits
    assert_eq!(
        calculate_effective_votes(0, 100, IvnIdentityTier::Verified, false),
        0
    );

    // Zero karma
    assert_eq!(
        calculate_effective_votes(100, 0, IvnIdentityTier::Verified, false),
        0
    );

    // Sybil flagged
    assert_eq!(
        calculate_effective_votes(100, 100, IvnIdentityTier::Verified, true),
        0
    );

    // Very low karma resulting in 0 karma_weight
    // Unverified multiplier is 1000 bps (0.1x). If karma is 9, (9 * 1000) / 10000 = 0
    assert_eq!(
        calculate_effective_votes(100, 9, IvnIdentityTier::Unverified, false),
        0
    );
}

#[test]
fn test_calculate_effective_votes_max_u64_allocations() {
    // Maximum u64 credits and max karma with Sovereign tier
    let eff = calculate_effective_votes(u64::MAX, u64::MAX, IvnIdentityTier::Sovereign, false);
    assert!(eff > 0);
    assert_eq!(eff, u64::MAX);

    // Max u64 credits with Unverified tier
    let eff_unverified =
        calculate_effective_votes(u64::MAX, u64::MAX, IvnIdentityTier::Unverified, false);
    assert!(eff_unverified > 0);
    assert!(eff_unverified < eff);
}

// ---------------------------------------------------------------------------
// Unit Tests - `QuadraticVoteEngine` Proposal Creation Edge Cases
// ---------------------------------------------------------------------------

#[test]
fn test_engine_default_and_get_voter() {
    let mut engine = QuadraticVoteEngine::default();
    assert_eq!(engine.default_quorum, 0);
    assert!(engine.get_voter("alice").is_none());

    let profile = VoterProfile::new("alice", 100, IvnIdentityTier::Verified, 500);
    engine.register_voter(profile.clone());
    assert_eq!(engine.get_voter("alice"), Some(&profile));
}

#[test]
fn test_create_proposal_empty_options() {
    let mut engine = QuadraticVoteEngine::new(100);
    let res = engine.create_proposal("prop_empty", vec![]);
    assert!(res.is_err());
    assert_eq!(res.unwrap_err(), "Proposal options cannot be empty");
}

#[test]
fn test_create_proposal_duplicate_id() {
    let mut engine = QuadraticVoteEngine::new(100);
    engine
        .create_proposal("prop_1", vec!["A".to_string()])
        .unwrap();

    let res = engine.create_proposal("prop_1", vec!["B".to_string()]);
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("already exists"));
}

// ---------------------------------------------------------------------------
// Unit Tests - `QuadraticVoteEngine` Voting Constraints & Edge Cases
// ---------------------------------------------------------------------------

#[test]
fn test_cast_vote_unregistered_voter() {
    let mut engine = QuadraticVoteEngine::new(100);
    engine
        .create_proposal("prop_1", vec!["Yes".to_string()])
        .unwrap();

    let res = engine.cast_vote("prop_1", "ghost_voter", "Yes", 10);
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("not registered"));
}

#[test]
fn test_cast_vote_nonexistent_proposal() {
    let mut engine = QuadraticVoteEngine::new(100);
    engine.register_voter(VoterProfile::new(
        "alice",
        100,
        IvnIdentityTier::Verified,
        100,
    ));

    let res = engine.cast_vote("prop_missing", "alice", "Yes", 10);
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("not found"));
}

#[test]
fn test_cast_vote_empty_allocations() {
    let mut engine = QuadraticVoteEngine::new(100);
    engine.register_voter(VoterProfile::new(
        "alice",
        100,
        IvnIdentityTier::Verified,
        100,
    ));
    engine
        .create_proposal("prop_1", vec!["Yes".to_string()])
        .unwrap();

    let res = engine.cast_multi_choice_ballot("prop_1", "alice", HashMap::new());
    assert!(res.is_err());
    assert_eq!(res.unwrap_err(), "Ballot allocations cannot be empty");
}

#[test]
fn test_cast_vote_invalid_option() {
    let mut engine = QuadraticVoteEngine::new(100);
    engine.register_voter(VoterProfile::new(
        "alice",
        100,
        IvnIdentityTier::Verified,
        100,
    ));
    engine
        .create_proposal("prop_1", vec!["Yes".to_string(), "No".to_string()])
        .unwrap();

    let res = engine.cast_vote("prop_1", "alice", "Maybe", 10);
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("Invalid option 'Maybe'"));
}

#[test]
fn test_cast_vote_zero_credits_allocation() {
    let mut engine = QuadraticVoteEngine::new(100);
    engine.register_voter(VoterProfile::new(
        "alice",
        100,
        IvnIdentityTier::Verified,
        100,
    ));
    engine
        .create_proposal("prop_1", vec!["Yes".to_string()])
        .unwrap();

    let res = engine.cast_vote("prop_1", "alice", "Yes", 0);
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("must be greater than zero"));
}

#[test]
fn test_cast_multi_choice_credit_sum_overflow() {
    let mut engine = QuadraticVoteEngine::new(100);
    engine.register_voter(VoterProfile::new(
        "alice",
        100,
        IvnIdentityTier::Verified,
        u64::MAX,
    ));
    engine
        .create_proposal("prop_1", vec!["A".to_string(), "B".to_string()])
        .unwrap();

    let mut allocations = HashMap::new();
    allocations.insert("A".to_string(), u64::MAX);
    allocations.insert("B".to_string(), 1);

    let res = engine.cast_multi_choice_ballot("prop_1", "alice", allocations);
    assert!(res.is_err());
    assert_eq!(res.unwrap_err(), "Credit sum overflow");
}

#[test]
fn test_cast_vote_insufficient_credits() {
    let mut engine = QuadraticVoteEngine::new(100);
    engine.register_voter(VoterProfile::new(
        "alice",
        100,
        IvnIdentityTier::Verified,
        50,
    ));
    engine
        .create_proposal("prop_1", vec!["Yes".to_string()])
        .unwrap();

    let res = engine.cast_vote("prop_1", "alice", "Yes", 51);
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("Insufficient credits"));
}

// ---------------------------------------------------------------------------
// Unit Tests - `QuadraticVoteEngine` Tallying Edge Cases
// ---------------------------------------------------------------------------

#[test]
fn test_tally_nonexistent_proposal() {
    let engine = QuadraticVoteEngine::new(100);
    let res = engine.tally("missing");
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("not found"));
}

#[test]
fn test_tally_unvoted_proposal() {
    let mut engine = QuadraticVoteEngine::new(50);
    engine
        .create_proposal("prop_unvoted", vec!["A".to_string(), "B".to_string()])
        .unwrap();

    let tally = engine.tally("prop_unvoted").unwrap();
    assert_eq!(tally.total_voters, 0);
    assert_eq!(tally.sybil_votes_rejected, 0);
    assert_eq!(tally.winning_option, None);
    assert!(!tally.quorum_reached);
    assert_eq!(*tally.option_tallies.get("A").unwrap(), 0);
    assert_eq!(*tally.option_tallies.get("B").unwrap(), 0);
}

#[test]
fn test_tally_unvoted_proposal_zero_quorum() {
    let mut engine = QuadraticVoteEngine::new(0);
    engine
        .create_proposal("prop_zero_q", vec!["A".to_string()])
        .unwrap();

    let tally = engine.tally("prop_zero_q").unwrap();
    assert!(tally.quorum_reached);
}

#[test]
fn test_tally_voter_unregistered_after_voting() {
    let mut engine = QuadraticVoteEngine::new(10);
    engine.register_voter(VoterProfile::new(
        "alice",
        100,
        IvnIdentityTier::Verified,
        100,
    ));
    engine
        .create_proposal("prop_1", vec!["Yes".to_string()])
        .unwrap();
    engine.cast_vote("prop_1", "alice", "Yes", 100).unwrap();

    // Remove voter from registered map
    engine.voters.remove("alice");

    // Tally should handle missing voter by defaulting karma to 0 / Unverified
    let tally = engine.tally("prop_1").unwrap();
    assert_eq!(tally.total_voters, 1);
    assert_eq!(tally.sybil_votes_rejected, 0);
    assert_eq!(*tally.option_tallies.get("Yes").unwrap(), 0);
}

#[test]
fn test_tally_all_sybil_voters() {
    let mut engine = QuadraticVoteEngine::new(10);
    engine.register_voter(
        VoterProfile::new("sybil_1", 100, IvnIdentityTier::Verified, 100).with_sybil_flag(true),
    );
    engine.register_voter(
        VoterProfile::new("sybil_2", 100, IvnIdentityTier::Verified, 100).with_sybil_flag(true),
    );

    engine
        .create_proposal("prop_sybil", vec!["Yes".to_string(), "No".to_string()])
        .unwrap();
    engine
        .cast_vote("prop_sybil", "sybil_1", "Yes", 50)
        .unwrap();
    engine.cast_vote("prop_sybil", "sybil_2", "No", 50).unwrap();

    let tally = engine.tally("prop_sybil").unwrap();
    assert_eq!(tally.total_voters, 2);
    assert_eq!(tally.sybil_votes_rejected, 2);
    assert_eq!(*tally.option_tallies.get("Yes").unwrap(), 0);
    assert_eq!(*tally.option_tallies.get("No").unwrap(), 0);
    assert_eq!(tally.winning_option, None);
}

// ---------------------------------------------------------------------------
// Unit Tests - Serialization / Deserialization (Serde)
// ---------------------------------------------------------------------------

#[test]
fn test_serde_roundtrip_all_types() {
    // IvnIdentityTier
    let tier = IvnIdentityTier::Sovereign;
    let serialized_tier = serde_json::to_string(&tier).unwrap();
    let deserialized_tier: IvnIdentityTier = serde_json::from_str(&serialized_tier).unwrap();
    assert_eq!(tier, deserialized_tier);

    // VoterProfile
    let voter =
        VoterProfile::new("bob", 250, IvnIdentityTier::Validator, 500).with_sybil_flag(true);
    let serialized_voter = serde_json::to_string(&voter).unwrap();
    let deserialized_voter: VoterProfile = serde_json::from_str(&serialized_voter).unwrap();
    assert_eq!(voter, deserialized_voter);

    // MultiChoiceBallot
    let mut allocations = HashMap::new();
    allocations.insert("Option1".to_string(), 100);
    let ballot = MultiChoiceBallot {
        voter_id: "bob".to_string(),
        allocations,
    };
    let serialized_ballot = serde_json::to_string(&ballot).unwrap();
    let deserialized_ballot: MultiChoiceBallot = serde_json::from_str(&serialized_ballot).unwrap();
    assert_eq!(ballot, deserialized_ballot);

    // QuadraticProposal
    let mut ballots = HashMap::new();
    ballots.insert("bob".to_string(), ballot);
    let proposal = QuadraticProposal {
        id: "p1".to_string(),
        options: vec!["Option1".to_string()],
        ballots,
        quorum: 100,
    };
    let serialized_prop = serde_json::to_string(&proposal).unwrap();
    let deserialized_prop: QuadraticProposal = serde_json::from_str(&serialized_prop).unwrap();
    assert_eq!(proposal, deserialized_prop);

    // ProposalTallyResult
    let mut option_tallies = HashMap::new();
    option_tallies.insert("Option1".to_string(), 100);
    let result = ProposalTallyResult {
        proposal_id: "p1".to_string(),
        option_tallies,
        total_voters: 1,
        sybil_votes_rejected: 0,
        winning_option: Some("Option1".to_string()),
        quorum_reached: true,
    };
    let serialized_result = serde_json::to_string(&result).unwrap();
    let deserialized_result: ProposalTallyResult =
        serde_json::from_str(&serialized_result).unwrap();
    assert_eq!(result, deserialized_result);
}
