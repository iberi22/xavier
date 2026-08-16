use rand::rngs::StdRng;
use rand::SeedableRng;
use xavier::data_commons::ivn::{
    apply_rewards, apply_sanctions, clear_exclusions, is_excluded, is_excluded_at,
    record_exclusion, ValidatorCandidate, ValidatorSelection, VerdictEngine, Vote,
};
use xavier::data_commons::reputation::{EigenTrustEngine, ReputationConfig};
use xavier::data_commons::types::WalletAddress;

#[test]
fn test_reward_application_exact_deltas() {
    let mut engine = EigenTrustEngine::new(ReputationConfig::default(), vec![]);

    let applicant = WalletAddress("xv1_applicant_1".into());
    let v1 = WalletAddress("xv1_validator_1".into());
    let v2 = WalletAddress("xv1_validator_2".into());
    let v3 = WalletAddress("xv1_validator_3".into());

    let votes = vec![
        (v1.clone(), Vote::Check),
        (v2.clone(), Vote::Check),
        (v3.clone(), Vote::Abstain),
    ];

    let vote_enums: Vec<Vote> = votes.iter().map(|(_, v)| *v).collect();
    let verdict = VerdictEngine::evaluate_votes(&vote_enums, 0.6);
    assert!(verdict.is_passed());

    let summary = apply_rewards(&mut engine, &verdict, &applicant, &votes);

    // Verified applicant gets +20 karma
    assert_eq!(summary.applicant_delta, 20);
    assert_eq!(engine.karma_of(&applicant), 20);

    // Correct Check validators get +5 karma
    assert_eq!(engine.karma_of(&v1), 5);
    assert_eq!(engine.karma_of(&v2), 5);

    // Abstaining validator gets +1 karma
    assert_eq!(engine.karma_of(&v3), 1);
}

#[test]
fn test_sanction_application_and_eigentrust_routing() {
    // NOTE: wallet names below are unique per test; do NOT call clear_exclusions()
    // here — EXCLUSION_STORE is a process-global and clearing it races with other
    // tests in this binary (parallel execution).
    let mut engine = EigenTrustEngine::new(ReputationConfig::default(), vec![]);

    let fp_val1 = WalletAddress("xv1_fp_val_1".into());
    let fp_val2 = WalletAddress("xv1_fp_val_2".into());
    let liar = WalletAddress("xv1_lying_applicant".into());

    let current_time = 1_700_000_000u64;

    let summary = apply_sanctions(
        &mut engine,
        &[fp_val1.clone(), fp_val2.clone()],
        Some(&liar),
        current_time,
    );

    // False positive validators lose -10 karma
    assert_eq!(engine.karma_of(&fp_val1), -10);
    assert_eq!(engine.karma_of(&fp_val2), -10);

    // Lying applicant loses -50 karma
    assert_eq!(engine.karma_of(&liar), -50);

    assert_eq!(summary.fp_validator_sanctions.len(), 2);
    assert!(summary.liar_sanction.is_some());

    // Check exclusion windows
    // FP validators excluded for 90 days (90 * 86,400 = 7,776,000s)
    let seconds_90d = 90 * 86_400;
    assert!(is_excluded_at(&fp_val1, current_time + 100));
    assert!(is_excluded_at(&fp_val1, current_time + seconds_90d - 1));
    assert!(!is_excluded_at(&fp_val1, current_time + seconds_90d + 1));

    // Lying applicant excluded for 180 days (180 * 86,400 = 15,552,000s)
    let seconds_180d = 180 * 86_400;
    assert!(is_excluded_at(&liar, current_time + 100));
    assert!(is_excluded_at(&liar, current_time + seconds_180d - 1));
    assert!(!is_excluded_at(&liar, current_time + seconds_180d + 1));
}

#[test]
fn test_exclusion_window_integration_with_selection() {
    // NOTE: no clear_exclusions() here — unique wallet names keep tests disjoint;
    // the global store is shared across tests in this binary (parallel execution).

    let excluded_val = WalletAddress("xv1_excluded_val".into());
    // Record exclusion until far into the future relative to current time
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    record_exclusion(&excluded_val, now + 100_000);

    assert!(is_excluded(&excluded_val));

    let pool = vec![
        ValidatorCandidate {
            node_id: WalletAddress("xv1_node_exc".into()),
            wallet: excluded_val.clone(),
            karma: 1000,
            seed: "seed_exc".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_ok1".into()),
            wallet: WalletAddress("xv1_w1".into()),
            karma: 500,
            seed: "s1".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_ok2".into()),
            wallet: WalletAddress("xv1_w2".into()),
            karma: 600,
            seed: "s2".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_ok3".into()),
            wallet: WalletAddress("xv1_w3".into()),
            karma: 700,
            seed: "s3".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_ok4".into()),
            wallet: WalletAddress("xv1_w4".into()),
            karma: 800,
            seed: "s4".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_ok5".into()),
            wallet: WalletAddress("xv1_w5".into()),
            karma: 900,
            seed: "s5".into(),
        },
    ];

    let mut rng = StdRng::seed_from_u64(42);
    let selected = ValidatorSelection::select_validators(&pool, "app_seed", &mut rng).unwrap();

    assert_eq!(selected.len(), 5);
    for cand in &selected {
        assert_ne!(cand.wallet, excluded_val);
    }
}
