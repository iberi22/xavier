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

/// Verdict on a HumanChallenge curation vote
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurationVerdict {
    /// The fact/decision is accepted as-is for training
    Accept,
    /// The fact/decision is rejected — do not use for training
    Reject,
    /// The content was refined/corrected by the human
    Refine,
}

impl CurationVerdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            CurationVerdict::Accept => "accept",
            CurationVerdict::Reject => "reject",
            CurationVerdict::Refine => "refine",
        }
    }
}

impl std::str::FromStr for CurationVerdict {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "accept" => CurationVerdict::Accept,
            "reject" => CurationVerdict::Reject,
            _ => CurationVerdict::Refine,
        })
    }
}

/// A human's curation vote on a HumanChallenge event.
/// Accepted/refined votes with training_eligible=true feed the TrainingExporter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurationVote {
    pub id: String,
    pub challenge_id: String,
    pub verdict: CurationVerdict,
    /// Human-refined version of the content (if verdict=Refine)
    pub curated_content: Option<String>,
    /// True when the human explicitly verified the fact is correct
    pub fact_verified: bool,
    /// Domain tags for the mini-expert segment (e.g., ["rust", "architecture"])
    pub domain_tags: Vec<String>,
    /// Explicit consent to use this item for model training
    pub training_eligible: bool,
    pub voted_at: DateTime<Utc>,
}

impl CurationVote {
    pub fn new(
        challenge_id: impl Into<String>,
        verdict: CurationVerdict,
        curated_content: Option<String>,
        fact_verified: bool,
        domain_tags: Vec<String>,
        training_eligible: bool,
    ) -> Self {
        Self {
            id: format!("cv_{}", ulid::Ulid::new()),
            challenge_id: challenge_id.into(),
            verdict,
            curated_content,
            fact_verified,
            domain_tags,
            training_eligible,
            voted_at: Utc::now(),
        }
    }
}

/// Status of a human introspection session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntrospectionStatus {
    Active,
    Completed,
    Abandoned,
}

impl IntrospectionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            IntrospectionStatus::Active => "active",
            IntrospectionStatus::Completed => "completed",
            IntrospectionStatus::Abandoned => "abandoned",
        }
    }
}

impl std::str::FromStr for IntrospectionStatus {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "completed" => IntrospectionStatus::Completed,
            "abandoned" => IntrospectionStatus::Abandoned,
            _ => IntrospectionStatus::Active,
        })
    }
}

/// The 6 human introspection techniques that the LLM facilitates.
/// The LLM guides — the human produces the insight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntrospectionTechnique {
    /// LLM asks questions that lead the human to discover the answer
    SocraticQuestioning,
    /// Recursive "why?" 5 levels deep to find root cause
    FiveWhys,
    /// Pre-mortem: what could go wrong in 6 months?
    PreMortem,
    /// Argue the best case for the opposing position
    SteelManning,
    /// Decompose to the most fundamental axioms
    FirstPrinciples,
    /// Have you seen this pattern before? What happened?
    PatternRecognition,
}

impl IntrospectionTechnique {
    pub fn as_str(&self) -> &'static str {
        match self {
            IntrospectionTechnique::SocraticQuestioning => "socratic_questioning",
            IntrospectionTechnique::FiveWhys => "five_whys",
            IntrospectionTechnique::PreMortem => "pre_mortem",
            IntrospectionTechnique::SteelManning => "steel_manning",
            IntrospectionTechnique::FirstPrinciples => "first_principles",
            IntrospectionTechnique::PatternRecognition => "pattern_recognition",
        }
    }

    /// Returns the LLM system prompt template for this technique.
    pub fn system_prompt(&self, challenge_description: &str) -> String {
        match self {
            IntrospectionTechnique::SocraticQuestioning => format!(
                "You are a Socratic guide. The human is exploring: '{challenge_description}'. \
                 Ask one focused question at a time that helps them discover the answer themselves. \
                 Never give the answer directly. Build on their previous responses."
            ),
            IntrospectionTechnique::FiveWhys => format!(
                "You are facilitating a 5 Whys analysis on: '{challenge_description}'. \
                 Each turn, acknowledge the human's answer and ask 'And why is that?' or a variant. \
                 Track depth (1-5). At depth 5, synthesize the root cause."
            ),
            IntrospectionTechnique::PreMortem => format!(
                "You are running a Pre-Mortem exercise on: '{challenge_description}'. \
                 Ask the human to imagine it is 6 months from now and this decision failed. \
                 Guide them to identify specific failure modes, probabilities, and mitigations."
            ),
            IntrospectionTechnique::SteelManning => format!(
                "You are facilitating Steel Manning on: '{challenge_description}'. \
                 Ask the human to build the strongest possible case for the opposing view. \
                 Challenge weak arguments, push for the genuinely best version of the opposing position."
            ),
            IntrospectionTechnique::FirstPrinciples => format!(
                "You are guiding First Principles thinking on: '{challenge_description}'. \
                 Ask the human to break down assumptions one by one until they reach undeniable axioms. \
                 Then help them rebuild from those axioms."
            ),
            IntrospectionTechnique::PatternRecognition => format!(
                "You are guiding Pattern Recognition on: '{challenge_description}'. \
                 Ask the human if they have seen similar situations before. \
                 Guide them to identify the pattern structure, past outcomes, and how it applies now."
            ),
        }
    }

    /// Recommended technique for a given ChallengeType.
    pub fn recommend_for(ct: ChallengeType) -> Self {
        match ct {
            ChallengeType::Contradiction => IntrospectionTechnique::SteelManning,
            ChallengeType::Decision => IntrospectionTechnique::PreMortem,
            ChallengeType::Execution => IntrospectionTechnique::FirstPrinciples,
            ChallengeType::Assumption => IntrospectionTechnique::SocraticQuestioning,
            ChallengeType::Clarification => IntrospectionTechnique::PatternRecognition,
        }
    }
}

/// Role in an introspection conversation turn
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnRole {
    LlmGuide,
    Human,
}

/// A single exchange turn in an introspection session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntrospectionTurn {
    pub role: TurnRole,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

/// A complete introspection session — persisted locally, never leaves the node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntrospectionSession {
    pub id: String,
    pub challenge_id: String,
    pub technique: IntrospectionTechnique,
    /// Full turn-by-turn conversation (JSON-serialized)
    pub turns: Vec<IntrospectionTurn>,
    /// 0.0–1.0 how deep the analysis went (derived from turn count + content)
    pub depth_score: f32,
    /// Key insights extracted from the session by the LLM guide
    pub insights: Vec<String>,
    pub status: IntrospectionStatus,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl IntrospectionSession {
    pub fn new(challenge_id: impl Into<String>, technique: IntrospectionTechnique) -> Self {
        Self {
            id: format!("is_{}", ulid::Ulid::new()),
            challenge_id: challenge_id.into(),
            technique,
            turns: Vec::new(),
            depth_score: 0.0,
            insights: Vec::new(),
            status: IntrospectionStatus::Active,
            started_at: Utc::now(),
            completed_at: None,
        }
    }

    /// Compute depth score based on turn count and average response length.
    pub fn compute_depth_score(&self) -> f32 {
        let human_turns: Vec<&IntrospectionTurn> = self
            .turns
            .iter()
            .filter(|t| t.role == TurnRole::Human)
            .collect();

        if human_turns.is_empty() {
            return 0.0;
        }

        let turn_factor = (human_turns.len() as f32 / 5.0).min(1.0);
        let avg_len: f32 = human_turns
            .iter()
            .map(|t| t.content.len() as f32)
            .sum::<f32>()
            / human_turns.len() as f32;
        let length_factor = (avg_len / 200.0).min(1.0);

        ((turn_factor + length_factor) / 2.0).clamp(0.0, 1.0)
    }
}

/// Gate that decides if curated challenges are ready to feed the TrainingExporter.
#[derive(Debug, Clone)]
pub struct TrainingReadinessGate {
    /// Minimum accepted curation votes needed to trigger a training bundle
    pub min_accepted: usize,
    /// Minimum fraction of votes that must have fact_verified=true
    pub min_fact_verified_ratio: f32,
    /// Minimum fraction of votes with training_eligible=true
    pub min_training_eligible_ratio: f32,
}

impl Default for TrainingReadinessGate {
    fn default() -> Self {
        Self {
            min_accepted: 20,
            min_fact_verified_ratio: 0.7,
            min_training_eligible_ratio: 0.8,
        }
    }
}

impl TrainingReadinessGate {
    pub fn is_ready(&self, votes: &[CurationVote]) -> bool {
        let accepted: Vec<&CurationVote> = votes
            .iter()
            .filter(|v| {
                v.verdict == CurationVerdict::Accept || v.verdict == CurationVerdict::Refine
            })
            .collect();

        if accepted.len() < self.min_accepted {
            return false;
        }

        let fact_verified_count = accepted.iter().filter(|v| v.fact_verified).count();
        let training_eligible_count = accepted.iter().filter(|v| v.training_eligible).count();

        let fact_ratio = fact_verified_count as f32 / accepted.len() as f32;
        let eligible_ratio = training_eligible_count as f32 / accepted.len() as f32;

        fact_ratio >= self.min_fact_verified_ratio
            && eligible_ratio >= self.min_training_eligible_ratio
    }
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
