//! SWAL Fase 0 — autenticación / recuperación de nodo (AUTH_RECOVERY_SPIKE).
//!
//! Implementa login **local sin servidor de cuentas**:
//! - BIP39-24 (+ passphrase opcional)
//! - Shamir 2-of-3 sobre la entropy
//! - Check-codes HMAC + challenge ASC/DESC
//! - Vault sellado con Argon2id + AES-256-GCM (+ device key opcional)
//! - Derivación Ed25519 nodo + commitment seed ML-DSA (para edge-mesh)
//!
//! Canónico: `docs/SWAL/AUTH_RECOVERY_SPIKE.md`, `DECENTRALIZED_LOGIN.md` §Fase 0.
//!
//! **No-goals:** Stripe-as-Pro, ledger en mesh, templates biométricos.

pub mod bip39_seed;
pub mod check_codes;
pub mod derive;
pub mod founder;
pub mod hybrid_pack;
pub mod persist;
pub mod shamir;
pub mod vault;

pub use bip39_seed::{GeneratedSeed, SeedPhrase};
pub use check_codes::{CheckCodes, OrderMode, OrderedChallenge};
pub use derive::{DerivedNodeKeys, DOMAIN_ML_DSA, DOMAIN_NODE_ED25519};
pub use founder::{
    founder_status_handler, generate_founder_attestation, verify_founder_attestation,
    FounderNodeAttestation, FounderStatusResponse, NodeMetadata, SwalGenesisParams,
};
pub use hybrid_pack::{onchain_pack_hash, HybridPackSignature};
pub use persist::{NodeStore, NodeStorePaths, PublicNodeIdentity};
pub use shamir::{ShamirShare, ShamirSplit};
pub use vault::{SealedVault, VaultError};

use anyhow::Result;

/// Orquestación Fase 0: crear identidad de nodo a partir de entropy fresca.
pub struct NodeBootstrap;

impl NodeBootstrap {
    /// Flujo de creación (§4.2 AUTH_RECOVERY_SPIKE).
    ///
    /// Retorna frase BIP39-24, shares Shamir 2-of-3, check-codes, claves derivadas
    /// y vault sellado (PIN + device_key opcional).
    pub fn create(
        passphrase: Option<&str>,
        pin: &str,
        device_key: Option<&[u8; 32]>,
    ) -> Result<BootstrapBundle> {
        let generated = SeedPhrase::generate_24(passphrase)?;
        let shares = ShamirSplit::split_2_of_3(&generated.entropy)?;
        let codes = CheckCodes::from_seed_bytes(&generated.seed_bytes);
        let keys = DerivedNodeKeys::from_seed_bytes(&generated.seed_bytes)?;
        let vault = SealedVault::seal(
            &generated.entropy,
            passphrase.unwrap_or(""),
            pin,
            device_key,
        )?;

        Ok(BootstrapBundle {
            mnemonic: generated.mnemonic_words,
            passphrase_used: passphrase.map(|s| !s.is_empty()).unwrap_or(false),
            shares,
            check_codes: codes,
            keys,
            vault,
        })
    }

    /// Flujo de recuperación (§4.3): ≥2 shares → seed → challenge ordenado → vault.
    pub fn recover_from_shares(
        shares: &[ShamirShare],
        passphrase: Option<&str>,
        challenge_response: &[u16; 6],
        challenge: &OrderedChallenge,
        pin: &str,
        device_key: Option<&[u8; 32]>,
    ) -> Result<BootstrapBundle> {
        let entropy = ShamirSplit::combine(shares)?;
        let phrase = SeedPhrase::from_entropy(&entropy, passphrase)?;
        let codes = CheckCodes::from_seed_bytes(&phrase.seed_bytes);
        if !challenge.verify(challenge_response, &codes) {
            anyhow::bail!("ordered check-code challenge failed");
        }
        let keys = DerivedNodeKeys::from_seed_bytes(&phrase.seed_bytes)?;
        let vault = SealedVault::seal(&entropy, passphrase.unwrap_or(""), pin, device_key)?;

        Ok(BootstrapBundle {
            mnemonic: phrase.mnemonic_words,
            passphrase_used: passphrase.map(|s| !s.is_empty()).unwrap_or(false),
            shares: ShamirSplit::split_2_of_3(&entropy)?,
            check_codes: codes,
            keys,
            vault,
        })
    }
}

/// Resultado de bootstrap / recovery (material sensible — no loguear).
#[derive(Clone)]
pub struct BootstrapBundle {
    pub mnemonic: String,
    pub passphrase_used: bool,
    pub shares: Vec<ShamirShare>,
    pub check_codes: CheckCodes,
    pub keys: DerivedNodeKeys,
    pub vault: SealedVault,
}

impl std::fmt::Debug for BootstrapBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BootstrapBundle")
            .field("mnemonic", &"[REDACTED]")
            .field("passphrase_used", &self.passphrase_used)
            .field("shares", &format!("{} shares", self.shares.len()))
            .field("check_codes", &self.check_codes)
            .field("keys", &self.keys)
            .field("vault", &self.vault)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fase0_create_recover_roundtrip() {
        let bundle = NodeBootstrap::create(Some("swal-pass"), "123456", None).unwrap();
        assert_eq!(bundle.mnemonic.split_whitespace().count(), 24);
        assert_eq!(bundle.shares.len(), 3);
        assert_eq!(bundle.check_codes.triplets.len(), 6);

        // 1 share alone fails
        assert!(ShamirSplit::combine(&bundle.shares[..1]).is_err());

        // 2 shares reconstruct
        let entropy = ShamirSplit::combine(&bundle.shares[0..2]).unwrap();
        let phrase = SeedPhrase::from_entropy(&entropy, Some("swal-pass")).unwrap();
        assert_eq!(phrase.mnemonic_words, bundle.mnemonic);

        let codes = CheckCodes::from_seed_bytes(&phrase.seed_bytes);
        let challenge = OrderedChallenge::new(OrderMode::Asc, &codes);
        let response = challenge.expected_response(&codes);

        let recovered = NodeBootstrap::recover_from_shares(
            &bundle.shares[1..3],
            Some("swal-pass"),
            &response,
            &challenge,
            "654321",
            None,
        )
        .unwrap();

        assert_eq!(recovered.keys.node_id, bundle.keys.node_id);
        assert_eq!(
            recovered.keys.ml_dsa_commitment,
            bundle.keys.ml_dsa_commitment
        );
    }

    #[test]
    fn vault_unlock_with_pin() {
        let bundle = NodeBootstrap::create(None, "999888", None).unwrap();
        let opened = bundle.vault.unlock("999888", None).unwrap();
        assert_eq!(opened.entropy.len(), 32);
        assert!(bundle.vault.unlock("000000", None).is_err());
    }
}
