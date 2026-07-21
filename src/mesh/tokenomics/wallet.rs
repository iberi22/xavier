// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Wallet — XP balance tracking for Xavier Mesh node participation.
//!
//! Each node has a wallet that tracks its earned XP (Xavier Points), staked
//! XP, and transaction history. Wallets persist as JSON files and can be
//! loaded on node startup.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::mesh::node::NodeId;

// ---------------------------------------------------------------------------
// InvestmentTier — Progressive APY and vesting tiers
// ---------------------------------------------------------------------------

/// Defines the investment and reward tier for a mesh node.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, PartialOrd, Default)]
pub enum InvestmentTier {
    #[default]
    Base,
    Bronze,
    Silver,
    Gold,
    Platinum,
    Diamond,
    Sovereign,
}

impl InvestmentTier {
    /// Returns the APY as a percentage (e.g. 5.0 for 5%).
    pub fn apy(&self) -> f64 {
        match self {
            InvestmentTier::Base => 5.0,
            InvestmentTier::Bronze => 7.5,
            InvestmentTier::Silver => 10.0,
            InvestmentTier::Gold => 12.5,
            InvestmentTier::Platinum => 17.5,
            InvestmentTier::Diamond => 25.0,
            InvestmentTier::Sovereign => 40.0,
        }
    }

    /// Returns the minimum investment required in USD.
    pub fn min_investment_usd(&self) -> u64 {
        match self {
            InvestmentTier::Base => 0,
            InvestmentTier::Bronze => 1_000,
            InvestmentTier::Silver => 5_000,
            InvestmentTier::Gold => 10_000,
            InvestmentTier::Platinum => 25_000,
            InvestmentTier::Diamond => 50_000,
            InvestmentTier::Sovereign => 100_000,
        }
    }

    /// Returns the lock-up duration (cliff) in months.
    pub fn cliff_months(&self) -> u32 {
        match self {
            InvestmentTier::Base => 0,
            InvestmentTier::Bronze => 2,
            InvestmentTier::Silver => 4,
            InvestmentTier::Gold => 6,
            InvestmentTier::Platinum => 9,
            InvestmentTier::Diamond => 12,
            InvestmentTier::Sovereign => 18,
        }
    }

    /// Returns the month when 50% is released.
    pub fn release_50_month(&self) -> u32 {
        match self {
            InvestmentTier::Base => 0,
            InvestmentTier::Bronze => 2,
            InvestmentTier::Silver => 2,
            InvestmentTier::Gold => 3,
            InvestmentTier::Platinum => 4,
            InvestmentTier::Diamond => 6,
            InvestmentTier::Sovereign => 8,
        }
    }

    /// Returns the month when 100% is released.
    pub fn release_100_month(&self) -> u32 {
        match self {
            InvestmentTier::Base => 0,
            InvestmentTier::Bronze => 4,
            InvestmentTier::Silver => 6,
            InvestmentTier::Gold => 9,
            InvestmentTier::Platinum => 12,
            InvestmentTier::Diamond => 18,
            InvestmentTier::Sovereign => 24,
        }
    }
}

// ---------------------------------------------------------------------------
// TransactionKind — What type of operation generated this transaction
// ---------------------------------------------------------------------------

/// Classifies the purpose of a wallet transaction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TransactionKind {
    /// Reward for contributing resources to the mesh
    Reward,
    /// Transfer of XP to another node
    Transfer {
        /// Destination node ID
        to: NodeId,
    },
    /// XP placed into staking for a fixed duration
    Stake {
        /// Number of days the XP is locked
        duration_days: u64,
    },
    /// XP unstaked and returned to available balance
    Unstake,
    /// XP redeemed for a premium feature or conversion
    Redemption,
    /// Network or protocol fee deducted
    Fee,
}

// ---------------------------------------------------------------------------
// Transaction — A single ledger entry
// ---------------------------------------------------------------------------

/// A single entry in the wallet's transaction history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    /// Unique transaction identifier
    pub id: Uuid,
    /// Type of transaction
    pub kind: TransactionKind,
    /// Amount of XP involved
    pub amount: u64,
    /// Unix timestamp (seconds since epoch)
    pub timestamp: i64,
    /// Human-readable description
    pub description: String,
    /// Optional counterparty node (e.g. for transfers)
    pub counterparty: Option<NodeId>,
}

// ---------------------------------------------------------------------------
// VestingState — Tracks the status of investment vesting
// ---------------------------------------------------------------------------

/// Tracks the progressional release of invested capital.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VestingState {
    pub tier: InvestmentTier,
    pub amount_total: u64,
    pub amount_released: u64,
    pub start_timestamp: i64,
    pub last_claim_timestamp: i64,
}

// ---------------------------------------------------------------------------
// WalletBalance — The three tracked XP balances
// ---------------------------------------------------------------------------

/// The balances tracked for a single mesh node wallet.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WalletBalance {
    /// Spendable XP balance
    pub xp_balance: u64,
    /// XP currently locked in staking
    pub staked_xp: u64,
    /// Total XP earned over the entire lifetime of the node
    pub lifetime_earned: u64,
    /// Current investment tier
    pub tier: InvestmentTier,
}

// ---------------------------------------------------------------------------
// Wallet — The full wallet for a mesh node
// ---------------------------------------------------------------------------

/// A node's wallet, tracking XP balances and transaction history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallet {
    /// The node that owns this wallet
    pub node_id: NodeId,
    /// Current balances
    pub balance: WalletBalance,
    /// Investment vesting status
    pub vesting: Option<VestingState>,
    /// Ordered list of all transactions (newest first is convention)
    pub transactions: Vec<Transaction>,
    /// Unix timestamp when the wallet was created
    pub created_at: i64,
    /// Unix timestamp when the wallet was last updated
    pub last_updated: i64,
}

impl Wallet {
    /// Create a new empty wallet for the given node.
    pub fn new(node_id: NodeId) -> Self {
        let now = Utc::now().timestamp();
        Wallet {
            node_id,
            balance: WalletBalance::default(),
            vesting: None,
            transactions: Vec::new(),
            created_at: now,
            last_updated: now,
        }
    }

    /// Add XP to the wallet balance and record a transaction.
    pub fn credit(&mut self, amount: u64, kind: TransactionKind, description: &str) {
        self.balance.xp_balance += amount;
        self.balance.lifetime_earned += amount;
        self.last_updated = Utc::now().timestamp();
        self.transactions.push(Transaction {
            id: Uuid::new_v4(),
            kind,
            amount,
            timestamp: self.last_updated,
            description: description.to_string(),
            counterparty: None,
        });
    }

    /// Deduct XP from the wallet balance. Fails if insufficient funds.
    pub fn debit(&mut self, amount: u64, kind: TransactionKind, description: &str) -> Result<()> {
        if self.balance.xp_balance < amount {
            bail!(
                "Insufficient XP balance: have {}, need {}",
                self.balance.xp_balance,
                amount
            );
        }
        self.balance.xp_balance -= amount;
        self.last_updated = Utc::now().timestamp();
        self.transactions.push(Transaction {
            id: Uuid::new_v4(),
            kind,
            amount,
            timestamp: self.last_updated,
            description: description.to_string(),
            counterparty: None,
        });
        Ok(())
    }

    /// Move XP from available balance into staking.
    pub fn stake(&mut self, amount: u64, days: u64) -> Result<()> {
        if self.balance.xp_balance < amount {
            bail!(
                "Insufficient XP to stake: have {}, need {}",
                self.balance.xp_balance,
                amount
            );
        }
        self.balance.xp_balance -= amount;
        self.balance.staked_xp += amount;
        self.last_updated = Utc::now().timestamp();
        self.transactions.push(Transaction {
            id: Uuid::new_v4(),
            kind: TransactionKind::Stake {
                duration_days: days,
            },
            amount,
            timestamp: self.last_updated,
            description: format!("Staked {} XP for {} days", amount, days),
            counterparty: None,
        });
        Ok(())
    }

    /// Move XP from staking back to available balance.
    ///
    /// Applies a 5% fee on the unstaked amount.
    pub fn unstake(&mut self, amount: u64) -> Result<()> {
        if self.balance.staked_xp < amount {
            bail!(
                "Insufficient staked XP to unstake: have {}, need {}",
                self.balance.staked_xp,
                amount
            );
        }
        // 5% early-unstake / protocol fee
        let fee = (amount as f64 * 0.05).ceil() as u64;
        let net = amount - fee;

        self.balance.staked_xp -= amount;
        self.balance.xp_balance += net;
        self.last_updated = Utc::now().timestamp();

        self.transactions.push(Transaction {
            id: Uuid::new_v4(),
            kind: TransactionKind::Unstake,
            amount: net,
            timestamp: self.last_updated,
            description: format!("Unstaked {} XP (5% fee: {} XP)", amount, fee),
            counterparty: None,
        });

        if fee > 0 {
            self.transactions.push(Transaction {
                id: Uuid::new_v4(),
                kind: TransactionKind::Fee,
                amount: fee,
                timestamp: self.last_updated,
                description: format!("Protocol fee for unstaking: {} XP", fee),
                counterparty: None,
            });
        }

        Ok(())
    }

    /// Total XP under this wallet's control (available + staked).
    ///
    /// Note: staked XP is subject to lock periods and may not be immediately
    /// spendable.
    pub fn get_effective_balance(&self) -> u64 {
        self.balance.xp_balance + self.balance.staked_xp
    }

    /// Return the most recent `limit` transactions.
    pub fn list_transactions(&self, limit: usize) -> Vec<&Transaction> {
        self.transactions.iter().rev().take(limit).collect()
    }

    /// Serialize the wallet to a JSON string.
    pub fn save(&self) -> Result<String> {
        serde_json::to_string_pretty(self).context("Failed to serialize wallet to JSON")
    }

    /// Deserialize a wallet from a JSON string.
    pub fn load(json: &str) -> Result<Self> {
        serde_json::from_str(json).context("Failed to deserialize wallet from JSON")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_node_id() -> NodeId {
        NodeId::parse("xv1-testnode0000000").unwrap()
    }

    #[test]
    fn test_new_wallet_has_zero_balance() {
        let wallet = Wallet::new(test_node_id());
        assert_eq!(wallet.balance.xp_balance, 0);
        assert_eq!(wallet.balance.staked_xp, 0);
        assert_eq!(wallet.balance.lifetime_earned, 0);
    }

    #[test]
    fn test_credit_increases_balance_and_lifetime() {
        let mut wallet = Wallet::new(test_node_id());
        wallet.credit(100, TransactionKind::Reward, "Test reward");
        assert_eq!(wallet.balance.xp_balance, 100);
        assert_eq!(wallet.balance.lifetime_earned, 100);
        assert_eq!(wallet.transactions.len(), 1);
    }

    #[test]
    fn test_debit_decreases_balance() {
        let mut wallet = Wallet::new(test_node_id());
        wallet.credit(200, TransactionKind::Reward, "Initial");
        wallet.debit(50, TransactionKind::Fee, "Test fee").unwrap();
        assert_eq!(wallet.balance.xp_balance, 150);
        assert_eq!(wallet.transactions.len(), 2);
    }

    #[test]
    fn test_debit_insufficient_funds_fails() {
        let mut wallet = Wallet::new(test_node_id());
        wallet.credit(10, TransactionKind::Reward, "Small");
        let result = wallet.debit(100, TransactionKind::Fee, "Overdraft");
        assert!(result.is_err());
        assert_eq!(wallet.balance.xp_balance, 10); // unchanged
    }

    #[test]
    fn test_stake_moves_xp_to_staked() {
        let mut wallet = Wallet::new(test_node_id());
        wallet.credit(500, TransactionKind::Reward, "Funding");
        wallet.stake(200, 30).unwrap();
        assert_eq!(wallet.balance.xp_balance, 300);
        assert_eq!(wallet.balance.staked_xp, 200);
    }

    #[test]
    fn test_stake_insufficient_funds_fails() {
        let mut wallet = Wallet::new(test_node_id());
        wallet.credit(50, TransactionKind::Reward, "Small");
        assert!(wallet.stake(100, 7).is_err());
    }

    #[test]
    fn test_unstake_applies_fee() {
        let mut wallet = Wallet::new(test_node_id());
        wallet.credit(1000, TransactionKind::Reward, "Funding");
        wallet.stake(400, 30).unwrap();
        wallet.unstake(400).unwrap();
        // 400 - 5% = 380 returned; 20 fee
        assert_eq!(wallet.balance.staked_xp, 0);
        // original 600 + 380 = 980
        assert_eq!(wallet.balance.xp_balance, 980);
    }

    #[test]
    fn test_effective_balance_includes_staked() {
        let mut wallet = Wallet::new(test_node_id());
        wallet.credit(300, TransactionKind::Reward, "Funding");
        wallet.stake(100, 7).unwrap();
        assert_eq!(wallet.get_effective_balance(), 300);
        // spendable is 200, staked is 100
    }

    #[test]
    fn test_list_transactions_returns_newest_first() {
        let mut wallet = Wallet::new(test_node_id());
        wallet.credit(10, TransactionKind::Reward, "First");
        wallet.credit(20, TransactionKind::Reward, "Second");
        wallet.credit(30, TransactionKind::Reward, "Third");
        let recent = wallet.list_transactions(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].description, "Third");
        assert_eq!(recent[1].description, "Second");
    }

    #[test]
    fn test_save_load_roundtrip() {
        let mut wallet = Wallet::new(test_node_id());
        wallet.credit(777, TransactionKind::Reward, "Roundtrip test");
        let json = wallet.save().unwrap();
        let loaded = Wallet::load(&json).unwrap();
        assert_eq!(loaded.node_id, wallet.node_id);
        assert_eq!(loaded.balance.xp_balance, 777);
        assert_eq!(loaded.balance.lifetime_earned, 777);
        assert_eq!(loaded.transactions.len(), 1);
    }

    #[test]
    fn test_load_invalid_json_fails() {
        let result = Wallet::load("not valid json");
        assert!(result.is_err());
    }
}
