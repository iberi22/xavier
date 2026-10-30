use chrono::{Duration, TimeZone, Utc};
use xavier::maloca::governance::{is_expired, outcome, tally, Outcome};
use xavier::maloca::types::{Vote, VoteChoice};

xavier::isolate_test_process!();

fn mk_vote(choice: VoteChoice, weight: f64) -> Vote {
    Vote {
        id: "v".into(),
        proposal_id: "p".into(),
        node_id: "n".into(),
        choice,
        weight,
        created_at: "2026-10-01T00:00:00Z".into(),
    }
}

#[test]
fn weighted_majority_with_quorum_is_approved() {
    let mut votes = vec![];
    votes.push(mk_vote(VoteChoice::Yes, 5.0));
    votes.push(mk_vote(VoteChoice::No, 2.0));
    votes.push(mk_vote(VoteChoice::Abstain, 1.0));

    let t = tally(&votes, 10.0);
    assert_eq!(t.participation_pct, 80.0); // 8 weight out of 10 eligible

    // Outcome: quorum 50%, threshold 0.5 (yes > no AND yes / (yes+no) >= 0.5)
    let res = outcome(&t, 50.0, 0.5);
    assert_eq!(res, Outcome::Approved);
}

#[test]
fn abstentions_count_for_quorum_not_for_decision() {
    let mut votes = vec![];
    votes.push(mk_vote(VoteChoice::Yes, 1.0));
    votes.push(mk_vote(VoteChoice::No, 1.0));
    for _ in 0..4 {
        votes.push(mk_vote(VoteChoice::Abstain, 1.0));
    }

    let t = tally(&votes, 10.0);
    assert_eq!(t.participation_pct, 60.0); // 6 weight out of 10 eligible -> quorum 50% met

    // Tie is a rejection
    let res = outcome(&t, 50.0, 0.5);
    assert_eq!(res, Outcome::Rejected);
}

#[test]
fn below_quorum_is_no_quorum() {
    let votes = vec![mk_vote(VoteChoice::Yes, 1.0)];
    let t = tally(&votes, 10.0); // 1 weight out of 10 = 10%

    // Quorum 50%
    let res = outcome(&t, 50.0, 0.5);
    assert_eq!(res, Outcome::NoQuorum);
}

#[test]
fn zero_eligible_weight_has_zero_participation_and_no_quorum() {
    let votes = vec![mk_vote(VoteChoice::Yes, 1.0)];
    let t = tally(&votes, 0.0);

    assert_eq!(t.participation_pct, 0.0);

    // Quorum > 0%
    let res = outcome(&t, 50.0, 0.5);
    assert_eq!(res, Outcome::NoQuorum);
}

#[test]
fn expiry_boundary() {
    let created_at = Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap();
    let duration_days = 7;

    let boundary = created_at + Duration::try_days(duration_days).unwrap();

    // one second before is false
    assert!(!is_expired(
        created_at,
        duration_days,
        boundary - Duration::seconds(1)
    ));

    // at boundary is true
    assert!(is_expired(created_at, duration_days, boundary));

    // after boundary is true
    assert!(is_expired(
        created_at,
        duration_days,
        boundary + Duration::seconds(1)
    ));
}
