//! RewardEngine — Calculates and distributes XP rewards for mesh contributions.
//!
//! When a node contributes storage, bandwidth, compute, or other resources,
//! the RewardEngine determines the XP award, enforces daily caps, and credits
//! the node's wallet.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{bail, Result};
use chrono::Utc;
use tokio::sync::Mutex;
use uuid::Uuid;

use super::wallet::{TransactionKind, Wallet};

// ---------------------------------------------------------------------------
// ContributionType — What kind of resource the node contributed
// ---------------------------------------------------------------------------

/// The type of resource contribution a node made to the mesh.
#[derive(Debug, Clone)]
pub enum ContributionType {
    /// Provided storage space over a period of time
    StorageProvided {
        /// Bytes contributed
        bytes: u64,
        /// Duration the storage was maintained (seconds)
        duration_secs: u64,
    },
    /// Provided bandwidth for data transfer
    BandwidthProvided {
        /// Bytes transferred
        bytes: u64,
    },
    /// Provided computational cycles
    ComputeProvided {
        /// Approximate CPU cycles contributed
        cycles: u64,
    },
    /// Discovered and connected new peers
    PeerDiscovery {
        /// Number of new peers connected
        peers_connected: u32,
    },
    /// Validated data records (e.g. consensus participation)
    DataValidated {
        /// Number of records validated
        records: u32,
    },
}

// ---------------------------------------------------------------------------
// RewardEvent — A record of a single XP reward distribution
// ---------------------------------------------------------------------------

/// Records a single reward event that occurred.
#[derive(Debug, Clone)]
pub struct RewardEvent {
    /// Unique event identifier
    pub event_id: Uuid,
    /// The contribution that triggered the reward
    pub contribution: ContributionType,
    /// Amount of XP awarded
    pub xp_awarded: u64,
    /// Unix timestamp of the reward
    pub timestamp: i64,
}

// ---------------------------------------------------------------------------
// RewardEngine — Core reward calculation and distribution
// ---------------------------------------------------------------------------

/// Controls XP reward calculations, enforces daily caps, and credits wallets.
pub struct RewardEngine {
    /// The node's wallet (shared for concurrent access)
    pub wallet: Arc<Mutex<Wallet>>,
    /// XP awarded per unit of contribution
    pub reward_rate: f64,
    /// Maximum XP that can be earned in a single day
    pub daily_cap: u64,
    /// XP already earned today (atomic for lock-free reads)
    today_earned: AtomicU64,
    /// Unix timestamp of the last daily reset
    last_reset: AtomicI64,
}

impl RewardEngine {
    /// Create a new reward engine with default settings.
    ///
    /// Defaults: reward_rate = 1.0, daily_cap = 10_000 XP.
    pub fn new(wallet: Arc<Mutex<Wallet>>) -> Self {
        let now = Utc::now().timestamp();
        RewardEngine {
            wallet,
            reward_rate: 1.0,
            daily_cap: 10_000,
            today_earned: AtomicU64::new(0),
            last_reset: AtomicI64::new(now),
        }
    }

    /// Set a custom reward rate (XP per contribution unit).
    pub fn with_rate(mut self, rate: f64) -> Self {
        self.reward_rate = rate;
        self
    }

    /// Set a custom daily XP cap.
    pub fn with_daily_cap(mut self, cap: u64) -> Self {
        self.daily_cap = cap;
        self
    }

    /// Calculate the XP reward for a given contribution without applying it.
    pub fn calculate_reward(
        &self,
        contribution: &ContributionType,
        tier: super::wallet::InvestmentTier,
    ) -> u64 {
        let apy_multiplier = tier.apy() / 5.0; // Base is 5.0

        let raw = match contribution {
            ContributionType::StorageProvided {
                bytes,
                duration_secs,
            } => {
                (*bytes as f64 * *duration_secs as f64) / 1_000_000.0
                    * self.reward_rate
                    * apy_multiplier
            }
            ContributionType::BandwidthProvided { bytes } => {
                *bytes as f64 / 1_000_000.0 * self.reward_rate * apy_multiplier
            }
            ContributionType::ComputeProvided { cycles } => {
                *cycles as f64 / 100_000.0 * self.reward_rate * apy_multiplier
            }
            ContributionType::PeerDiscovery { peers_connected } => {
                *peers_connected as f64 * 10.0 * apy_multiplier
            }
            ContributionType::DataValidated { records } => {
                *records as f64 * 5.0 * apy_multiplier
            }
        };
        // Floor to u64, minimum 1 XP for non-zero contributions
        if raw > 0.0 {
            (raw.floor() as u64).max(1)
        } else {
            0
        }
    }

    /// Check and perform daily reset if the day has changed.
    pub fn daily_reset(&self) {
        let now = Utc::now().timestamp();
        let last = self.last_reset.load(Ordering::Relaxed);

        // If more than 24 hours have passed, reset the daily counter
        if now - last > 86_400 {
            self.today_earned.store(0, Ordering::Relaxed);
            self.last_reset.store(now, Ordering::Relaxed);
            tracing::debug!("🔄 RewardEngine: daily XP counter reset");
        }
    }

    /// Calculate the reward, check the daily cap, and credit the wallet.
    ///
    /// Returns a `RewardEvent` on success.
    pub async fn process_contribution(
        &self,
        contribution: ContributionType,
        description: &str,
    ) -> Result<RewardEvent> {
        // Ensure daily cap is fresh
        self.daily_reset();

        let (tier, lifetime_earned) = {
            let wallet = self.wallet.lock().await;
            (wallet.balance.tier, wallet.balance.lifetime_earned)
        };

        // Economy: 2% Annual Inflation (placeholder logic)
        // Adjust reward based on how long the system has been running or total supply
        // For now, we simulate a slight inflation adjustment based on lifetime_earned
        let inflation_adj = 1.0 + (lifetime_earned as f64 / 10_000_000.0).min(0.02);

        let xp = (self.calculate_reward(&contribution, tier) as f64 * inflation_adj) as u64;
        if xp == 0 {
            bail!("Contribution too small to earn XP");
        }

        // Check daily cap
        let earned_today = self.today_earned.load(Ordering::Relaxed);
        if earned_today >= self.daily_cap {
            bail!(
                "Daily XP cap reached ({} / {}). Try again tomorrow.",
                earned_today,
                self.daily_cap
            );
        }

        // Cap the reward if it would exceed the daily limit
        let available = self.daily_cap - earned_today;
        let xp_awarded = xp.min(available);

        if xp_awarded == 0 {
            bail!("No XP available under daily cap");
        }

        // Credit the wallet, applying the 5% burn rate if applicable
        {
            let mut wallet = self.wallet.lock().await;

            // Economy: Apply 5% protocol burn if it's a high-value reward or based on economy settings
            // For now, we simulate the burn by reducing the credited amount
            let burn_amount = (xp_awarded as f64 * 0.05) as u64;
            let net_xp = xp_awarded.saturating_sub(burn_amount);

            wallet.credit(net_xp, TransactionKind::Reward, description);

            if burn_amount > 0 {
                tracing::debug!(burn = burn_amount, "🔥 Economy: burned XP from reward");
                // In a real implementation, we would record this in the EconomyEngine or a global ledger
            }
        }

        // Update atomic counter
        self.today_earned.fetch_add(xp_awarded, Ordering::Relaxed);

        let event = RewardEvent {
            event_id: Uuid::new_v4(),
            contribution,
            xp_awarded,
            timestamp: Utc::now().timestamp(),
        };

        tracing::debug!(
            xp = xp_awarded,
            cap_remaining = self.daily_cap - self.today_earned.load(Ordering::Relaxed),
            "🎁 RewardEngine: awarded XP"
        );

        Ok(event)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup_engine() -> (RewardEngine, Arc<Mutex<Wallet>>) {
        let node_id = crate::mesh::node::NodeId::parse("xv1-testreward00001").unwrap();
        let wallet = Arc::new(Mutex::new(Wallet::new(node_id)));
        let engine = RewardEngine::new(wallet.clone())
            .with_rate(1.0)
            .with_daily_cap(100_000);
        (engine, wallet)
    }

    #[tokio::test]
    async fn test_storage_reward_calculation() {
        let (engine, _) = setup_engine().await;
        // 1 MB stored for 1000 seconds → 1_000_000 * 1000 / 1_000_000 * 1.0 = 1000 XP
        let contrib = ContributionType::StorageProvided {
            bytes: 1_000_000,
            duration_secs: 1000,
        };
        let xp = engine.calculate_reward(&contrib, super::super::wallet::InvestmentTier::Base);
        assert_eq!(xp, 1000);
    }

    #[tokio::test]
    async fn test_bandwidth_reward_calculation() {
        let (engine, _) = setup_engine().await;
        // 10 MB transferred → 10_000_000 / 1_000_000 * 1.0 = 10 XP
        let contrib = ContributionType::BandwidthProvided { bytes: 10_000_000 };
        let xp = engine.calculate_reward(&contrib, super::super::wallet::InvestmentTier::Base);
        assert_eq!(xp, 10);
    }

    #[tokio::test]
    async fn test_compute_reward_calculation() {
        let (engine, _) = setup_engine().await;
        // 500_000 cycles → 500_000 / 100_000 * 1.0 = 5 XP
        let contrib = ContributionType::ComputeProvided { cycles: 500_000 };
        let xp = engine.calculate_reward(&contrib, super::super::wallet::InvestmentTier::Base);
        assert_eq!(xp, 5);
    }

    #[tokio::test]
    async fn test_peer_discovery_flat_reward() {
        let (engine, _) = setup_engine().await;
        let contrib = ContributionType::PeerDiscovery { peers_connected: 3 };
        let xp = engine.calculate_reward(&contrib, super::super::wallet::InvestmentTier::Base);
        assert_eq!(xp, 30); // 3 * 10
    }

    #[tokio::test]
    async fn test_data_validated_flat_reward() {
        let (engine, _) = setup_engine().await;
        let contrib = ContributionType::DataValidated { records: 7 };
        let xp = engine.calculate_reward(&contrib, super::super::wallet::InvestmentTier::Base);
        assert_eq!(xp, 35); // 7 * 5
    }

    #[tokio::test]
    async fn test_process_contribution_credits_wallet() {
        let (engine, wallet) = setup_engine().await;
        let contrib = ContributionType::DataValidated { records: 10 };

        let event = engine
            .process_contribution(contrib, "Validated 10 records")
            .await
            .unwrap();

        // 50 XP awarded, but 5% burn applied (50 - 2 = 48)
        assert_eq!(event.xp_awarded, 50);

        let w = wallet.lock().await;
        assert_eq!(w.balance.xp_balance, 48);
    }

    #[tokio::test]
    async fn test_daily_cap_enforced() {
        let (mut engine, _) = setup_engine().await;
        // Force the engine to a tiny daily cap for testing
        engine.daily_cap = 5; // Override for test only

        let contrib = ContributionType::DataValidated { records: 10 }; // 50 XP normally

        let result = engine
            .process_contribution(contrib, "Should be capped")
            .await;
        assert!(result.is_ok());
        let event = result.unwrap();
        assert_eq!(event.xp_awarded, 5); // capped to daily cap

        // Next attempt should fail
        let contrib2 = ContributionType::DataValidated { records: 1 };
        let result2 = engine.process_contribution(contrib2, "Should fail").await;
        assert!(result2.is_err());
        assert!(result2
            .unwrap_err()
            .to_string()
            .contains("Daily XP cap reached"));
    }

    #[tokio::test]
    async fn test_tiny_contribution_returns_minimum_reward() {
        let (engine, _) = setup_engine().await;
        let contrib = ContributionType::StorageProvided {
            bytes: 1,
            duration_secs: 1,
        };
        let xp = engine.calculate_reward(&contrib, super::super::wallet::InvestmentTier::Base);
        assert_eq!(xp, 1); // non-zero contributions receive the minimum reward
    }

    #[tokio::test]
    async fn test_daily_reset_clears_counter() {
        let (engine, _) = setup_engine().await;
        engine.today_earned.store(50_000, Ordering::Relaxed);

        // Set last_reset to 25 hours ago to trigger reset
        let old_ts = Utc::now().timestamp() - 90_000; // 25 hours ago
        engine.last_reset.store(old_ts, Ordering::Relaxed);

        engine.daily_reset();

        assert_eq!(engine.today_earned.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_custom_rate_affects_reward() {
        let node_id = crate::mesh::node::NodeId::parse("xv1-testrate000001").unwrap();
        let wallet = Arc::new(Mutex::new(Wallet::new(node_id)));
        let engine = RewardEngine::new(wallet).with_rate(2.5);

        // 1 MB stored for 1000s → 1_000_000 * 1000 / 1_000_000 * 2.5 = 2500 XP
        let contrib = ContributionType::StorageProvided {
            bytes: 1_000_000,
            duration_secs: 1000,
        };
        assert_eq!(engine.calculate_reward(&contrib, super::super::wallet::InvestmentTier::Base), 2500);
    }
}
