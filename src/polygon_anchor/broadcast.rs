//! Live Polygon broadcast for identity/pack anchors (feature `dao-evm`).
//!
//! Only metadata hashes — never seeds or payloads (ADR-SWAL-MESH-GOV).

use super::abi::{AnchorKind, PreparedAnchorCall};
use alloy::{
    network::{Ethereum, EthereumWallet},
    primitives::{Address, B256},
    providers::ProviderBuilder,
    signers::local::PrivateKeySigner,
    sol,
};
use anyhow::{bail, Context, Result};

sol!(
    #[sol(rpc)]
    interface ISwalIdentityRegistry {
        function anchorIdentity(bytes32 contentHash) external;
        function anchorPack(bytes32 contentHash) external;
    }
);

fn parse_pk(raw: &str) -> Result<PrivateKeySigner> {
    let s = raw.trim().trim_start_matches("0x");
    s.parse::<PrivateKeySigner>()
        .context("SWAL_ANCHOR_KEY is not a valid secp256k1 private key")
}

fn parse_hash32(content_hash_hex: &str) -> Result<B256> {
    let raw = crate::crypto::hex_decode(content_hash_hex.trim_start_matches("0x"))?;
    if raw.len() != 32 {
        bail!("content_hash must be 32 bytes");
    }
    Ok(B256::from_slice(&raw))
}

/// Broadcast prepared anchor calldata; returns 0x-prefixed tx hash.
pub async fn broadcast_prepared_anchor(
    rpc_url: &str,
    private_key: &str,
    prepared: &PreparedAnchorCall,
) -> Result<String> {
    let signer = parse_pk(private_key)?;
    let wallet = EthereumWallet::from(signer);
    let provider = ProviderBuilder::new()
        .network::<Ethereum>()
        .wallet(wallet)
        .connect_http(
            rpc_url
                .parse::<url::Url>()
                .context("SWAL_POLYGON_RPC_URL")?,
        );

    let to: Address = prepared
        .to
        .parse()
        .context("SWAL_ANCHOR_CONTRACT address")?;
    let hash = parse_hash32(&prepared.content_hash_hex)?;
    let contract = ISwalIdentityRegistry::new(to, provider);

    let pending = match prepared.kind {
        AnchorKind::Identity => contract.anchorIdentity(hash).send().await?,
        AnchorKind::Pack => contract.anchorPack(hash).send().await?,
    };
    let tx_hash = *pending.tx_hash();
    let _ = pending.get_receipt().await;
    Ok(format!("{tx_hash:#x}"))
}

/// Build + broadcast from env (used when `SWAL_ANCHOR_BROADCAST=1` + feature `dao-evm`).
pub async fn broadcast_from_env(
    content_hash_hex: &str,
    chain_id: u64,
    contract: &str,
    kind: AnchorKind,
) -> Result<String> {
    let rpc = std::env::var("SWAL_POLYGON_RPC_URL").context("SWAL_POLYGON_RPC_URL")?;
    let key = std::env::var("SWAL_ANCHOR_KEY").context("SWAL_ANCHOR_KEY")?;
    let prepared = super::abi::prepare_anchor_call(contract, content_hash_hex, chain_id, kind)?;
    broadcast_prepared_anchor(&rpc, &key, &prepared).await
}
