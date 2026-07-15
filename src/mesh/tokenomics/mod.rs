//! Tokenomics — XP-based incentive system for Xavier Mesh.
//!
//! This module implements a placeholder tokenomics system where nodes earn
//! XP (Xavier Points) for contributing resources (storage, bandwidth, compute)
//! to the mesh network. XP can be redeemed for premium features, priority
//! sync slots, or future token conversions.

pub mod accounting;
#[cfg(feature = "dao-evm")]
pub mod contracts;
pub mod economy;
pub mod rewards;
pub mod tests;
pub mod vesting;
pub mod wallet;

pub use accounting::{PeerAccount, ResourceAccounting};
pub use rewards::{ContributionType, RewardEngine, RewardEvent};
pub use wallet::{InvestmentTier, Transaction, TransactionKind, Wallet, WalletBalance};
