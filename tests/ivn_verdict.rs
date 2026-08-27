use rand::rngs::StdRng;
use rand::SeedableRng;
use xavier::data_commons::governance::DynamicQuorum;
use xavier::data_commons::ivn::{
    sanction_validator, sanction_validator_with_config, IvnConfig, IvnError, SanctionResult,
    ValidatorCandidate, ValidatorSelection, VerdictEngine, VerdictStatus, Vote,
};
use xavier::data_commons::types::WalletAddress;

#[test]
fn test_validator_selection_weights_by_karma() {
    let mut high_karma_selected_count = 0;
    let total_trials = 100;

    let pool = vec![
        ValidatorCandidate {
            node_id: WalletAddress("xv1_high_karma".into()),
            wallet: WalletAddress("xv1_w_high".into()),
            karma: 3000,
            seed: "seed_high".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_low_1".into()),
            wallet: WalletAddress("xv1_w_low1".into()),
            karma: 300,
            seed: "seed_low1".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_low_2".into()),
            wallet: WalletAddress("xv1_w_low2".into()),
            karma: 300,
            seed: "seed_low2".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_low_3".into()),
            wallet: WalletAddress("xv1_w_low3".into()),
            karma: 300,
            seed: "seed_low3".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_low_4".into()),
            wallet: WalletAddress("xv1_w_low4".into()),
            karma: 300,
            seed: "seed_low4".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_low_5".into()),
            wallet: WalletAddress("xv1_w_low5".into()),
            karma: 300,
            seed: "seed_low5".into(),
        },
    ];

    for trial in 0..total_trials {
        let mut rng = StdRng::seed_from_u64(trial as u64 + 100);
        let selected =
            ValidatorSelection::select_validators(&pool, "applicant_seed", &mut rng).unwrap();

        if selected
            .iter()
            .any(|v| v.node_id == WalletAddress("xv1_high_karma".into()))
        {
            high_karma_selected_count += 1;
        }
    }

    assert!(
        high_karma_selected_count >= 95,
        "High karma node should be selected in almost all trials, got {}",
        high_karma_selected_count
    );
}

#[test]
fn test_dynamic_quorum_calculation() {
    let dq = DynamicQuorum::new(0.8, 0.51);

    // Baseline user quorum is 0.8
    assert_eq!(dq.effective_user_quorum(0.50), 0.8);

    // Low participation (<0.30) lowers quorum by 20% (0.8 -> 0.64)
    let low_q = dq.effective_user_quorum(0.20);
    assert!((low_q - 0.64).abs() < 1e-6);

    // High participation (>0.80) raises quorum by 10% (0.8 -> 0.88)
    let high_q = dq.effective_user_quorum(0.90);
    assert!((high_q - 0.88).abs() < 1e-6);
}

#[test]
fn test_sanction_application() {
    let config = IvnConfig::default();

    // False positive sanction
    let sanction_fp = sanction_validator_with_config(&config, 1, false);
    assert_eq!(sanction_fp.karma_penalty, -10);
    assert_eq!(sanction_fp.exclusion_days, 90);

    // Liar sanction
    let sanction_liar = sanction_validator_with_config(&config, 0, true);
    assert_eq!(sanction_liar.karma_penalty, -50);
    assert_eq!(sanction_liar.exclusion_days, 90);

    // Combined false positive + liar sanction
    let sanction_both = sanction_validator_with_config(&config, 2, true);
    assert_eq!(sanction_both.karma_penalty, -70); // -50 liar + (2 * -10)
    assert_eq!(sanction_both.exclusion_days, 90);
}

#[test]
fn test_vote_recording_and_tallying() {
    let votes = vec![
        Vote::Check,
        Vote::Check,
        Vote::Check,
        Vote::Reject,
        Vote::Abstain,
    ];

    let verdict = VerdictEngine::evaluate_votes(&votes, 0.6);
    assert_eq!(verdict.check_count, 3);
    assert_eq!(verdict.reject_count, 1);
    assert_eq!(verdict.abstain_count, 1);
    assert_eq!(verdict.total_votes, 5);
    assert!((verdict.approval_ratio - 0.6).abs() < 1e-6);
    assert_eq!(verdict.status, VerdictStatus::Passed);
}

#[test]
fn test_selection_karma_weighted_distribution() {
    let mut high_karma_selected_count = 0;
    let total_trials = 100;

    let pool = vec![
        ValidatorCandidate {
            node_id: WalletAddress("xv1_high_karma".into()),
            wallet: WalletAddress("xv1_w_high".into()),
            karma: 3000,
            seed: "seed_high".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_low_1".into()),
            wallet: WalletAddress("xv1_w_low1".into()),
            karma: 300,
            seed: "seed_low1".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_low_2".into()),
            wallet: WalletAddress("xv1_w_low2".into()),
            karma: 300,
            seed: "seed_low2".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_low_3".into()),
            wallet: WalletAddress("xv1_w_low3".into()),
            karma: 300,
            seed: "seed_low3".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_low_4".into()),
            wallet: WalletAddress("xv1_w_low4".into()),
            karma: 300,
            seed: "seed_low4".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_low_5".into()),
            wallet: WalletAddress("xv1_w_low5".into()),
            karma: 300,
            seed: "seed_low5".into(),
        },
    ];

    for trial in 0..total_trials {
        let mut rng = StdRng::seed_from_u64(trial as u64 + 100);
        let selected =
            ValidatorSelection::select_validators(&pool, "applicant_seed", &mut rng).unwrap();

        if selected
            .iter()
            .any(|v| v.node_id == WalletAddress("xv1_high_karma".into()))
        {
            high_karma_selected_count += 1;
        }
    }

    assert!(
        high_karma_selected_count >= 95,
        "High karma node should be selected in almost all trials, got {}",
        high_karma_selected_count
    );
}

#[test]
fn test_exclusion_of_shared_seed_and_insufficient_karma() {
    let mut rng = StdRng::seed_from_u64(42);

    let shared_seed = "shared_ip_or_seed_123";

    let pool = vec![
        ValidatorCandidate {
            node_id: WalletAddress("xv1_shared1".into()),
            wallet: WalletAddress("xv1_w_s1".into()),
            karma: 1000,
            seed: shared_seed.into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_low_karma".into()),
            wallet: WalletAddress("xv1_w_lk".into()),
            karma: 250,
            seed: "unique_seed_1".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_ok_1".into()),
            wallet: WalletAddress("xv1_w_ok1".into()),
            karma: 500,
            seed: "unique_seed_2".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_ok_2".into()),
            wallet: WalletAddress("xv1_w_ok2".into()),
            karma: 600,
            seed: "unique_seed_3".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_ok_3".into()),
            wallet: WalletAddress("xv1_w_ok3".into()),
            karma: 700,
            seed: "unique_seed_4".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_ok_4".into()),
            wallet: WalletAddress("xv1_w_ok4".into()),
            karma: 800,
            seed: "unique_seed_5".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_ok_5".into()),
            wallet: WalletAddress("xv1_w_ok5".into()),
            karma: 900,
            seed: "unique_seed_6".into(),
        },
    ];

    let selected = ValidatorSelection::select_validators(&pool, shared_seed, &mut rng).unwrap();

    assert_eq!(selected.len(), 5);
    for v in &selected {
        assert_ne!(v.seed, shared_seed);
        assert!(v.karma >= 300);
    }

    let small_pool = vec![
        ValidatorCandidate {
            node_id: WalletAddress("xv1_shared_only".into()),
            wallet: WalletAddress("xv1_w_so".into()),
            karma: 1000,
            seed: shared_seed.into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_ok_1".into()),
            wallet: WalletAddress("xv1_w_ok1".into()),
            karma: 500,
            seed: "unique_1".into(),
        },
    ];

    let err = ValidatorSelection::select_validators(&small_pool, shared_seed, &mut rng);
    assert!(err.is_err());
    assert_eq!(
        err.unwrap_err(),
        IvnError::InsufficientValidators {
            found: 1,
            required: 5
        }
    );
}

#[test]
fn test_quorum_4_of_5_pass_and_fail() {
    let votes_4_pass = vec![
        Vote::Check,
        Vote::Check,
        Vote::Check,
        Vote::Check,
        Vote::Reject,
    ];
    let verdict_pass = VerdictEngine::evaluate_votes(&votes_4_pass, 0.8);
    assert_eq!(verdict_pass.status, VerdictStatus::Passed);
    assert!(verdict_pass.is_passed());
    assert_eq!(verdict_pass.check_count, 4);
    assert_eq!(verdict_pass.reject_count, 1);
    assert_eq!(verdict_pass.approval_ratio, 0.8);

    let votes_3_fail = vec![
        Vote::Check,
        Vote::Check,
        Vote::Check,
        Vote::Reject,
        Vote::Reject,
    ];
    let verdict_fail = VerdictEngine::evaluate_votes(&votes_3_fail, 0.8);
    assert_eq!(verdict_fail.status, VerdictStatus::Rejected);
    assert!(!verdict_fail.is_passed());
    assert_eq!(verdict_fail.approval_ratio, 0.6);

    let votes_5_pass = vec![
        Vote::Check,
        Vote::Check,
        Vote::Check,
        Vote::Check,
        Vote::Check,
    ];
    let verdict_perfect = VerdictEngine::evaluate_votes(&votes_5_pass, 0.8);
    assert_eq!(verdict_perfect.status, VerdictStatus::Passed);
    assert_eq!(verdict_perfect.approval_ratio, 1.0);
}

#[test]
fn test_abstention_handling_in_verdict() {
    let votes_abstain_fail = vec![
        Vote::Check,
        Vote::Check,
        Vote::Check,
        Vote::Abstain,
        Vote::Abstain,
    ];
    let verdict_abstain = VerdictEngine::evaluate_votes(&votes_abstain_fail, 0.8);
    assert_eq!(verdict_abstain.status, VerdictStatus::QuorumNotMet);
    assert_eq!(verdict_abstain.check_count, 3);
    assert_eq!(verdict_abstain.abstain_count, 2);

    let votes_abstain_pass = vec![
        Vote::Check,
        Vote::Check,
        Vote::Check,
        Vote::Check,
        Vote::Abstain,
    ];
    let verdict_pass = VerdictEngine::evaluate_votes(&votes_abstain_pass, 0.8);
    assert_eq!(verdict_pass.status, VerdictStatus::Passed);
    assert_eq!(verdict_pass.check_count, 4);
    assert_eq!(verdict_pass.abstain_count, 1);
}

#[test]
fn test_dynamic_quorum_integration_with_ivn() {
    let dq = DynamicQuorum::new(0.8, 0.51);

    let votes = vec![
        Vote::Check,
        Vote::Check,
        Vote::Check,
        Vote::Check,
        Vote::Reject,
    ];

    let verdict_low_part = dq.evaluate_ivn_verdict(&votes, 0.20);
    assert_eq!(verdict_low_part.status, VerdictStatus::Passed);
    assert!((verdict_low_part.effective_quorum - 0.64).abs() < 1e-6);

    let verdict_high_part = dq.evaluate_ivn_verdict(&votes, 0.90);
    assert_eq!(verdict_high_part.status, VerdictStatus::Rejected);
    assert!((verdict_high_part.effective_quorum - 0.88).abs() < 1e-6);
}

#[test]
fn test_sanction_validator_penalties() {
    let sanction1 = sanction_validator(1);
    assert_eq!(
        sanction1,
        SanctionResult {
            karma_penalty: -10,
            exclusion_days: 90
        }
    );

    let sanction3 = sanction_validator(3);
    assert_eq!(sanction3.karma_penalty, -30);

    let config = IvnConfig::default();
    let sanction_lie = sanction_validator_with_config(&config, 1, true);
    assert_eq!(
        sanction_lie,
        SanctionResult {
            karma_penalty: -60,
            exclusion_days: 90
        }
    );
}
