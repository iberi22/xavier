//! HumanChallenge Module
//!
//! Implements HumanChallenge scanner (cron), SQLite event storage,
//! and 5 canonical challenge types for X2 farming.

pub mod cron;
pub mod scanner;
pub mod store;
pub mod types;

pub use cron::{HumanChallengeCron, HumanChallengeCronConfig};
pub use scanner::SessionScanner;
pub use store::HumanChallengeStore;
pub use types::{
    AnonymousMeshScore, ChallengeStatus, ChallengeType, FarmingSummary, HumanChallengeEvent,
};
