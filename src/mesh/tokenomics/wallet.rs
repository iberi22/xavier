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
// Multisig Wallet — Multi-signature consensus tracking
// ---------------------------------------------------------------------------

/// A wallet or contract address.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Address(pub String);

impl Address {
    /// Create a new Address from a string.
    pub fn new(addr: impl Into<String>) -> Self {
        Address(addr.into())
    }
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for Address {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for Address {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Transaction ID wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TxId(pub Uuid);

/// Action proposed for execution on the multisig treasury.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MultisigProposal {
    /// Transfer XP from treasury to another address
    Transfer {
        /// Target wallet address
        to: Address,
        /// Amount of XP to transfer
        amount: u64,
        /// Description of transfer purpose
        description: String,
    },
    /// Move XP into staking lockups
    Stake {
        /// Amount to stake
        amount: u64,
        /// Duration of stake lockup
        duration_days: u64,
    },
    /// Reclaim staked XP
    Unstake {
        /// Amount to unstake
        amount: u64,
    },
}

/// A transaction proposal tracking current signatures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultisigTransaction {
    /// ID of the transaction
    pub id: TxId,
    /// The action proposed
    pub proposal: MultisigProposal,
    /// Owners who have signed off on this transaction
    pub signatures: std::collections::HashSet<Address>,
    /// Whether the transaction has already been executed
    pub executed: bool,
}

/// A Multi-Signature wallet tracking owner consensus and execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultisigWallet {
    /// Unique address for the multisig wallet
    pub address: Address,
    /// List of addresses that own this wallet
    pub owners: Vec<Address>,
    /// Minimum threshold of signatures required to execute a transaction
    pub threshold: u8,
    /// Underlying standard wallet tracking balance and transactions
    pub wallet: Wallet,
    /// History and status of proposed transactions
    pub transactions: std::collections::HashMap<TxId, MultisigTransaction>,
}

impl MultisigWallet {
    /// Create a new MultisigWallet with owners and signature threshold.
    pub fn new(owners: Vec<Address>, threshold: u8) -> Self {
        let unique_id = Uuid::new_v4();
        let address = Address::new(format!("multisig_{}", &unique_id.to_string()[..8]));
        let node_id = NodeId::parse("xv1-multisig0000000").unwrap();
        Self {
            address,
            owners,
            threshold,
            wallet: Wallet::new(node_id),
            transactions: std::collections::HashMap::new(),
        }
    }

    /// Submit a new transaction proposal. The submitter counts as the first signer.
    pub fn submit_tx(&mut self, tx: MultisigProposal, signer: Address) -> Result<TxId> {
        submit_tx(self, tx, signer)
    }

    /// Sign an existing, unexecuted transaction proposal.
    pub fn sign_tx(&mut self, tx_id: TxId, signer: Address) -> Result<()> {
        sign_tx(self, tx_id, signer)
    }

    /// Execute a transaction proposal once the signature threshold has been reached.
    pub fn execute_tx(&mut self, tx_id: TxId) -> Result<()> {
        execute_tx(self, tx_id)
    }
}

// Global registry of all multisig wallets
static MULTISIG_REGISTRY: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<Address, MultisigWallet>>,
> = std::sync::OnceLock::new();

fn get_registry() -> &'static std::sync::Mutex<std::collections::HashMap<Address, MultisigWallet>> {
    MULTISIG_REGISTRY.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Create a new multisig wallet and register it in the global registry.
pub fn create_multisig(owners: Vec<Address>, threshold: u8) -> Address {
    let wallet = MultisigWallet::new(owners, threshold);
    let address = wallet.address.clone();
    let mut registry = get_registry().lock().unwrap();
    registry.insert(address.clone(), wallet);
    address
}

/// Retrieve a copy of a multisig wallet from the registry by its address.
pub fn get_multisig_wallet(address: &Address) -> Option<MultisigWallet> {
    let registry = get_registry().lock().unwrap();
    registry.get(address).cloned()
}

/// Submit a transaction proposal to a multisig wallet.
pub fn submit_tx(
    wallet: &mut MultisigWallet,
    tx: MultisigProposal,
    signer: Address,
) -> Result<TxId> {
    if !wallet.owners.contains(&signer) {
        bail!("Signer is not an owner of this multisig wallet");
    }
    let tx_id = TxId(Uuid::new_v4());
    let mut signatures = std::collections::HashSet::new();
    signatures.insert(signer);

    let multisig_tx = MultisigTransaction {
        id: tx_id,
        proposal: tx,
        signatures,
        executed: false,
    };

    wallet.transactions.insert(tx_id, multisig_tx);

    // Sync to global registry if registered
    let mut registry = get_registry().lock().unwrap();
    if registry.contains_key(&wallet.address) {
        registry.insert(wallet.address.clone(), wallet.clone());
    }

    Ok(tx_id)
}

/// Sign a transaction proposal in a multisig wallet.
pub fn sign_tx(wallet: &mut MultisigWallet, tx_id: TxId, signer: Address) -> Result<()> {
    if !wallet.owners.contains(&signer) {
        bail!("Signer is not an owner of this multisig wallet");
    }
    let tx = wallet
        .transactions
        .get_mut(&tx_id)
        .context("Transaction not found")?;
    if tx.executed {
        bail!("Transaction already executed");
    }
    tx.signatures.insert(signer);

    // Sync to global registry if registered
    let mut registry = get_registry().lock().unwrap();
    if registry.contains_key(&wallet.address) {
        registry.insert(wallet.address.clone(), wallet.clone());
    }

    Ok(())
}

/// Execute a transaction proposal if the threshold is met.
pub fn execute_tx(wallet: &mut MultisigWallet, tx_id: TxId) -> Result<()> {
    let (proposal, sig_count) = {
        let tx = wallet
            .transactions
            .get(&tx_id)
            .context("Transaction not found")?;
        if tx.executed {
            bail!("Transaction already executed");
        }
        let count = tx.signatures.len();
        (tx.proposal.clone(), count)
    };

    if sig_count < wallet.threshold as usize {
        bail!(
            "Threshold not reached: have {}, need {}",
            sig_count,
            wallet.threshold
        );
    }

    // Execute proposal actions on the underlying wallet
    match &proposal {
        MultisigProposal::Transfer {
            to,
            amount,
            description,
        } => {
            wallet
                .wallet
                .debit(*amount, TransactionKind::Redemption, description)?;

            // If the target address is in the global registry, credit it
            let mut registry = get_registry().lock().unwrap();
            if let Some(target_wallet) = registry.get_mut(to) {
                target_wallet
                    .wallet
                    .credit(*amount, TransactionKind::Reward, description);
            }
        }
        MultisigProposal::Stake {
            amount,
            duration_days,
        } => {
            wallet.wallet.stake(*amount, *duration_days)?;
        }
        MultisigProposal::Unstake { amount } => {
            wallet.wallet.unstake(*amount)?;
        }
    }

    // Mark as executed
    let tx = wallet
        .transactions
        .get_mut(&tx_id)
        .context("Transaction not found")?;
    tx.executed = true;

    // Sync to global registry if registered
    let mut registry = get_registry().lock().unwrap();
    if registry.contains_key(&wallet.address) {
        registry.insert(wallet.address.clone(), wallet.clone());
    }

    Ok(())
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

    #[test]
    fn multisig_requires_threshold() {
        let owner1 = Address::new("owner1");
        let owner2 = Address::new("owner2");
        let owner3 = Address::new("owner3");
        let mut wallet =
            MultisigWallet::new(vec![owner1.clone(), owner2.clone(), owner3.clone()], 2);

        // Fund standard wallet balance
        wallet
            .wallet
            .credit(1000, TransactionKind::Reward, "Initial funding");

        let tx = MultisigProposal::Transfer {
            to: Address::new("recipient"),
            amount: 400,
            description: "Transfer 400 XP".to_string(),
        };

        // Submit proposal
        let tx_id = submit_tx(&mut wallet, tx, owner1.clone()).unwrap();

        // Attempting to execute with only 1 signature should fail
        let res = execute_tx(&mut wallet, tx_id);
        assert!(res.is_err());
        assert_eq!(wallet.wallet.balance.xp_balance, 1000);
    }

    #[test]
    fn multisig_executes_after_threshold() {
        let owner1 = Address::new("owner1");
        let owner2 = Address::new("owner2");
        let owner3 = Address::new("owner3");
        let mut wallet =
            MultisigWallet::new(vec![owner1.clone(), owner2.clone(), owner3.clone()], 2);

        // Fund standard wallet balance
        wallet
            .wallet
            .credit(1000, TransactionKind::Reward, "Initial funding");

        let tx = MultisigProposal::Transfer {
            to: Address::new("recipient"),
            amount: 400,
            description: "Transfer 400 XP".to_string(),
        };

        // Submit proposal
        let tx_id = submit_tx(&mut wallet, tx, owner1.clone()).unwrap();

        // Sign with owner2
        sign_tx(&mut wallet, tx_id, owner2.clone()).unwrap();

        // Executing should now succeed
        execute_tx(&mut wallet, tx_id).unwrap();
        assert_eq!(wallet.wallet.balance.xp_balance, 600);

        // Transaction is marked executed
        let tx_record = wallet.transactions.get(&tx_id).unwrap();
        assert!(tx_record.executed);
    }

    #[test]
    fn non_owner_cannot_submit() {
        let owner1 = Address::new("owner1");
        let owner2 = Address::new("owner2");
        let mut wallet = MultisigWallet::new(vec![owner1.clone(), owner2.clone()], 2);

        let non_owner = Address::new("intruder");
        let tx = MultisigProposal::Transfer {
            to: Address::new("recipient"),
            amount: 100,
            description: "Unauthorized".to_string(),
        };

        let res = submit_tx(&mut wallet, tx, non_owner);
        assert!(res.is_err());
    }

    #[test]
    fn test_create_multisig_registry_and_transfer() {
        let owner1 = Address::new("owner1");
        let owner2 = Address::new("owner2");

        let multisig_addr = create_multisig(vec![owner1.clone(), owner2.clone()], 2);
        let recipient_addr = create_multisig(vec![owner1.clone()], 1);

        // Get multisig wallet from registry and fund it
        {
            let mut registry = get_registry().lock().unwrap();
            let wallet = registry.get_mut(&multisig_addr).unwrap();
            wallet.wallet.credit(1000, TransactionKind::Reward, "Fund");
        }

        // Re-fetch
        let mut wallet = get_multisig_wallet(&multisig_addr).unwrap();
        let tx = MultisigProposal::Transfer {
            to: recipient_addr.clone(),
            amount: 300,
            description: "Treasury reward".to_string(),
        };

        let tx_id = wallet.submit_tx(tx, owner1.clone()).unwrap();
        wallet.sign_tx(tx_id, owner2.clone()).unwrap();
        wallet.execute_tx(tx_id).unwrap();

        // Check local
        assert_eq!(wallet.wallet.balance.xp_balance, 700);

        // Check recipient in registry got funded
        let recipient_wallet = get_multisig_wallet(&recipient_addr).unwrap();
        assert_eq!(recipient_wallet.wallet.balance.xp_balance, 300);
    }
}
