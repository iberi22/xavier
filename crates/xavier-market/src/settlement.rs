//! On-chain settlement and escrow integration model (MKT-21).
//!
//! # Scope & Owner Decisions
//! In v1, Xavier Marketplace balances run off-chain using non-transferable internal credits
//! (`Currency::InternalCredits`, `Settlement::OffChain`).
//! On-chain settlement and escrow (`MarketSettlement.sol`) is implemented in-repo
//! with Foundry tests and sits behind:
//! - Feature flag `market-v2`
//! - Runtime flag `XAVIER_MARKET_V2=beta`
//! - `market.settlement=on_chain` (rejected by default in v1 until ADR, Amoy soak period, and audit)
//! - `market.anchor_mode=dry_run` (anchoring runs in dry-run mode only by default)

use crate::config::AnchorMode;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_AMOY_CHAIN_ID: u64 = 80002;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscrowStatus {
    None,
    Deposited,
    Completed,
    Refunded,
    Disputed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EscrowRecord {
    pub order_id: String,
    pub listing_id: String,
    pub buyer: String,
    pub seller: String,
    pub token: Option<String>,
    pub amount: u64,
    pub fee_bps: u32,
    pub created_at_ms: i64,
    pub timeout_seconds: u64,
    pub status: EscrowStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SettlementAnchorPlan {
    pub merkle_root: String,
    pub leaf_count: usize,
    pub chain_id: u64,
    pub contract_address: Option<String>,
    pub dry_run: bool,
    pub calldata_hex: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SettlementError {
    #[error("on-chain settlement requires 4 weeks on Amoy and an audit before activation")]
    SettlementNotReady,
    #[error("live Amoy submission requires owner authorization and funded deployer")]
    LiveSubmissionRequiresOwnerGo,
    #[error("invalid order id: {0}")]
    InvalidOrderId(String),
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    #[error("invalid calldata format: {0}")]
    InvalidCalldata(String),
}

/// Selector for `anchorMerkleRoot(bytes32,uint256)`
/// keccak256("anchorMerkleRoot(bytes32,uint256)")[0..4] == 0x6f70d0fd
pub const SELECTOR_ANCHOR_MERKLE_ROOT: [u8; 4] = [0x6f, 0x70, 0xd0, 0xfd];

/// Encodes calldata for `anchorMerkleRoot(bytes32,uint256)`.
pub fn encode_anchor_merkle_root_calldata(
    merkle_root_hex: &str,
    leaf_count: usize,
) -> Result<Vec<u8>, SettlementError> {
    let raw = hex::decode(merkle_root_hex.trim_start_matches("0x"))
        .map_err(|e| SettlementError::InvalidCalldata(e.to_string()))?;
    if raw.len() != 32 {
        return Err(SettlementError::InvalidCalldata(format!(
            "merkle root must be 32 bytes, got {}",
            raw.len()
        )));
    }

    let mut data = Vec::with_capacity(4 + 32 + 32);
    data.extend_from_slice(&SELECTOR_ANCHOR_MERKLE_ROOT);
    data.extend_from_slice(&raw);

    let mut count_bytes = [0u8; 32];
    count_bytes[24..32].copy_from_slice(&(leaf_count as u64).to_be_bytes());
    data.extend_from_slice(&count_bytes);

    Ok(data)
}

/// Generates a settlement anchor plan for a given list of leaves under owner decisions.
/// Dry run mode is enforced by default.
pub fn plan_settlement_anchor(
    leaves: &[String],
    mode: AnchorMode,
    contract_address: Option<&str>,
) -> Result<SettlementAnchorPlan, SettlementError> {
    if mode == AnchorMode::LiveAmoy {
        return Err(SettlementError::LiveSubmissionRequiresOwnerGo);
    }

    let root = crate::merkle::merkle_root(leaves)
        .ok_or_else(|| SettlementError::InvalidConfig("empty or invalid leaves".to_string()))?;

    let calldata = encode_anchor_merkle_root_calldata(&root, leaves.len())?;

    Ok(SettlementAnchorPlan {
        merkle_root: root,
        leaf_count: leaves.len(),
        chain_id: DEFAULT_AMOY_CHAIN_ID,
        contract_address: contract_address.map(ToString::to_string),
        dry_run: true,
        calldata_hex: format!("0x{}", hex::encode(calldata)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anchor_mode_defaults_and_parse() {
        assert_eq!(AnchorMode::default(), AnchorMode::DryRun);
        assert_eq!(AnchorMode::parse("dry_run").unwrap(), AnchorMode::DryRun);
        assert_eq!(
            AnchorMode::parse("live_amoy").unwrap(),
            AnchorMode::LiveAmoy
        );
        assert!(AnchorMode::parse("mainnet").is_err());
    }

    #[test]
    fn test_live_amoy_rejected_by_default() {
        let leaves = vec!["01".to_string(), "02".to_string()];
        let err = plan_settlement_anchor(&leaves, AnchorMode::LiveAmoy, None).unwrap_err();
        assert_eq!(err, SettlementError::LiveSubmissionRequiresOwnerGo);
    }

    #[test]
    fn test_plan_settlement_anchor_dry_run() {
        let leaves = vec!["01".to_string(), "02".to_string()];
        let plan = plan_settlement_anchor(&leaves, AnchorMode::DryRun, Some("0x1234")).unwrap();

        assert_eq!(plan.leaf_count, 2);
        assert_eq!(plan.chain_id, DEFAULT_AMOY_CHAIN_ID);
        assert!(plan.dry_run);
        assert_eq!(plan.contract_address.as_deref(), Some("0x1234"));
        assert_eq!(
            plan.merkle_root,
            crate::merkle::merkle_root(&leaves).unwrap()
        );
        assert!(plan.calldata_hex.starts_with("0x6f70d0fd"));
        // 4 bytes selector + 32 bytes root + 32 bytes leaf count = 68 bytes = 136 hex chars + '0x' = 138
        assert_eq!(plan.calldata_hex.len(), 2 + 68 * 2);
    }

    #[test]
    fn test_encode_calldata_invalid_root() {
        assert!(encode_anchor_merkle_root_calldata("short", 5).is_err());
        assert!(encode_anchor_merkle_root_calldata("not-hex", 5).is_err());
    }
}
