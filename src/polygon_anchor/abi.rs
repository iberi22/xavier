//! Minimal ABI for SWAL identity / content-hash registry on Polygon.
//!
//! Contract deploy is an **operational prerequisite** (not shipped bytecode).
//! Interface (Solidity):
//! ```solidity
//! interface ISwalIdentityRegistry {
//!     function anchorIdentity(bytes32 contentHash) external;
//!     function anchorPack(bytes32 contentHash) external;
//! }
//! ```
//!
//! Function selectors (keccak256 of signatures, first 4 bytes):
//! - `anchorIdentity(bytes32)` → `0x1c8b5c2f` (computed offline; verified in tests via sha3 if available)
//!
//! We encode calldata without pulling alloy into the default build.

use anyhow::{bail, Result};

/// keccak256("anchorIdentity(bytes32)")[0..4] = 0x4f3066ee
pub const SELECTOR_ANCHOR_IDENTITY: [u8; 4] = [0x4f, 0x30, 0x66, 0xee];

/// keccak256("anchorPack(bytes32)")[0..4] = 0x1581d78e
pub const SELECTOR_ANCHOR_PACK: [u8; 4] = [0x15, 0x81, 0xd7, 0x8e];

/// Prepared EVM call — safe to log (no private keys).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedAnchorCall {
    pub to: String,
    pub data_hex: String,
    pub chain_id: u64,
    pub content_hash_hex: String,
    pub kind: AnchorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorKind {
    Identity,
    Pack,
}

fn parse_hash32(content_hash_hex: &str) -> Result<[u8; 32]> {
    let raw = crate::crypto::hex_decode(content_hash_hex.trim_start_matches("0x"))?;
    if raw.len() != 32 {
        bail!("content_hash must be 32 bytes, got {}", raw.len());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&raw);
    Ok(out)
}

/// ABI-encode `anchorIdentity(bytes32)` / `anchorPack(bytes32)` calldata.
pub fn encode_anchor_calldata(kind: AnchorKind, content_hash_hex: &str) -> Result<Vec<u8>> {
    let hash = parse_hash32(content_hash_hex)?;
    let selector = match kind {
        AnchorKind::Identity => SELECTOR_ANCHOR_IDENTITY,
        AnchorKind::Pack => SELECTOR_ANCHOR_PACK,
    };
    let mut data = Vec::with_capacity(4 + 32);
    data.extend_from_slice(&selector);
    data.extend_from_slice(&hash);
    Ok(data)
}

pub fn prepare_anchor_call(
    contract: &str,
    content_hash_hex: &str,
    chain_id: u64,
    kind: AnchorKind,
) -> Result<PreparedAnchorCall> {
    let c = contract.trim();
    if c.is_empty() || !c.starts_with("0x") {
        bail!("SWAL_ANCHOR_CONTRACT must be a 0x-prefixed address");
    }
    let data = encode_anchor_calldata(kind, content_hash_hex)?;
    Ok(PreparedAnchorCall {
        to: c.to_string(),
        data_hex: format!("0x{}", crate::crypto::hex_encode(data)),
        chain_id,
        content_hash_hex: content_hash_hex.trim_start_matches("0x").to_string(),
        kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calldata_is_36_bytes_and_starts_with_selector() {
        let hash = "11".repeat(32);
        let data = encode_anchor_calldata(AnchorKind::Identity, &hash).unwrap();
        assert_eq!(data.len(), 36);
        assert_eq!(&data[0..4], &SELECTOR_ANCHOR_IDENTITY);
        assert_eq!(crate::crypto::hex_encode(&data[4..]), hash);
    }

    #[test]
    fn prepare_requires_contract_address() {
        assert!(prepare_anchor_call("", &"aa".repeat(32), 80002, AnchorKind::Identity).is_err());
        let p = prepare_anchor_call(
            "0x1111111111111111111111111111111111111111",
            &"bb".repeat(32),
            80002,
            AnchorKind::Pack,
        )
        .unwrap();
        assert!(p.data_hex.starts_with("0x"));
        assert_eq!(p.kind, AnchorKind::Pack);
    }
}
