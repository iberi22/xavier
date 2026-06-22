//! Tokenomics — XP-based incentive system for Xavier Mesh.
//!
//! This module implements a placeholder tokenomics system where nodes earn
//! XP (Xavier Points) for contributing resources (storage, bandwidth, compute)
//! to the mesh network. XP can be redeemed for premium features, priority
//! sync slots, or future token conversions.

pub mod wallet;
pub mod rewards;
pub mod accounting;
pub mod vesting;
pub mod economy;
#[cfg(feature = "dao-evm")]
pub mod contracts;
pub mod tests;

pub use wallet::{Wallet, WalletBalance, Transaction, TransactionKind, InvestmentTier};
pub use rewards::{RewardEngine, RewardEvent, ContributionType};
pub use accounting::{ResourceAccounting, PeerAccount};
