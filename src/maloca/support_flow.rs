//! Support ticket lifecycle rules. Pure: no I/O, no storage.
//!
//! Forward moves only, plus `Resolved -> InProgress` (reopen). `Closed` is
//! reachable only from `Resolved` and is terminal.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportStatus {
    Open,
    Triaged,
    InProgress,
    Resolved,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidTransition {
    pub from: SupportStatus,
    pub to: SupportStatus,
}

impl fmt::Display for InvalidTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid support transition {:?} -> {:?}",
            self.from, self.to
        )
    }
}

impl std::error::Error for InvalidTransition {}

fn rank(status: SupportStatus) -> u8 {
    match status {
        SupportStatus::Open => 0,
        SupportStatus::Triaged => 1,
        SupportStatus::InProgress => 2,
        SupportStatus::Resolved => 3,
        SupportStatus::Closed => 4,
    }
}

pub fn transition(
    from: SupportStatus,
    to: SupportStatus,
) -> Result<SupportStatus, InvalidTransition> {
    use SupportStatus::*;
    let allowed = match (from, to) {
        (Resolved, InProgress) | (Resolved, Closed) => true,
        (_, Closed) => false,
        (_, _) => rank(to) > rank(from),
    };
    if allowed {
        Ok(to)
    } else {
        Err(InvalidTransition { from, to })
    }
}

#[cfg(test)]
mod tests {
    use super::SupportStatus::*;
    use super::*;

    #[test]
    fn forward_moves_are_allowed() {
        assert_eq!(transition(Open, Triaged), Ok(Triaged));
        assert_eq!(transition(Triaged, InProgress), Ok(InProgress));
        assert_eq!(transition(InProgress, Resolved), Ok(Resolved));
        // Forward skips are still forward moves.
        assert_eq!(transition(Open, InProgress), Ok(InProgress));
    }

    #[test]
    fn reopen_from_resolved_is_allowed() {
        assert_eq!(transition(Resolved, InProgress), Ok(InProgress));
    }

    #[test]
    fn close_only_from_resolved() {
        assert_eq!(transition(Resolved, Closed), Ok(Closed));
        assert!(transition(Open, Closed).is_err());
        assert!(transition(Triaged, Closed).is_err());
        assert!(transition(InProgress, Closed).is_err());
    }

    #[test]
    fn backward_and_same_state_moves_are_rejected() {
        assert!(transition(Triaged, Open).is_err());
        assert!(transition(InProgress, Triaged).is_err());
        assert!(transition(Resolved, Triaged).is_err());
        assert!(transition(Open, Open).is_err());
        assert!(transition(Resolved, Resolved).is_err());
    }

    #[test]
    fn closed_is_terminal() {
        assert!(transition(Closed, Resolved).is_err());
        assert!(transition(Closed, InProgress).is_err());
        assert!(transition(Closed, Closed).is_err());
    }

    #[test]
    fn error_carries_from_and_to() {
        let err = transition(Closed, Open).unwrap_err();
        assert_eq!(
            err,
            InvalidTransition {
                from: Closed,
                to: Open
            }
        );
        assert!(err.to_string().contains("Closed"));
    }
}
