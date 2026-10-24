//! Pure governance rules: vote tally, quorum and proposal expiry.
//!
//! No I/O. Vote weights come from the caller (`Vote::weight`), never from a
//! constant. Callers pass only the votes of one proposal.

use chrono::{DateTime, Duration, Utc};

use super::types::{Vote, VoteChoice};

/// Float slack for percentage and threshold comparisons.
const EPS: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tally {
    pub yes: f64,
    pub no: f64,
    pub abstain: f64,
    pub eligible: f64,
    /// Cast weight (abstentions included) as a percentage of `eligible`.
    pub participation_pct: f64,
}

impl Tally {
    /// Total weight of every cast vote, abstentions included.
    pub fn cast(&self) -> f64 {
        self.yes + self.no + self.abstain
    }

    /// Weight of the decisive votes (yes and no). Abstentions are excluded.
    pub fn decisive(&self) -> f64 {
        self.yes + self.no
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Approved,
    Rejected,
    NoQuorum,
}

pub fn tally(votes: &[Vote], eligible_weight: f64) -> Tally {
    let (mut yes, mut no, mut abstain) = (0.0, 0.0, 0.0);
    for vote in votes {
        match vote.choice {
            VoteChoice::Yes => yes += vote.weight,
            VoteChoice::No => no += vote.weight,
            VoteChoice::Abstain => abstain += vote.weight,
        }
    }
    let cast = yes + no + abstain;
    let participation_pct = if eligible_weight > 0.0 {
        cast / eligible_weight * 100.0
    } else {
        0.0
    };
    Tally {
        yes,
        no,
        abstain,
        eligible: eligible_weight,
        participation_pct,
    }
}

/// `quorum_pct` is a percentage of the eligible weight; abstentions count
/// toward it. `approval_threshold` is a fraction (0.0..=1.0) of the decisive
/// votes (yes + no); abstentions never count toward approval.
pub fn outcome(tally: &Tally, quorum_pct: f64, approval_threshold: f64) -> Outcome {
    if tally.participation_pct + EPS < quorum_pct {
        return Outcome::NoQuorum;
    }
    let decisive = tally.decisive();
    if decisive <= 0.0 {
        return Outcome::Rejected;
    }
    if tally.yes + EPS >= approval_threshold * decisive {
        Outcome::Approved
    } else {
        Outcome::Rejected
    }
}

/// A proposal expires once `now` reaches `created_at + duration_days`.
/// Out-of-range durations never expire.
pub fn is_expired(created_at: DateTime<Utc>, duration_days: i64, now: DateTime<Utc>) -> bool {
    match Duration::try_days(duration_days).and_then(|d| created_at.checked_add_signed(d)) {
        Some(expiry) => now >= expiry,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn vote(choice: VoteChoice, weight: f64) -> Vote {
        Vote {
            id: "v".into(),
            proposal_id: "p".into(),
            node_id: "n".into(),
            choice,
            weight,
            created_at: "2026-10-01T00:00:00Z".into(),
        }
    }

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, day, hour, 0, 0).unwrap()
    }

    #[test]
    fn tally_sums_weights_per_choice() {
        let votes = [
            vote(VoteChoice::Yes, 2.5),
            vote(VoteChoice::Yes, 1.5),
            vote(VoteChoice::No, 0.5),
            vote(VoteChoice::Abstain, 1.0),
        ];
        let t = tally(&votes, 10.0);
        assert_eq!(t.yes, 4.0);
        assert_eq!(t.no, 0.5);
        assert_eq!(t.abstain, 1.0);
        assert_eq!(t.cast(), 5.5);
        assert_eq!(t.eligible, 10.0);
        assert!((t.participation_pct - 55.0).abs() < 1e-9);
    }

    #[test]
    fn tally_without_eligible_weight_has_zero_participation() {
        let t = tally(&[vote(VoteChoice::Yes, 1.0)], 0.0);
        assert_eq!(t.participation_pct, 0.0);
        assert_eq!(outcome(&t, 30.0, 0.5), Outcome::NoQuorum);
    }

    #[test]
    fn one_yes_out_of_ten_eligible_is_no_quorum() {
        let t = tally(&[vote(VoteChoice::Yes, 1.0)], 10.0);
        assert_eq!(outcome(&t, 30.0, 0.5), Outcome::NoQuorum);
    }

    #[test]
    fn majority_of_votes_cast_with_quorum_is_approved() {
        // 51 yes, 49 no: 100 cast of 100 eligible.
        let mut votes = vec![vote(VoteChoice::Yes, 1.0); 51];
        votes.extend(vec![vote(VoteChoice::No, 1.0); 49]);
        let t = tally(&votes, 100.0);
        assert_eq!(outcome(&t, 30.0, 0.5), Outcome::Approved);
    }

    #[test]
    fn threshold_is_relative_to_decisive_votes_not_eligible() {
        // 6 yes / 3 no out of 20 eligible: 9 cast (45%, quorum met).
        // 6/9 = 0.67 >= 0.6 -> Approved. An absolute check (6 >= 0.6 * 20)
        // would wrongly reject.
        let mut votes = vec![vote(VoteChoice::Yes, 1.0); 6];
        votes.extend(vec![vote(VoteChoice::No, 1.0); 3]);
        let t = tally(&votes, 20.0);
        assert_eq!(outcome(&t, 30.0, 0.6), Outcome::Approved);
    }

    #[test]
    fn yes_below_threshold_of_votes_cast_is_rejected() {
        let mut votes = vec![vote(VoteChoice::Yes, 1.0); 4];
        votes.extend(vec![vote(VoteChoice::No, 1.0); 6]);
        let t = tally(&votes, 10.0);
        assert_eq!(outcome(&t, 30.0, 0.5), Outcome::Rejected);
    }

    #[test]
    fn approval_at_exact_threshold_passes() {
        let mut votes = vec![vote(VoteChoice::Yes, 1.0); 5];
        votes.extend(vec![vote(VoteChoice::No, 1.0); 5]);
        let t = tally(&votes, 10.0);
        assert_eq!(outcome(&t, 30.0, 0.5), Outcome::Approved);
    }

    #[test]
    fn abstentions_do_not_count_toward_approval() {
        // 3 yes, 1 no, 2 abstain out of 10 eligible. Quorum: 6/10 = 60% >= 30%.
        // Approval over decisive votes: 3/4 = 0.75 >= 0.6 -> Approved.
        // Over all cast votes it would be 3/6 = 0.5 < 0.6 -> Rejected.
        let mut votes = vec![vote(VoteChoice::Yes, 1.0); 3];
        votes.push(vote(VoteChoice::No, 1.0));
        votes.extend(vec![vote(VoteChoice::Abstain, 1.0); 2]);
        let t = tally(&votes, 10.0);
        assert_eq!(outcome(&t, 30.0, 0.6), Outcome::Approved);
    }

    #[test]
    fn abstain_only_meets_quorum_but_never_approves() {
        // 5 abstain out of 10 eligible: quorum met, no decisive votes -> Rejected.
        let votes = vec![vote(VoteChoice::Abstain, 1.0); 5];
        let t = tally(&votes, 10.0);
        assert_eq!(outcome(&t, 30.0, 0.5), Outcome::Rejected);
    }

    #[test]
    fn no_votes_with_zero_quorum_is_rejected() {
        let t = tally(&[], 10.0);
        assert_eq!(outcome(&t, 0.0, 0.5), Outcome::Rejected);
    }

    #[test]
    fn expiry_boundaries() {
        let created = at(1, 0);
        assert!(!is_expired(created, 7, at(8, 0) - Duration::seconds(1)));
        assert!(is_expired(created, 7, at(8, 0)));
        assert!(is_expired(created, 7, at(9, 0)));
        assert!(!is_expired(created, 0, created - Duration::seconds(1)));
        assert!(is_expired(created, 0, created));
    }

    #[test]
    fn expiry_out_of_range_never_expires() {
        assert!(!is_expired(at(1, 0), i64::MAX, at(1, 0)));
    }
}
