//! HumanChallenge Module
//!
//! Implements HumanChallenge scanner (cron), SQLite event storage,
//! and 5 canonical challenge types for X2 farming.

pub mod cron;
pub mod curation_gate;
pub mod introspection;
pub mod scanner;
pub mod store;
pub mod types;

pub use cron::{HumanChallengeCron, HumanChallengeCronConfig};
pub use curation_gate::CurationGate;
pub use introspection::IntrospectionEngine;
pub use scanner::SessionScanner;
pub use store::HumanChallengeStore;
pub use types::{
    AnonymousMeshScore, ChallengeStatus, ChallengeType, CurationVerdict, CurationVote,
    FarmingSummary, HumanChallengeEvent, IntrospectionSession, IntrospectionStatus,
    IntrospectionTechnique, TurnRole,
};
