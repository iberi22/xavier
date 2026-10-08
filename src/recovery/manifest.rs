//! The recovery manifest: what must be recoverable, and how healthy it is.
//!
//! # The asset is a SET, not a key
//!
//! Recovering `record.key` alone does not restore a readable vault. Two more
//! things are required, and this manifest tracks all three because each can be
//! lost independently:
//!
//! 1. **`record.key`** — the only thing that unwraps the 22,410 per-record DEKs.
//! 2. **`master.key`** — `auth2` SQLCipher + JWT + `secrets` vault. Re-derivable
//!    from host name + machine-id, but only on the *same* machine.
//! 3. **The rclone crypt passphrase** — `~/.config/xavier-backup/crypt-recovery.txt`.
//!    Without it every snapshot on `gdrive-xavier-crypt:` is permanently
//!    unreadable, and today it is backed up nowhere.
//!
//! Point 3 is why this is a manifest and not a single-key escrow: reporting
//! "master key recoverable" while the crypt password sits in no backup at all is
//! exactly the false comfort this design exists to remove.

use serde::{Deserialize, Serialize};

use crate::crypto::hex_encode;
use crate::recovery::kcv::encode_kcv;
use crate::recovery::store::{KcvState, SealHealth};

/// On-disk manifest version.
pub const RECOVERY_FORMAT_VERSION: u16 = 1;

/// Which recovery paths currently hold a usable copy.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct RecoveryStatus {
    /// A 24-word BIP39 seal of the keys exists.
    pub mnemonic_seal_present: bool,
    /// A passphrase-sealed blob exists.
    pub passphrase_seal_present: bool,
    pub mnemonic_seal: SealHealth,
    pub passphrase_seal: SealHealth,
    pub kcv: KcvState,
    /// XDK2 rows in this space require the master key, independently of record.key.
    pub default_space_keystore_exists: bool,
    /// The rclone crypt passphrase is stored somewhere recoverable.
    ///
    /// `None` means "unknown"; `Some(false)` means "verified absent". The
    /// distinction matters: an unchecked passphrase must never be reported as safe.
    pub crypt_passphrase_backed_up: Option<bool>,
}

impl RecoveryStatus {
    /// Recovery is considered real only when a path exists **and** the
    /// independent crypt passphrase is accounted for.
    pub fn is_recoverable(&self) -> bool {
        let path = self.mnemonic_seal.is_valid() || self.passphrase_seal.is_valid();
        let crypt_ok = self.crypt_passphrase_backed_up == Some(true);
        path && matches!(
            self.kcv,
            KcvState::MatchesLiveKey | KcvState::LiveKeyMissing
        ) && crypt_ok
    }

    /// Human-readable reasons recovery is not yet trustworthy.
    pub fn gaps(&self) -> Vec<String> {
        let mut gaps = Vec::new();
        if !self.mnemonic_seal_present && !self.passphrase_seal_present {
            gaps.push("no recovery seal exists for the record key".to_string());
        }
        match self.kcv {
            KcvState::Absent => gaps.push("no KCV: an unsealed key cannot be verified".to_string()),
            KcvState::MismatchesLiveKey => gaps.push(
                "seals protect a different key than the live record.key (KCV mismatch)".to_string(),
            ),
            KcvState::Malformed => {
                gaps.push("the key-check value or manifest is malformed".to_string())
            }
            KcvState::ManifestDisagrees => {
                gaps.push("the manifest disagrees with the key-check value sidecar".to_string())
            }
            KcvState::MatchesLiveKey | KcvState::LiveKeyMissing => {}
        }
        if self.default_space_keystore_exists {
            gaps.push("default-space keystore exists: XDK2 rows are protected by the master key, not by record.key".to_string());
        }
        match self.crypt_passphrase_backed_up {
            Some(true) => {}
            Some(false) => gaps.push(
                "the rclone crypt passphrase is NOT backed up: every encrypted snapshot is \
                 currently unrecoverable"
                    .to_string(),
            ),
            None => {
                gaps.push("the rclone crypt passphrase backup state was never verified".to_string())
            }
        }
        gaps
    }
}

/// The sealed recovery bundle. Contains KCVs, never keys.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveryManifest {
    pub version: u16,
    /// Hex `HMAC(record_key, "xavier-kcv-v1")`.
    pub record_key_kcv: String,
    /// Hex `HMAC(master_key, "xavier-kcv-v1")`.
    pub master_key_kcv: String,
    pub created_at: String,
    /// Optional: where this manifest was sealed (a path or remote label).
    pub note: Option<String>,
}

impl RecoveryManifest {
    /// Build a manifest for the given keys.
    ///
    /// Takes the keys by reference and stores only their KCVs, so the manifest is
    /// safe to keep next to the ciphertext.
    pub fn new(record_key: &[u8; 32], master_key: Option<&[u8; 32]>) -> Self {
        Self {
            version: RECOVERY_FORMAT_VERSION,
            record_key_kcv: encode_kcv(record_key),
            master_key_kcv: master_key.map_or_else(String::new, encode_kcv),
            created_at: chrono::Utc::now().to_rfc3339(),
            note: None,
        }
    }

    /// Whether the candidate record key is provably the right one.
    pub fn verify_record_key(&self, candidate: &[u8; 32]) -> Result<(), crate::recovery::KcvError> {
        crate::recovery::kcv::verify_kcv(candidate, &self.record_key_kcv)
    }

    /// Whether the candidate master key is provably the right one.
    ///
    /// An empty stored KCV means the manifest was built without a master key;
    /// that is reported as [`crate::recovery::KcvError::Absent`] rather than a
    /// mismatch, so a caller never mistakes "not covered" for "wrong".
    pub fn verify_master_key(&self, candidate: &[u8; 32]) -> Result<(), crate::recovery::KcvError> {
        crate::recovery::kcv::verify_kcv(candidate, &self.master_key_kcv)
    }

    /// The hex of a record key's KCV, for writing to a sidecar file.
    pub fn record_kcv_hex(&self) -> String {
        hex_encode(self.record_key_kcv.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_stores_kcvs_not_keys() {
        let record = [0x44u8; 32];
        let master = [0x55u8; 32];
        let m = RecoveryManifest::new(&record, Some(&master));

        let blob = serde_json::to_string(&m).unwrap();
        assert!(!blob.contains(&hex_encode(record)), "record key leaked");
        assert!(!blob.contains(&hex_encode(master)), "master key leaked");
        assert_eq!(m.record_key_kcv.len(), 64);
        assert_eq!(m.master_key_kcv.len(), 64);
    }

    /// AC: correct key verifies, wrong key is a mismatch — with no data touched.
    #[test]
    fn manifest_verifies_keys_without_side_effects() {
        let record = [0x01u8; 32];
        let m = RecoveryManifest::new(&record, None);
        assert_eq!(m.verify_record_key(&record), Ok(()));
        assert!(m.verify_record_key(&[0x02u8; 32]).is_err());
    }

    /// AC: a manifest without a master key must not report a mismatch.
    #[test]
    fn absent_master_kcv_is_not_a_mismatch() {
        let m = RecoveryManifest::new(&[0x01u8; 32], None);
        assert_eq!(
            m.verify_master_key(&[0x09u8; 32]),
            Err(crate::recovery::KcvError::Absent)
        );
    }

    /// AC: recovering the key is not enough — the crypt passphrase gates recovery.
    #[test]
    fn status_requires_the_crypt_passphrase_too() {
        let with_seal = RecoveryStatus {
            mnemonic_seal_present: true,
            passphrase_seal_present: false,
            crypt_passphrase_backed_up: Some(false),
            mnemonic_seal: SealHealth::Valid { mode_0600: true },
            kcv: KcvState::MatchesLiveKey,
            ..Default::default()
        };
        assert!(
            !with_seal.is_recoverable(),
            "a seal without the crypt passphrase is NOT recovery"
        );
        assert!(with_seal
            .gaps()
            .iter()
            .any(|g| g.contains("crypt passphrase")));

        let complete = RecoveryStatus {
            crypt_passphrase_backed_up: Some(true),
            ..with_seal.clone()
        };
        assert!(complete.is_recoverable());
        assert!(complete.gaps().is_empty());
    }

    #[test]
    fn status_reports_default_space_master_key_gap() {
        let status = RecoveryStatus {
            default_space_keystore_exists: true,
            ..Default::default()
        };
        assert!(status.gaps().iter().any(|gap| gap ==
            "default-space keystore exists: XDK2 rows are protected by the master key, not by record.key"));
    }

    /// An unverified passphrase must never be reported as safe.
    #[test]
    fn unverified_crypt_passphrase_is_a_gap() {
        let s = RecoveryStatus {
            mnemonic_seal_present: true,
            passphrase_seal_present: true,
            crypt_passphrase_backed_up: None,
            mnemonic_seal: SealHealth::Valid { mode_0600: true },
            passphrase_seal: SealHealth::Valid { mode_0600: true },
            kcv: KcvState::MatchesLiveKey,
            ..Default::default()
        };
        assert!(!s.is_recoverable());
        assert!(s.gaps().iter().any(|g| g.contains("never verified")));
    }

    /// No seal at all is the weakest state and must be reported.
    #[test]
    fn no_seal_is_a_gap() {
        let s = RecoveryStatus {
            mnemonic_seal_present: false,
            passphrase_seal_present: false,
            crypt_passphrase_backed_up: Some(true),
            ..Default::default()
        };
        assert!(!s.is_recoverable());
        assert!(s.gaps().iter().any(|g| g.contains("no recovery seal")));
    }
}
