//! Resource Accounting and Reputation for Data Commons.
//!
//! Tracks resource sharing (storage, bandwidth, compute) between mesh peers
//! and calculates reputation scores to incentivize contributions and penalize
//! freeloading.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use crate::mesh::node::NodeId;

/// Individual peer's resource accounting and reputation state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerAccount {
    /// Bytes of storage provided to the mesh
    pub storage_contributed: u64,
    /// Bytes of storage consumed from the mesh
    pub storage_consumed: u64,
    /// Bytes of bandwidth provided for mesh transfers
    pub bandwidth_contributed: u64,
    /// Bytes of bandwidth consumed from mesh transfers
    pub bandwidth_consumed: u64,
    /// CPU cycles provided for mesh computations
    pub compute_contributed: u64,
    /// CPU cycles consumed from mesh computations
    pub compute_consumed: u64,
    /// Number of high-quality data contributions validated by peers
    pub quality_contributions: u32,
    /// Manual reputation penalties (e.g. for protocol violations)
    pub manual_penalties: u32,
    /// Current reputation score (0-1000)
    pub reputation_score: u32,
}

impl Default for PeerAccount {
    fn default() -> Self {
        Self::new()
    }
}

impl PeerAccount {
    /// Create a new account with neutral reputation.
    pub fn new() -> Self {
        Self {
            storage_contributed: 0,
            storage_consumed: 0,
            bandwidth_contributed: 0,
            bandwidth_consumed: 0,
            compute_contributed: 0,
            compute_consumed: 0,
            quality_contributions: 0,
            manual_penalties: 0,
            reputation_score: 500, // Initial neutral reputation
        }
    }

    /// Calculate a ratio-based score for a resource (0.0 to 1.0).
    /// Returns 0.625 for neutral state so that base_score is 500.
    fn resource_ratio_score(contributed: u64, consumed: u64) -> f64 {
        if consumed == 0 {
            if contributed > 0 { 1.0 } else { 0.625 }
        } else {
            let ratio = contributed as f64 / consumed as f64;
            ratio.min(1.0)
        }
    }

    /// Update reputation score based on current metrics.
    ///
    /// The score is composed of:
    /// - 80% Resource sharing ratio (storage, bandwidth, compute)
    /// - 20% Quality contribution bonus
    /// - Minus manual penalties
    pub fn update_reputation(&mut self) {
        let s_score = Self::resource_ratio_score(self.storage_contributed, self.storage_consumed);
        let b_score = Self::resource_ratio_score(self.bandwidth_contributed, self.bandwidth_consumed);
        let c_score = Self::resource_ratio_score(self.compute_contributed, self.compute_consumed);

        // Quality bonus: 5 points per quality contribution, capped at 200
        let quality_bonus = (self.quality_contributions * 5).min(200);

        // Base score is average of resource ratios (scaled to 800)
        let base_score = ((s_score + b_score + c_score) / 3.0 * 800.0) as u32;

        self.reputation_score = (base_score + quality_bonus)
            .saturating_sub(self.manual_penalties)
            .min(1000);
    }
}

/// Global resource accounting system for the mesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceAccounting {
    /// Map of NodeId to their respective accounts
    pub accounts: HashMap<NodeId, PeerAccount>,
    /// Minimum reputation required to avoid consumption penalties
    pub penalty_threshold: u32,
}

impl Default for ResourceAccounting {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceAccounting {
    /// Create a new resource accounting manager.
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            penalty_threshold: 300,
        }
    }

    /// Get or create a peer's account.
    pub fn get_account_mut(&mut self, node_id: &NodeId) -> &mut PeerAccount {
        self.accounts.entry(node_id.clone()).or_insert_with(PeerAccount::new)
    }

    /// Record a resource contribution and update reputation.
    pub fn record_contribution(&mut self, node_id: &NodeId, storage: u64, bandwidth: u64, compute: u64) {
        let acc = self.get_account_mut(node_id);
        acc.storage_contributed += storage;
        acc.bandwidth_contributed += bandwidth;
        acc.compute_contributed += compute;
        acc.update_reputation();
    }

    /// Record a resource consumption and update reputation.
    pub fn record_consumption(&mut self, node_id: &NodeId, storage: u64, bandwidth: u64, compute: u64) {
        let acc = self.get_account_mut(node_id);
        acc.storage_consumed += storage;
        acc.bandwidth_consumed += bandwidth;
        acc.compute_consumed += compute;
        acc.update_reputation();
    }

    /// Record a quality data contribution (incentive).
    pub fn record_quality_contribution(&mut self, node_id: &NodeId) {
        let acc = self.get_account_mut(node_id);
        acc.quality_contributions += 1;
        acc.update_reputation();
    }

    /// Check if a peer is a "freeloader" (below penalty threshold).
    pub fn is_freeloader(&self, node_id: &NodeId) -> bool {
        if let Some(acc) = self.accounts.get(node_id) {
            acc.reputation_score < self.penalty_threshold
        } else {
            false
        }
    }

    /// Apply a manual reputation penalty (e.g. for protocol violations).
    pub fn apply_penalty(&mut self, node_id: &NodeId, amount: u32) {
        let acc = self.get_account_mut(node_id);
        acc.manual_penalties += amount;
        acc.update_reputation();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_node_id() -> NodeId {
        NodeId::parse("xv1-testnode00000000000000000000").unwrap()
    }

    #[test]
    fn test_initial_reputation() {
        let acc = PeerAccount::new();
        assert_eq!(acc.reputation_score, 500);
    }

    #[test]
    fn test_neutral_update_stays_at_500() {
        let mut acc = PeerAccount::new();
        acc.update_reputation();
        assert_eq!(acc.reputation_score, 500);
    }

    #[test]
    fn test_contribution_increases_reputation() {
        let mut ra = ResourceAccounting::new();
        let node = test_node_id();

        // Initial 500
        ra.record_contribution(&node, 1000, 1000, 1000);
        // After contribution (1000 contrib / 0 consumed) -> ratios 1.0 -> 800 pts
        assert!(ra.get_account_mut(&node).reputation_score >= 800);
    }

    #[test]
    fn test_consumption_decreases_reputation() {
        let mut ra = ResourceAccounting::new();
        let node = test_node_id();

        ra.record_contribution(&node, 100, 100, 100);
        let high_rep = ra.get_account_mut(&node).reputation_score;

        ra.record_consumption(&node, 1000, 1000, 1000);
        let low_rep = ra.get_account_mut(&node).reputation_score;

        assert!(low_rep < high_rep);
    }

    #[test]
    fn test_quality_incentive() {
        let mut ra = ResourceAccounting::new();
        let node = test_node_id();

        ra.record_contribution(&node, 100, 100, 100);
        let base_rep = ra.get_account_mut(&node).reputation_score;

        ra.record_quality_contribution(&node);
        let boosted_rep = ra.get_account_mut(&node).reputation_score;

        assert_eq!(boosted_rep, base_rep + 5);
    }

    #[test]
    fn test_freeloader_detection() {
        let mut ra = ResourceAccounting::new();
        let node = test_node_id();

        // Consume heavily without contributing
        ra.record_consumption(&node, 1_000_000, 1_000_000, 1_000_000);

        assert!(ra.get_account_mut(&node).reputation_score < 300);
        assert!(ra.is_freeloader(&node));
    }

    #[test]
    fn test_manual_penalty_persists() {
        let mut ra = ResourceAccounting::new();
        let node = test_node_id();

        // 1. Record some activity to get a base reputation (e.g. 800)
        ra.record_contribution(&node, 100, 100, 100);
        let base_rep = ra.get_account_mut(&node).reputation_score;
        assert_eq!(base_rep, 800);

        // 2. Apply a penalty
        ra.apply_penalty(&node, 100);
        assert_eq!(ra.get_account_mut(&node).reputation_score, base_rep - 100);

        // 3. Record more activity, reputation should still reflect the penalty
        ra.record_contribution(&node, 100, 100, 100);
        // Without penalty, it would be 800. With penalty, it should be 700.
        assert_eq!(ra.get_account_mut(&node).reputation_score, 700);
        assert_eq!(ra.get_account_mut(&node).manual_penalties, 100);
    }
}
