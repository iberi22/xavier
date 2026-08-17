//! SWAL Fase 2 — Polygon identity / content-hash anchors (metadata only).
//!
//! On-chain: pubkey commitment hash, sealed-pack content_hash, tx receipt.
//! Off-chain: seed, vault, payloads (ADR-SWAL-MESH-GOV §2.2).
//!
//! Env (required for live submit — never hardcode secrets):
//! - `SWAL_POLYGON_RPC_URL` — HTTPS RPC
//! - `SWAL_ANCHOR_KEY` — hex private key (never logged)
//! - `SWAL_POLYGON_CHAIN_ID` — default `80002` (Polygon Amoy)
//! - `SWAL_ANCHOR_CONTRACT` — deployed `ISwalIdentityRegistry` address
//! - `SWAL_ANCHOR_DRY_RUN=1` — force mock (default when RPC/key/contract unset)
//! - `SWAL_ANCHOR_BROADCAST=1` — send tx via alloy (`dao-evm` feature); else `live-prepared`

pub mod abi;

#[cfg(feature = "dao-evm")]
pub mod broadcast;

pub use abi::{
    encode_anchor_calldata, prepare_anchor_call, AnchorKind, PreparedAnchorCall,
    SELECTOR_ANCHOR_IDENTITY, SELECTOR_ANCHOR_PACK,
};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// Polygon Amoy testnet chain id (default).
pub const DEFAULT_CHAIN_ID_AMOY: u64 = 80002;

/// Domain-separated identity anchor payload (no secrets).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IdentityAnchorPayload {
    pub version: u8,
    pub node_id: String,
    pub ed25519_public_hex: String,
    pub ml_dsa_commitment_hex: String,
    /// SHA-256 hex of [`canonical_identity_bytes`].
    pub content_hash_hex: String,
}

/// Local receipt after (mock or live) anchor submit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnchorReceipt {
    pub content_hash_hex: String,
    pub chain_id: u64,
    /// Hex tx hash when live; synthetic `mock:…` / `live-prepared:…` otherwise.
    pub tx_hash: String,
    pub dry_run: bool,
    pub anchored_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<String>,
    /// ABI calldata prepared for the registry (safe to audit; no secrets).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calldata_hex: Option<String>,
}

/// Canonical bytes hashed for on-chain registry (DL-F2-01).
pub fn canonical_identity_bytes(
    node_id: &str,
    ed25519_public_hex: &str,
    ml_dsa_commitment_hex: &str,
) -> Vec<u8> {
    format!("swal-identity-anchor-v1|{node_id}|{ed25519_public_hex}|{ml_dsa_commitment_hex}")
        .into_bytes()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    crate::crypto::hex_encode(hasher.finalize())
}

/// Build identity anchor payload from Xavier public card fields.
pub fn build_identity_anchor(
    node_id: &str,
    ed25519_public_hex: &str,
    ml_dsa_commitment_hex: &str,
) -> IdentityAnchorPayload {
    let raw = canonical_identity_bytes(node_id, ed25519_public_hex, ml_dsa_commitment_hex);
    IdentityAnchorPayload {
        version: 1,
        node_id: node_id.to_string(),
        ed25519_public_hex: ed25519_public_hex.to_string(),
        ml_dsa_commitment_hex: ml_dsa_commitment_hex.to_string(),
        content_hash_hex: sha256_hex(&raw),
    }
}

/// Sealed-pack content hash: SHA-256(ciphertext ‖ meta_utf8) — on-chain only this hash (DL-F2-02).
pub fn sealed_pack_content_hash(ciphertext: &[u8], meta_utf8: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ciphertext);
    hasher.update(meta_utf8.as_bytes());
    crate::crypto::hex_encode(hasher.finalize())
}

/// Transport that submits `content_hash` to Polygon (or mocks).
pub trait AnchorTransport: Send + Sync {
    fn submit_hash(
        &self,
        content_hash_hex: &str,
        chain_id: u64,
        contract: Option<&str>,
    ) -> Result<AnchorReceipt>;

    fn submit_pack_hash(
        &self,
        content_hash_hex: &str,
        chain_id: u64,
        contract: Option<&str>,
    ) -> Result<AnchorReceipt> {
        self.submit_hash(content_hash_hex, chain_id, contract)
    }
}

/// In-memory / deterministic mock — default for CI.
#[derive(Debug, Default, Clone)]
pub struct MockAnchorTransport;

impl AnchorTransport for MockAnchorTransport {
    fn submit_hash(
        &self,
        content_hash_hex: &str,
        chain_id: u64,
        contract: Option<&str>,
    ) -> Result<AnchorReceipt> {
        let synthetic =
            sha256_hex(format!("swal-mock-tx|{chain_id}|{content_hash_hex}").as_bytes());
        let calldata = contract
            .map(|_| {
                encode_anchor_calldata(AnchorKind::Identity, content_hash_hex)
                    .map(|d| format!("0x{}", crate::crypto::hex_encode(d)))
            })
            .transpose()?;
        Ok(AnchorReceipt {
            content_hash_hex: content_hash_hex.to_string(),
            chain_id,
            tx_hash: format!("mock:{synthetic}"),
            dry_run: true,
            anchored_at: chrono::Utc::now().to_rfc3339(),
            contract: contract.map(|s| s.to_string()),
            calldata_hex: calldata,
        })
    }
}

/// Env-configured client: dry-run unless RPC + key + contract present and dry-run unset.
#[derive(Debug, Clone)]
pub struct EnvAnchorTransport {
    pub rpc_url: Option<String>,
    pub chain_id: u64,
    pub contract: Option<String>,
    pub dry_run: bool,
    /// Presence-only flag — never log the key.
    pub has_anchor_key: bool,
    pub kind: AnchorKind,
}

impl EnvAnchorTransport {
    pub fn from_env() -> Self {
        Self::from_env_kind(AnchorKind::Identity)
    }

    pub fn from_env_kind(kind: AnchorKind) -> Self {
        let rpc_url = std::env::var("SWAL_POLYGON_RPC_URL")
            .ok()
            .filter(|s| !s.is_empty());
        let chain_id = std::env::var("SWAL_POLYGON_CHAIN_ID")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_CHAIN_ID_AMOY);
        let contract = std::env::var("SWAL_ANCHOR_CONTRACT")
            .ok()
            .filter(|s| !s.is_empty());
        let has_anchor_key = std::env::var("SWAL_ANCHOR_KEY")
            .ok()
            .filter(|s| !s.is_empty())
            .is_some();
        let force_dry = std::env::var("SWAL_ANCHOR_DRY_RUN")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let dry_run = force_dry || rpc_url.is_none() || !has_anchor_key || contract.is_none();
        Self {
            rpc_url,
            chain_id,
            contract,
            dry_run,
            has_anchor_key,
            kind,
        }
    }

    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        if dry_run {
            self.dry_run = true;
        }
        self
    }
}

impl AnchorTransport for EnvAnchorTransport {
    fn submit_hash(
        &self,
        content_hash_hex: &str,
        chain_id: u64,
        contract: Option<&str>,
    ) -> Result<AnchorReceipt> {
        let chain = if chain_id == 0 {
            self.chain_id
        } else {
            chain_id
        };
        let contract = contract.or(self.contract.as_deref());

        if self.dry_run {
            return MockAnchorTransport.submit_hash(content_hash_hex, chain, contract);
        }

        let rpc = self
            .rpc_url
            .as_deref()
            .context("SWAL_POLYGON_RPC_URL required for live anchor")?;
        if !self.has_anchor_key {
            bail!("SWAL_ANCHOR_KEY required for live anchor");
        }
        if !(rpc.starts_with("http://") || rpc.starts_with("https://")) {
            bail!("SWAL_POLYGON_RPC_URL must be http(s)");
        }
        let contract = contract.context("SWAL_ANCHOR_CONTRACT required for live anchor")?;
        let prepared = prepare_anchor_call(contract, content_hash_hex, chain, self.kind)?;

        let digest = sha256_hex(prepared.data_hex.as_bytes());
        let broadcast = std::env::var("SWAL_ANCHOR_BROADCAST")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let tx_hash = if broadcast {
            #[cfg(feature = "dao-evm")]
            {
                let rt = tokio::runtime::Handle::try_current();
                let fut =
                    broadcast::broadcast_from_env(content_hash_hex, chain, contract, self.kind);

                match rt {
                    Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut))?,
                    Err(_) => {
                        let runtime = tokio::runtime::Runtime::new()
                            .context("tokio runtime for polygon broadcast")?;
                        runtime.block_on(fut)?
                    }
                }
            }
            #[cfg(not(feature = "dao-evm"))]
            {
                tracing::warn!(
                    "SWAL_ANCHOR_BROADCAST=1 but xavier built without feature `dao-evm`; \
                     emitting live-broadcast-pending (rebuild with --features dao-evm)"
                );
                format!("live-broadcast-pending:{digest}")
            }
        } else {
            format!("live-prepared:{digest}")
        };

        tracing::info!(
            chain_id = chain,
            rpc_host = %rpc.split('/').nth(2).unwrap_or("rpc"),
            contract = %contract,
            broadcast,
            "SWAL Polygon anchor live path (metadata hash only)"
        );

        Ok(AnchorReceipt {
            content_hash_hex: content_hash_hex.to_string(),
            chain_id: chain,
            tx_hash,
            dry_run: false,
            anchored_at: chrono::Utc::now().to_rfc3339(),
            contract: Some(contract.to_string()),
            calldata_hex: Some(prepared.data_hex),
        })
    }

    fn submit_pack_hash(
        &self,
        content_hash_hex: &str,
        chain_id: u64,
        contract: Option<&str>,
    ) -> Result<AnchorReceipt> {
        let mut pack_transport = self.clone();
        pack_transport.kind = AnchorKind::Pack;
        pack_transport.submit_hash(content_hash_hex, chain_id, contract)
    }
}

/// Persist receipt under `{data_dir}/anchors/{content_hash}.json` (0600).
pub struct AnchorRegistry {
    pub root: PathBuf,
}

impl AnchorRegistry {
    pub fn under_data_dir(data_dir: impl AsRef<Path>) -> Self {
        Self {
            root: data_dir.as_ref().join("anchors"),
        }
    }

    pub fn default_from_env() -> Self {
        Self::under_data_dir(crate::settings::XavierSettings::resolve_data_dir())
    }

    pub fn save(&self, receipt: &AnchorReceipt) -> Result<PathBuf> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("create {}", self.root.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700));
        }
        let path = self.root.join(format!("{}.json", receipt.content_hash_hex));
        let json = serde_json::to_string_pretty(receipt)?;
        fs::write(&path, json.as_bytes()).with_context(|| format!("write {}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }
        Ok(path)
    }

    pub fn load(&self, content_hash_hex: &str) -> Result<AnchorReceipt> {
        let path = self.root.join(format!("{content_hash_hex}.json"));
        let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        Ok(serde_json::from_str(&raw)?)
    }
}

/// High-level: build identity anchor, submit via transport, persist receipt.
pub fn anchor_node_identity<T: AnchorTransport>(
    transport: &T,
    node_id: &str,
    ed25519_public_hex: &str,
    ml_dsa_commitment_hex: &str,
    registry: Option<&AnchorRegistry>,
) -> Result<(IdentityAnchorPayload, AnchorReceipt)> {
    let payload = build_identity_anchor(node_id, ed25519_public_hex, ml_dsa_commitment_hex);
    let chain_id = std::env::var("SWAL_POLYGON_CHAIN_ID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_CHAIN_ID_AMOY);
    let receipt = transport.submit_hash(&payload.content_hash_hex, chain_id, None)?;
    if let Some(reg) = registry {
        reg.save(&receipt)?;
    }
    Ok((payload, receipt))
}

/// Anchor sealed-pack content_hash only (ciphertext stays off-chain).
pub fn anchor_sealed_pack<T: AnchorTransport>(
    transport: &T,
    ciphertext: &[u8],
    meta_utf8: &str,
    registry: Option<&AnchorRegistry>,
) -> Result<(String, AnchorReceipt)> {
    let content_hash_hex = sealed_pack_content_hash(ciphertext, meta_utf8);
    let chain_id = std::env::var("SWAL_POLYGON_CHAIN_ID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_CHAIN_ID_AMOY);
    let receipt = transport.submit_pack_hash(&content_hash_hex, chain_id, None)?;
    if let Some(reg) = registry {
        reg.save(&receipt)?;
    }
    Ok((content_hash_hex, receipt))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_hash_stable_and_domain_separated() {
        let a = build_identity_anchor("xv1-abc", "aa", "bb");
        let b = build_identity_anchor("xv1-abc", "aa", "bb");
        assert_eq!(a.content_hash_hex, b.content_hash_hex);
        assert_eq!(a.content_hash_hex.len(), 64);
        let c = build_identity_anchor("xv1-abc", "aa", "cc");
        assert_ne!(a.content_hash_hex, c.content_hash_hex);
    }

    #[test]
    fn sealed_pack_hash_excludes_plaintext_secret() {
        let h1 = sealed_pack_content_hash(b"cipher", r#"{"v":1}"#);
        let h2 = sealed_pack_content_hash(b"cipher", r#"{"v":1}"#);
        let h3 = sealed_pack_content_hash(b"cipher", r#"{"v":2}"#);
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn mock_submit_and_registry_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let reg = AnchorRegistry::under_data_dir(dir.path());
        let transport = MockAnchorTransport;
        let (payload, receipt) = anchor_node_identity(
            &transport,
            "xv1-node",
            &"ab".repeat(32),
            &"cd".repeat(32),
            Some(&reg),
        )
        .unwrap();
        assert!(receipt.dry_run);
        assert!(receipt.tx_hash.starts_with("mock:"));
        let loaded = reg.load(&payload.content_hash_hex).unwrap();
        assert_eq!(loaded.content_hash_hex, receipt.content_hash_hex);
    }

    #[test]
    fn live_path_prepares_calldata_without_secrets() {
        let t = EnvAnchorTransport {
            rpc_url: Some("https://rpc.example/amoy".into()),
            chain_id: DEFAULT_CHAIN_ID_AMOY,
            contract: Some("0x1111111111111111111111111111111111111111".into()),
            dry_run: false,
            has_anchor_key: true,
            kind: AnchorKind::Identity,
        };
        let hash = "ee".repeat(32);
        let r = t.submit_hash(&hash, DEFAULT_CHAIN_ID_AMOY, None).unwrap();
        assert!(!r.dry_run);
        assert!(r.tx_hash.starts_with("live-prepared:"));
        let data = r.calldata_hex.unwrap();
        assert!(data.starts_with("0x4f3066ee"));
    }

    #[test]
    fn env_transport_defaults_dry_run() {
        let t = EnvAnchorTransport {
            rpc_url: None,
            chain_id: DEFAULT_CHAIN_ID_AMOY,
            contract: None,
            dry_run: true,
            has_anchor_key: false,
            kind: AnchorKind::Identity,
        };
        let r = t.submit_hash("abcd", DEFAULT_CHAIN_ID_AMOY, None).unwrap();
        assert!(r.dry_run);
    }

    #[test]
    fn anchor_pack_roundtrip() {
        let t = MockAnchorTransport;
        let (hash, receipt) =
            anchor_sealed_pack(&t, b"cipher-bytes", r#"{"pack":1}"#, None).unwrap();
        assert_eq!(hash.len(), 64);
        assert!(receipt.dry_run);
    }
}
