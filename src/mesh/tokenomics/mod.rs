//! Tokenomics — XP-based incentive system for Xavier Mesh.
//!
//! This module implements a placeholder tokenomics system where nodes earn
//! XP (Xavier Points) for contributing resources (storage, bandwidth, compute)
//! to the mesh network. XP can be redeemed for premium features, priority
//! sync slots, or future token conversions.

pub mod wallet;
pub mod rewards;

pub use wallet::{Wallet, WalletBalance, Transaction, TransactionKind};
pub use rewards::{RewardEngine, RewardEvent, ContributionType};
