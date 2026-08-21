//! HumanChallenge Types & Data Models
//!
//! Defines the 5 canonical challenge types (Contradiction, Decision,
//! Execution, Assumption, Clarification), storage models, and farming status
//! following Privacy P4 guidelines (local payload, anonymous mesh score).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// The 5 canonical HumanChallenge types defined in HUMAN_CHALLENGE.md
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeType {
    /// Detects conflicting facts, instructions, or declarations
    Contradiction,
    /// Detects architectural, design, or operational choices
    Decision,
    /// Detects critical tool calls or command executions with impact
    Execution,
    /// Detects unverified hypotheses or implicit assumptions
    Assumption,
    /// Detects requests for disambiguation or missing requirements
    Clarification,
}

impl ChallengeType {
    /// Returns a user-friendly display name.
    pub fn as_str(&self) -> &'static str {
        match self {
            ChallengeType::Contradiction => "contradiction",
            ChallengeType::Decision => "decision",
            ChallengeType::Execution => "execution",
            ChallengeType::Assumption => "assumption",
            ChallengeType::Clarification => "clarification",
        }
    }
}

/// Status of a HumanChallenge event
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeStatus {
    /// Discovered by scanner, waiting for human input
    Candidate,
    /// Answered by human
    Answered,
    /// Answer verified and points awarded
    Verified,
    /// Rejected or dismissed by user
    Rejected,
    /// Timed out without response
    Expired,
}

impl ChallengeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChallengeStatus::Candidate => "candidate",
            ChallengeStatus::Answered => "answered",
            ChallengeStatus::Verified => "verified",
            ChallengeStatus::Rejected => "rejected",
            ChallengeStatus::Expired => "expired",
        }
    }
}

impl FromStr for ChallengeStatus {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "answered" => ChallengeStatus::Answered,
            "verified" => ChallengeStatus::Verified,
            "rejected" => ChallengeStatus::Rejected,
            "expired" => ChallengeStatus::Expired,
            _ => ChallengeStatus::Candidate,
        })
    }
}

/// Structured HumanChallenge event stored locally in node's SQLite
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanChallengeEvent {
    pub id: String,
    pub session_id: String,
    pub challenge_type: ChallengeType,
    pub description: String,
    pub raw_content: String,
    pub confidence_score: f32,
    pub status: ChallengeStatus,
    pub created_at: DateTime<Utc>,
    pub answered_at: Option<DateTime<Utc>>,
    pub response: Option<String>,
    pub points_awarded: u32,
    /// Privacy P4 guarantee: true means full content remains local to this node
    pub privacy_p4_local_only: bool,
}

impl HumanChallengeEvent {
    /// Creates a new candidate challenge event with Privacy P4 default (local only).
    pub fn new(
        session_id: impl Into<String>,
        challenge_type: ChallengeType,
        description: impl Into<String>,
        raw_content: impl Into<String>,
        confidence_score: f32,
    ) -> Self {
        Self {
            id: format!("hc_{}", ulid::Ulid::new()),
            session_id: session_id.into(),
            challenge_type,
            description: description.into(),
            raw_content: raw_content.into(),
            confidence_score,
            status: ChallengeStatus::Candidate,
            created_at: Utc::now(),
            answered_at: None,
            response: None,
            points_awarded: 0,
            privacy_p4_local_only: true,
        }
    }
}

/// Monthly X2 Farming Summary (Target: 10 points/month for answered challenges)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FarmingSummary {
    pub year_month: String,
    pub total_points: u32,
    pub target_points: u32,
    pub answered_count: u32,
    pub verified_count: u32,
}

impl Default for FarmingSummary {
    fn default() -> Self {
        Self {
            year_month: Utc::now().format("%Y-%m").to_string(),
            total_points: 0,
            target_points: 10,
            answered_count: 0,
            verified_count: 0,
        }
    }
}

/// Privacy P4 Anonymous payload suitable for uploading to Mesh without sensitive text
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnonymousMeshScore {
    pub challenge_id_hash: String,
    pub challenge_type: ChallengeType,
    pub status: ChallengeStatus,
    pub timestamp: DateTime<Utc>,
    pub points: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_challenge_type_str() {
        assert_eq!(ChallengeType::Contradiction.as_str(), "contradiction");
        assert_eq!(ChallengeType::Decision.as_str(), "decision");
        assert_eq!(ChallengeType::Execution.as_str(), "execution");
        assert_eq!(ChallengeType::Assumption.as_str(), "assumption");
        assert_eq!(ChallengeType::Clarification.as_str(), "clarification");
    }

    #[test]
    fn test_human_challenge_event_new() {
        let event = HumanChallengeEvent::new(
            "session_123",
            ChallengeType::Decision,
            "Test Decision",
            "We decided to use SQLite",
            0.9,
        );

        assert!(event.id.starts_with("hc_"));
        assert_eq!(event.session_id, "session_123");
        assert_eq!(event.challenge_type, ChallengeType::Decision);
        assert_eq!(event.status, ChallengeStatus::Candidate);
        assert!(event.privacy_p4_local_only);
    }

    #[test]
    fn test_farming_summary_default() {
        let summary = FarmingSummary::default();
        assert_eq!(summary.target_points, 10);
        assert_eq!(summary.total_points, 0);
    }
}
