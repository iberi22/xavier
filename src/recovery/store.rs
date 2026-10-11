//! On-disk recovery store and the **non-destructive** restore.
//!
//! # Why restore must never overwrite
//!
//! [`crate::memory::sqlite_vec_store::at_rest::resolve_record_key_with_source`]
//! *generates a fresh random key when none is found* (`at_rest.rs:84-88`). That is
//! correct behaviour for a new node and catastrophic for a restore: if a restore
//! writes a candidate key over the live file first and validates afterwards, the
//! window in which a wrong key is installed is exactly the window in which new
//! rows get written under the wrong key — and those rows are then unreadable
//! forever, while the originals look fine.
//!
//! [`RecoveryStore::restore_into`] therefore never writes over an existing file:
//!
//! ```text
//! 1. stage unique 0600 candidate (<dir>/<name>.candidate.<pid>.<rand>)
//! 2. verify its KCV against the manifest          <- fails here => nothing changes
//! 3. hard_link(candidate, <dir>/<name>)           <- fails if target exists; only after verification
//! 4. remove candidate
//! ```
//!
//! A failed verification leaves both the live key and the candidate untouched.
//! There is no code path in this module that calls `fs::write` on an existing key
//! file, and [`RestoreOutcome`] reports which of the two cases occurred.
//!
//! Restore uses `hard_link` for an atomic no-clobber install, so the data
//! directory must be on a filesystem that supports hard links (ext4/btrfs/xfs
//! do; some FUSE/vfat/exFAT USB mounts do not) and on such filesystems restore
//! fails cleanly without touching the target.
//!
//! # Nothing here prints key material
//!
//! Errors name the *reason* (mismatch, malformed, absent) and never the bytes.
//! [`RecoveryStore`] has no `Display`/`Debug` that includes a key.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::crypto::hex_decode;
use crate::crypto::hex_encode;
use crate::keystore::{create_private_file_new, ensure_private_dir, write_private_file};
use crate::recovery::kcv::encode_kcv;
use crate::recovery::manifest::RecoveryManifest;
use crate::recovery::mnemonic::{MnemonicPath, MnemonicSeal};
use crate::recovery::passphrase::{PassphrasePath, PassphraseSeal};

/// Filename of the KCV sidecar.
const KCV_FILENAME: &str = "record.key.kcv";
/// Suffix of the pre-verification staging file.
const CANDIDATE_SUFFIX: &str = ".candidate";

/// Filename of the mnemonic seal.
pub const MNEMONIC_SEAL_FILE: &str = "record.mnemonic.seal.json";
/// Filename of the passphrase seal.
pub const PASSPHRASE_SEAL_FILE: &str = "record.passphrase.seal.json";

/// Where a key came from, for operator-facing reporting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySourceReport {
    /// `XAVIER_RECORD_KEY` is set and well formed.
    Env,
    /// Read from `<data_dir>/node/record.key`.
    File(PathBuf),
    /// No key exists at this path; probing never creates one.
    Missing(PathBuf),
    /// Nothing available: the node keeps running but writes plaintext.
    Unavailable(String),
}

/// Structural health of a seal; this does not validate the caller's secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub enum SealHealth {
    #[default]
    Absent,
    Malformed(&'static str),
    Valid {
        mode_0600: bool,
    },
}

impl SealHealth {
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid { .. })
    }
}

/// Whether the stored KCV establishes which record key the seals protect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum KcvState {
    #[default]
    Absent,
    Malformed,
    ManifestDisagrees,
    MatchesLiveKey,
    MismatchesLiveKey,
    LiveKeyMissing,
}

#[derive(Clone, Copy)]
enum SealKind {
    Mnemonic,
    Passphrase,
}

fn inspect_seal(path: &Path, kind: SealKind) -> SealHealth {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return SealHealth::Absent,
        Err(_) => return SealHealth::Malformed("not_regular_file"),
    };
    if !metadata.file_type().is_file() {
        return SealHealth::Malformed("not_regular_file");
    }
    if metadata.len() > 64 * 1024 {
        return SealHealth::Malformed("too_large");
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return SealHealth::Malformed("json_invalid"),
    };
    let (salt, nonce, ciphertext, known_kdf) = match kind {
        SealKind::Mnemonic => {
            let seal: MnemonicSeal = match serde_json::from_slice(&bytes) {
                Ok(seal) => seal,
                Err(_) => return SealHealth::Malformed("json_invalid"),
            };
            let known = seal.kdf == MnemonicPath::kdf_label();
            (seal.salt, seal.nonce, seal.ciphertext, known)
        }
        SealKind::Passphrase => {
            let seal: PassphraseSeal = match serde_json::from_slice(&bytes) {
                Ok(seal) => seal,
                Err(_) => return SealHealth::Malformed("json_invalid"),
            };
            let known =
                serde_json::from_str::<serde_json::Value>(&seal.params).is_ok_and(|params| {
                    params["variant"] == "argon2id"
                        && params["domain"] == "xavier-recovery-passphrase-v1"
                        && params["m_cost"] == 19456
                        && params["t_cost"] == 2
                        && params["p_cost"] == 1
                });
            (seal.salt, seal.nonce, seal.ciphertext, known)
        }
    };
    for (field, length, reason) in [
        (&salt, 16, "salt"),
        (&nonce, 12, "nonce"),
        (&ciphertext, 48, "ciphertext_len"),
    ] {
        if !hex_decode(field).is_ok_and(|decoded| decoded.len() == length) {
            return SealHealth::Malformed(reason);
        }
    }
    if !known_kdf {
        return SealHealth::Malformed(match kind {
            SealKind::Mnemonic => "kdf_unknown",
            SealKind::Passphrase => "kdf_params_unknown",
        });
    }
    #[cfg(unix)]
    let mode_0600 = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o777 == 0o600
    };
    #[cfg(not(unix))]
    let mode_0600 = false;
    SealHealth::Valid { mode_0600 }
}

/// Result of a restore attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RestoreOutcome {
    /// The candidate was verified and atomically installed.
    Installed,
    /// Verification failed; the existing file was left untouched.
    Rejected {
        /// Stable, non-sensitive reason code.
        reason: String,
    },
}

impl RestoreOutcome {
    /// Whether the live key file was replaced.
    pub fn installed(&self) -> bool {
        matches!(self, Self::Installed)
    }
}

/// Recovery store rooted at a directory (default `~/.xavier/recovery`).
#[derive(Debug, Clone)]
pub struct RecoveryStore {
    root: PathBuf,
}

impl RecoveryStore {
    /// Store at an explicit root. Tests must inject a `tempfile::tempdir()`.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Store at `$XAVIER_RECOVERY_DIR`, else `~/.xavier/recovery`.
    pub fn from_env_or_default() -> Self {
        let root = std::env::var("XAVIER_RECOVERY_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".xavier")
                    .join("recovery")
            });
        Self { root }
    }

    /// Root directory of this store.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Create the root as `0700`.
    pub fn ensure_root(&self) -> Result<()> {
        ensure_private_dir(&self.root)
            .with_context(|| format!("cannot create recovery dir {}", self.root.display()))
    }

    /// Write the manifest (KCVs only, never keys) with `0600`.
    pub fn write_manifest(&self, manifest: &RecoveryManifest) -> Result<PathBuf> {
        self.ensure_root()?;
        let path = self.root.join("recovery-manifest.json");
        let bytes = serde_json::to_vec_pretty(manifest).context("cannot serialize manifest")?;
        write_private_file(&path, &bytes)
            .with_context(|| format!("cannot write manifest {}", path.display()))?;
        Ok(path)
    }

    /// Read the manifest, if present.
    pub fn read_manifest(&self) -> Result<Option<RecoveryManifest>> {
        let path = self.root.join("recovery-manifest.json");
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&path).context("cannot read manifest")?;
        let manifest: RecoveryManifest =
            serde_json::from_slice(&bytes).context("manifest is malformed")?;
        Ok(Some(manifest))
    }

    /// Write a KCV sidecar for a key, `0600`.
    pub fn write_kcv(&self, key: &[u8; 32]) -> Result<PathBuf> {
        self.ensure_root()?;
        let path = self.root.join(KCV_FILENAME);
        write_private_file(&path, encode_kcv(key).as_bytes())
            .with_context(|| format!("cannot write KCV {}", path.display()))?;
        Ok(path)
    }

    /// Read a stored KCV.
    pub fn read_kcv(&self) -> Result<Option<String>> {
        let path = self.root.join(KCV_FILENAME);
        if !path.exists() {
            return Ok(None);
        }
        let text = fs::read_to_string(&path).context("cannot read KCV")?;
        Ok(Some(text.trim().to_string()))
    }

    /// Verify a candidate key against a stored KCV sidecar.
    ///
    /// Returns `false` when no KCV is stored, so callers can distinguish
    /// "unverified" from "verified".
    pub fn verify_kcv(&self, candidate: &[u8; 32]) -> Result<bool> {
        match self.read_kcv()? {
            None => Ok(false),
            Some(stored) => Ok(crate::recovery::kcv::verify_kcv(candidate, &stored).is_ok()),
        }
    }

    /// Seal `key` under a 24-word mnemonic and install it as a `0600` file.
    ///
    /// The staged file is re-read from disk and reopened with the same words
    /// *before* it replaces any previous seal, so a seal that cannot be opened
    /// is never installed and never clobbers a good one.
    pub fn write_mnemonic_seal(&self, key: &[u8; 32], words: &str) -> Result<PathBuf> {
        let seal = MnemonicPath::seal(key, words)?;
        self.write_verified_seal(MNEMONIC_SEAL_FILE, &seal, key, |s: &MnemonicSeal| {
            MnemonicPath::open(s, words)
        })
    }

    /// Seal `key` under a passphrase; same guarantees as
    /// [`Self::write_mnemonic_seal`].
    pub fn write_passphrase_seal(&self, key: &[u8; 32], pass: &str) -> Result<PathBuf> {
        let seal = PassphrasePath::seal(key, pass)?;
        self.write_verified_seal(PASSPHRASE_SEAL_FILE, &seal, key, |s: &PassphraseSeal| {
            PassphrasePath::open(s, pass)
        })
    }

    /// Install a mnemonic seal only when no entry exists at its destination.
    /// The atomic install also refuses dangling symlinks and concurrent writers.
    pub fn write_mnemonic_seal_new(&self, key: &[u8; 32], words: &str) -> Result<PathBuf> {
        let seal = MnemonicPath::seal(key, words)?;
        self.write_verified_seal_with_replace(
            MNEMONIC_SEAL_FILE,
            &seal,
            key,
            |s: &MnemonicSeal| MnemonicPath::open(s, words),
            false,
        )
    }

    /// Install a passphrase seal only when no entry exists at its destination.
    pub fn write_passphrase_seal_new(&self, key: &[u8; 32], pass: &str) -> Result<PathBuf> {
        let seal = PassphrasePath::seal(key, pass)?;
        self.write_verified_seal_with_replace(
            PASSPHRASE_SEAL_FILE,
            &seal,
            key,
            |s: &PassphraseSeal| PassphrasePath::open(s, pass),
            false,
        )
    }

    /// Stage `seal` into a unique per-operation candidate file as `0600`, prove the
    /// bytes on disk reopen to `key`, then rename over `file`. On any failure only
    /// this operation's staging file is removed and the previous seal (if any) is
    /// left untouched.
    fn write_verified_seal<S, F>(
        &self,
        file: &str,
        seal: &S,
        key: &[u8; 32],
        open: F,
    ) -> Result<PathBuf>
    where
        S: serde::Serialize + serde::de::DeserializeOwned,
        F: Fn(&S) -> Result<[u8; 32]>,
    {
        self.write_verified_seal_with_replace(file, seal, key, open, true)
    }

    fn write_verified_seal_with_replace<S, F>(
        &self,
        file: &str,
        seal: &S,
        key: &[u8; 32],
        open: F,
        replace: bool,
    ) -> Result<PathBuf>
    where
        S: serde::Serialize + serde::de::DeserializeOwned,
        F: Fn(&S) -> Result<[u8; 32]>,
    {
        self.ensure_root()?;
        let path = self.root.join(file);
        let pid = std::process::id();
        let mut rnd = [0u8; 8];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut rnd);
        let rand_hex = hex_encode(rnd);
        let staging = self
            .root
            .join(format!("{file}{CANDIDATE_SUFFIX}.{pid}.{rand_hex}"));
        let bytes = serde_json::to_vec_pretty(seal).context("cannot serialize seal")?;
        create_private_file_new(&staging, &bytes)
            .with_context(|| format!("cannot stage seal at {}", staging.display()))?;

        let verified = fs::read(&staging)
            .context("cannot re-read staged seal")
            .and_then(|b| serde_json::from_slice::<S>(&b).context("staged seal is malformed"))
            .and_then(|s| open(&s).context("staged seal cannot be reopened"))
            .and_then(|reopened| {
                if &reopened == key {
                    Ok(())
                } else {
                    bail!("reopened key does not match the sealed key")
                }
            });
        if let Err(e) = verified {
            remove_staging(&staging);
            return Err(e.context(format!(
                "seal verification failed; {} was left untouched",
                path.display()
            )));
        }

        let install = if replace {
            fs::rename(&staging, &path)
        } else {
            fs::hard_link(&staging, &path)
        };
        if let Err(e) = install {
            remove_staging(&staging);
            bail!("cannot install seal at {}: {e}", path.display());
        }
        if !replace {
            remove_staging(&staging);
        }
        Ok(path)
    }

    /// Read the mnemonic seal from disk, open it with words, and verify against stored KCV if present.
    /// Never logs or prints key material.
    pub fn unseal_mnemonic(&self, words: &str) -> Result<[u8; 32]> {
        let path = self.root.join(MNEMONIC_SEAL_FILE);
        if !path.exists() {
            bail!("mnemonic seal not found at {}", path.display());
        }
        let bytes = fs::read(&path)
            .with_context(|| format!("cannot read mnemonic seal at {}", path.display()))?;
        let seal: MnemonicSeal = serde_json::from_slice(&bytes)
            .with_context(|| format!("mnemonic seal at {} is malformed", path.display()))?;
        let key = MnemonicPath::open(&seal, words)
            .context("failed to open mnemonic seal: wrong words or corrupted file")?;

        if let Some(expected_kcv) = self.read_kcv()? {
            crate::recovery::kcv::verify_kcv(&key, &expected_kcv)
                .map_err(|e| anyhow::anyhow!("key-check value verification failed: {e}"))?;
        }

        Ok(key)
    }

    /// Read the passphrase seal from disk, open it with pass, and verify against stored KCV if present.
    /// Never logs or prints key material.
    pub fn unseal_passphrase(&self, pass: &str) -> Result<[u8; 32]> {
        let path = self.root.join(PASSPHRASE_SEAL_FILE);
        if !path.exists() {
            bail!("passphrase seal not found at {}", path.display());
        }
        let bytes = fs::read(&path)
            .with_context(|| format!("cannot read passphrase seal at {}", path.display()))?;
        let seal: PassphraseSeal = serde_json::from_slice(&bytes)
            .with_context(|| format!("passphrase seal at {} is malformed", path.display()))?;
        let key = PassphrasePath::open(&seal, pass)
            .context("failed to open passphrase seal: wrong passphrase or corrupted file")?;

        if let Some(expected_kcv) = self.read_kcv()? {
            crate::recovery::kcv::verify_kcv(&key, &expected_kcv)
                .map_err(|e| anyhow::anyhow!("key-check value verification failed: {e}"))?;
        }

        Ok(key)
    }

    /// Restore a key to `target` **without ever overwriting an existing key**.
    ///
    /// `expected_kcv` is the required KCV the key must match. Restore fails
    /// closed when no KCV is available.
    ///
    /// Returns [`RestoreOutcome::Rejected`] and leaves `target` untouched when the
    /// candidate fails verification or `target` already exists.
    pub fn restore_into(
        &self,
        candidate_hex: &str,
        target: &Path,
        expected_kcv: Option<&str>,
    ) -> Result<RestoreOutcome> {
        if fs::symlink_metadata(target).is_ok() {
            return Ok(RestoreOutcome::Rejected {
                reason: "target_exists".to_string(),
            });
        }

        // A malformed candidate is a REJECTION, not an error. This function is
        // the entry point of a disaster-recovery path: a truncated key, a bad
        // paste, or a corrupted offline copy must be refused cleanly and leave
        // the host untouched — never propagate an error the caller might treat
        // as fatal, and never panic.
        let Ok(bytes) = hex_decode(candidate_hex.trim()) else {
            return Ok(RestoreOutcome::Rejected {
                reason: "not_hex".to_string(),
            });
        };
        if bytes.len() != 32 {
            return Ok(RestoreOutcome::Rejected {
                reason: "wrong_key_length".to_string(),
            });
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);

        if expected_kcv.is_none() {
            return Ok(RestoreOutcome::Rejected {
                reason: "kcv_required".to_string(),
            });
        }
        if let Some(expected) = expected_kcv {
            if let Err(e) = crate::recovery::kcv::verify_kcv(&key, expected) {
                return Ok(RestoreOutcome::Rejected {
                    reason: format!("kcv_{e:?}").to_lowercase(),
                });
            }
        }

        // Staging file: a sibling of the target so the hard link is on the
        // same filesystem, and it carries the target's restrictive mode.
        let target_name = target
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "record.key".to_string());
        let pid = std::process::id();
        let mut rnd = [0u8; 8];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut rnd);
        let rand_hex = hex_encode(rnd);
        let staging_name = format!("{target_name}{CANDIDATE_SUFFIX}.{pid}.{rand_hex}");
        let staging = match target.parent() {
            Some(parent) => {
                ensure_private_dir(parent)?;
                parent.join(staging_name)
            }
            None => PathBuf::from(staging_name),
        };

        create_private_file_new(&staging, hex_encode(key).as_bytes())
            .with_context(|| format!("cannot stage candidate at {}", staging.display()))?;

        match fs::hard_link(&staging, target) {
            Ok(()) => {
                remove_staging(&staging);
                Ok(RestoreOutcome::Installed)
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                remove_staging(&staging);
                Ok(RestoreOutcome::Rejected {
                    reason: "target_exists".to_string(),
                })
            }
            Err(e) => {
                remove_staging(&staging);
                bail!("cannot install restored key at {}: {e}", target.display())
            }
        }
    }

    /// Restore directly into the live node record key path.
    ///
    /// Refuses when a live key already exists — see the module docs for why an
    /// overwrite here can orphan every row.
    pub fn restore_record_key(&self, candidate_hex: &str) -> Result<RestoreOutcome> {
        let expected = self.read_kcv()?;
        self.restore_into(candidate_hex, &record_key_path(), expected.as_deref())
    }

    /// Report structural recovery health without creating or changing keys.
    pub fn status(
        &self,
        crypt_passphrase_backed_up: Option<bool>,
        live_key: Option<&[u8; 32]>,
    ) -> crate::recovery::RecoveryStatus {
        let mnemonic_seal = inspect_seal(&self.root.join(MNEMONIC_SEAL_FILE), SealKind::Mnemonic);
        let passphrase_seal =
            inspect_seal(&self.root.join(PASSPHRASE_SEAL_FILE), SealKind::Passphrase);
        let kcv = match self.read_kcv() {
            Ok(None) => KcvState::Absent,
            Err(_) => KcvState::Malformed,
            Ok(Some(stored)) => {
                if stored.len() != 64 || !stored.bytes().all(|b| b.is_ascii_hexdigit()) {
                    KcvState::Malformed
                } else {
                    match self.read_manifest() {
                        Err(_) => KcvState::Malformed,
                        Ok(Some(manifest)) if manifest.record_key_kcv != stored => {
                            KcvState::ManifestDisagrees
                        }
                        _ => match live_key {
                            None => KcvState::LiveKeyMissing,
                            Some(key) if self.verify_kcv(key).unwrap_or(false) => {
                                KcvState::MatchesLiveKey
                            }
                            Some(_) => KcvState::MismatchesLiveKey,
                        },
                    }
                }
            }
        };
        crate::recovery::RecoveryStatus {
            mnemonic_seal_present: mnemonic_seal.is_valid(),
            passphrase_seal_present: passphrase_seal.is_valid(),
            mnemonic_seal,
            passphrase_seal,
            kcv,
            default_space_keystore_exists:
                crate::memory::sqlite_vec_store::at_rest::default_keystore_exists(),
            crypt_passphrase_backed_up,
        }
    }
}

/// Remove a staging file, warning when cleanup fails.
///
/// `NotFound` is silent (the file may already be gone). Any other failure
/// emits a `tracing::warn!` and an `eprintln!` naming the staging path and the
/// io error — never key material — so a CLI user sees the leftover.
fn remove_staging(staging: &Path) {
    if let Err(e) = fs::remove_file(staging) {
        if e.kind() == std::io::ErrorKind::NotFound {
            return;
        }
        tracing::warn!(
            path = %staging.display(),
            error = %e,
            "failed to remove staging file"
        );
        eprintln!(
            "warning: failed to remove staging file {}: {e}",
            staging.display()
        );
    }
}

/// Where the node record key lives, resolved the same way the store resolves it.
fn record_key_path() -> PathBuf {
    crate::memory::sqlite_vec_store::at_rest::record_key_path()
}

/// The live key source, for operator reporting.
///
/// This probe never mints a key when the live file is missing.
pub fn live_key_source() -> KeySourceReport {
    use crate::memory::sqlite_vec_store::at_rest::KeySource as Src;
    match crate::memory::sqlite_vec_store::at_rest::peek_record_key_with_source() {
        (Some(_), Src::Env) => KeySourceReport::Env,
        (Some(_), Src::File(p)) => KeySourceReport::File(p),
        (_, Src::Missing(p)) => KeySourceReport::Missing(p),
        (_, Src::Unavailable(reason)) => KeySourceReport::Unavailable(reason),
        (_, other) => KeySourceReport::Unavailable(format!("{other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_A: [u8; 32] = [0xA1; 32];
    const KEY_B: [u8; 32] = [0xB2; 32];

    #[test]
    fn restore_requires_kcv_before_installing() {
        crate::isolate_test_process!();
        let dir = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(dir.path().join("recovery"));
        let target = dir.path().join("node").join("record.key");
        let outcome = store
            .restore_into(&hex_encode(KEY_A), &target, None)
            .unwrap();
        assert_eq!(
            outcome,
            RestoreOutcome::Rejected {
                reason: "kcv_required".to_string(),
            }
        );
        assert!(!target.parent().unwrap().exists());
        assert!(!store.root().exists());
    }

    #[test]
    fn kcv_roundtrip_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(dir.path().join("recovery"));
        store.write_kcv(&KEY_A).unwrap();

        let path = store.root().join(KCV_FILENAME);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert!(store.verify_kcv(&KEY_A).unwrap());
        assert!(!store.verify_kcv(&KEY_B).unwrap());
    }

    /// AC: an absent KCV means "unverified", not "verified" and not "wrong".
    #[test]
    fn absent_kcv_reports_unverified() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(dir.path().join("recovery"));
        assert!(!store.verify_kcv(&KEY_A).unwrap());
    }

    /// AC: a wrong key is rejected and the live file is untouched.
    #[test]
    fn wrong_key_is_rejected_without_touching_the_target() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(dir.path().join("recovery"));
        store.write_kcv(&KEY_A).unwrap();

        let target = dir.path().join("node").join("record.key");
        let outcome = store
            .restore_into(&hex_encode(KEY_B), &target, Some(&encode_kcv(&KEY_A)))
            .unwrap();

        assert!(!outcome.installed());
        assert!(
            !target.exists(),
            "a rejected restore must not create the target"
        );
    }

    /// AC: the correct key is installed atomically.
    #[test]
    fn correct_key_is_installed() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(dir.path().join("recovery"));
        let target = dir.path().join("node").join("record.key");

        let outcome = store
            .restore_into(&hex_encode(KEY_A), &target, Some(&encode_kcv(&KEY_A)))
            .unwrap();
        assert!(outcome.installed());
        assert_eq!(fs::read_to_string(&target).unwrap(), hex_encode(KEY_A));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    /// The invariant: restore never clobbers an existing key file.
    #[test]
    fn restore_refuses_to_overwrite_an_existing_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(dir.path().join("recovery"));
        let target = dir.path().join("record.key");
        fs::write(&target, "existing-key-material").unwrap();

        let outcome = store
            .restore_into(&hex_encode(KEY_A), &target, None)
            .unwrap();
        assert!(!outcome.installed());
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "existing-key-material",
            "an existing key must survive a restore attempt verbatim"
        );
    }

    /// A wrong key must not leave staging files behind.
    #[test]
    fn failed_restore_leaves_no_staging_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(dir.path().join("recovery"));
        let target = dir.path().join("record.key");

        let outcome = store
            .restore_into(&hex_encode(KEY_B), &target, Some(&encode_kcv(&KEY_A)))
            .unwrap();
        assert!(!outcome.installed());
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(CANDIDATE_SUFFIX))
            .collect();
        assert!(leftovers.is_empty(), "staging leftovers: {leftovers:?}");
    }

    /// AC: a malformed candidate is rejected, never installed.
    #[test]
    fn malformed_candidate_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(dir.path().join("recovery"));
        let target = dir.path().join("record.key");

        for bad in ["zz", "ab", &"aa".repeat(31), &"aa".repeat(33)] {
            let outcome = store.restore_into(bad, &target, None).unwrap();
            assert!(!outcome.installed(), "accepted malformed key {bad}");
        }
        assert!(!target.exists());
    }

    /// The manifest on disk must contain KCVs, not keys.
    #[test]
    fn manifest_file_contains_no_key_material() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(dir.path().join("recovery"));
        let manifest = RecoveryManifest::new(&KEY_A, Some(&KEY_B));
        store.write_manifest(&manifest).unwrap();

        let raw = fs::read_to_string(store.root().join("recovery-manifest.json")).unwrap();
        assert!(!raw.contains(&hex_encode(KEY_A)));
        assert!(!raw.contains(&hex_encode(KEY_B)));
        assert_eq!(store.read_manifest().unwrap().unwrap(), manifest);
    }

    /// Recovery is only reported as real when the crypt passphrase is accounted for.
    #[test]
    fn status_does_not_claim_recoverability_without_the_crypt_passphrase() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(dir.path().join("recovery"));
        store
            .write_manifest(&RecoveryManifest::new(&KEY_A, None))
            .unwrap();
        store.write_kcv(&KEY_A).unwrap();

        // A manifest and KCV alone do not establish that any seal exists.
        assert!(!store.status(Some(true), Some(&KEY_A)).is_recoverable());
        let (words, _) = MnemonicPath::generate().unwrap();
        store.write_mnemonic_seal(&KEY_A, &words).unwrap();
        assert!(!store.status(Some(false), Some(&KEY_A)).is_recoverable());
        assert!(store.status(Some(true), Some(&KEY_A)).is_recoverable());
        assert!(!store.status(None, Some(&KEY_A)).is_recoverable());
    }

    #[test]
    fn seal_inspection_checks_structure_and_known_kdf() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seal.json");
        assert_eq!(
            inspect_seal(&path, SealKind::Passphrase),
            SealHealth::Absent
        );
        fs::write(&path, b"{").unwrap();
        assert_eq!(
            inspect_seal(&path, SealKind::Passphrase),
            SealHealth::Malformed("json_invalid")
        );
        let seal = PassphraseSeal {
            salt: "aa".repeat(16),
            nonce: "aa".repeat(12),
            ciphertext: "aa".repeat(48),
            params: serde_json::json!({
                "variant": "argon2id", "domain": "xavier-recovery-passphrase-v1",
                "m_cost": 19456, "t_cost": 2, "p_cost": 1,
            })
            .to_string(),
        };
        for (field, value, reason) in [
            ("salt", "zz", "salt"),
            ("nonce", "aa", "nonce"),
            ("ciphertext", "aa", "ciphertext_len"),
            ("params", "{}", "kdf_params_unknown"),
        ] {
            let mut bad = serde_json::to_value(&seal).unwrap();
            bad[field] = serde_json::Value::String(value.to_string());
            fs::write(&path, serde_json::to_vec(&bad).unwrap()).unwrap();
            assert_eq!(
                inspect_seal(&path, SealKind::Passphrase),
                SealHealth::Malformed(reason)
            );
        }
        write_private_file(&path, &serde_json::to_vec(&seal).unwrap()).unwrap();
        #[cfg(unix)]
        assert_eq!(
            inspect_seal(&path, SealKind::Passphrase),
            SealHealth::Valid { mode_0600: true }
        );
        let mut mnemonic = MnemonicSeal {
            salt: seal.salt,
            nonce: seal.nonce,
            ciphertext: seal.ciphertext,
            kdf: MnemonicPath::kdf_label(),
        };
        fs::write(&path, serde_json::to_vec(&mnemonic).unwrap()).unwrap();
        assert!(inspect_seal(&path, SealKind::Mnemonic).is_valid());
        mnemonic.kdf.push('x');
        fs::write(&path, serde_json::to_vec(&mnemonic).unwrap()).unwrap();
        assert_eq!(
            inspect_seal(&path, SealKind::Mnemonic),
            SealHealth::Malformed("kdf_unknown")
        );
        fs::write(&path, vec![b' '; 65537]).unwrap();
        assert_eq!(
            inspect_seal(&path, SealKind::Mnemonic),
            SealHealth::Malformed("too_large")
        );
    }

    #[cfg(unix)]
    #[test]
    fn seal_inspection_rejects_symlinks_and_reports_unsafe_modes() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seal.json");
        let link = dir.path().join("link.json");
        symlink(&path, &link).unwrap();
        assert_eq!(
            inspect_seal(&link, SealKind::Mnemonic),
            SealHealth::Malformed("not_regular_file")
        );
        fs::create_dir(&path).unwrap();
        assert_eq!(
            inspect_seal(&path, SealKind::Mnemonic),
            SealHealth::Malformed("not_regular_file")
        );
        fs::remove_dir(&path).unwrap();
        let seal = MnemonicSeal {
            salt: "aa".repeat(16),
            nonce: "aa".repeat(12),
            ciphertext: "aa".repeat(48),
            kdf: MnemonicPath::kdf_label(),
        };
        fs::write(&path, serde_json::to_vec(&seal).unwrap()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            inspect_seal(&path, SealKind::Mnemonic),
            SealHealth::Valid { mode_0600: false }
        );
    }

    #[test]
    fn status_distinguishes_kcv_health_and_actual_seals() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(dir.path());
        assert_eq!(store.status(Some(true), Some(&KEY_A)).kcv, KcvState::Absent);
        store.write_kcv(&KEY_A).unwrap();
        assert_eq!(
            store.status(Some(true), Some(&KEY_A)).kcv,
            KcvState::MatchesLiveKey
        );
        assert_eq!(
            store.status(Some(true), Some(&KEY_B)).kcv,
            KcvState::MismatchesLiveKey
        );
        assert_eq!(store.status(Some(true), None).kcv, KcvState::LiveKeyMissing);
        assert!(!store.status(Some(true), None).is_recoverable());
        let seal = MnemonicSeal {
            salt: "aa".repeat(16),
            nonce: "aa".repeat(12),
            ciphertext: "aa".repeat(48),
            kdf: MnemonicPath::kdf_label(),
        };
        write_private_file(
            &store.root().join(MNEMONIC_SEAL_FILE),
            &serde_json::to_vec(&seal).unwrap(),
        )
        .unwrap();
        let status = store.status(Some(true), None);
        assert!(status.mnemonic_seal_present);
        assert!(!status.passphrase_seal_present);
        assert!(status.is_recoverable());
        assert!(!store.status(Some(true), Some(&KEY_B)).is_recoverable());
        store
            .write_manifest(&RecoveryManifest::new(&KEY_B, None))
            .unwrap();
        assert_eq!(
            store.status(Some(true), Some(&KEY_A)).kcv,
            KcvState::ManifestDisagrees
        );
        fs::write(store.root().join(KCV_FILENAME), "bad").unwrap();
        assert_eq!(
            store.status(Some(true), Some(&KEY_A)).kcv,
            KcvState::Malformed
        );
    }

    #[test]
    fn mnemonic_seal_write_and_unseal_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(dir.path().join("recovery"));
        let (words, _) = MnemonicPath::generate().unwrap();
        store.write_kcv(&KEY_A).unwrap();

        let seal_path = store.write_mnemonic_seal(&KEY_A, &words).unwrap();
        assert!(seal_path.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&seal_path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        let unsealed = store.unseal_mnemonic(&words).unwrap();
        assert_eq!(unsealed, KEY_A);

        let (wrong_words, _) = MnemonicPath::generate().unwrap();
        assert!(store.unseal_mnemonic(&wrong_words).is_err());
    }

    #[test]
    fn passphrase_seal_write_and_unseal_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(dir.path().join("recovery"));
        let pass = "test-passphrase-sufficiently-long-123";
        store.write_kcv(&KEY_A).unwrap();

        let seal_path = store.write_passphrase_seal(&KEY_A, pass).unwrap();
        assert!(seal_path.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&seal_path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        let unsealed = store.unseal_passphrase(pass).unwrap();
        assert_eq!(unsealed, KEY_A);

        assert!(store
            .unseal_passphrase("wrong-passphrase-still-long-123")
            .is_err());
    }

    #[test]
    fn unseal_fails_on_kcv_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(dir.path().join("recovery"));
        let pass = "test-passphrase-sufficiently-long-123";
        // Seal KEY_A
        store.write_passphrase_seal(&KEY_A, pass).unwrap();
        // But store KCV for KEY_B
        store.write_kcv(&KEY_B).unwrap();

        let err = store.unseal_passphrase(pass).unwrap_err();
        assert!(err
            .to_string()
            .contains("key-check value verification failed"));
    }

    /// A seal that does not reopen to the sealed key must not be installed and
    /// must not replace the previous, good seal.
    #[test]
    fn unverifiable_seal_is_rejected_and_previous_seal_kept() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(dir.path().join("recovery"));
        let pass = "test-passphrase-sufficiently-long-123";
        let good = store.write_passphrase_seal(&KEY_A, pass).unwrap();
        let before = fs::read(&good).unwrap();

        let seal = PassphrasePath::seal(&KEY_A, pass).unwrap();
        let err = store
            .write_verified_seal(
                PASSPHRASE_SEAL_FILE,
                &seal,
                &KEY_A,
                |_s: &PassphraseSeal| Ok(KEY_B),
            )
            .unwrap_err();

        assert!(format!("{err:#}").contains("does not match"), "{err:#}");
        assert_eq!(fs::read(&good).unwrap(), before, "previous seal clobbered");
        let prefix = format!("{PASSPHRASE_SEAL_FILE}{CANDIDATE_SUFFIX}");
        let candidate_leftovers: Vec<_> = fs::read_dir(store.root())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with(&prefix))
            .collect();
        assert!(
            candidate_leftovers.is_empty(),
            "staging files left behind: {candidate_leftovers:?}"
        );
    }

    #[test]
    fn concurrent_seal_writers_each_install_a_verified_seal() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(dir.path().join("recovery"));
        let pairs: Vec<([u8; 32], String)> = (0..8)
            .map(|i| {
                let mut key = [0u8; 32];
                key.fill(i as u8 + 1);
                let pass = format!("test-passphrase-concurrent-seal-{i}");
                (key, pass)
            })
            .collect();

        std::thread::scope(|s| {
            let mut handles = Vec::new();
            for (key, pass) in &pairs {
                let store_ref = &store;
                handles.push(s.spawn(move || store_ref.write_passphrase_seal(key, pass)));
            }
            for h in handles {
                assert!(h.join().unwrap().is_ok());
            }
        });

        let mut matches = 0;
        for (key, pass) in &pairs {
            if let Ok(unsealed) = store.unseal_passphrase(pass) {
                assert_eq!(&unsealed, key);
                matches += 1;
            }
        }
        assert_eq!(matches, 1, "installed seal must open with exactly one pair");

        let leftovers: Vec<_> = fs::read_dir(store.root())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(CANDIDATE_SUFFIX))
            .collect();
        assert!(leftovers.is_empty(), "candidate leftovers: {leftovers:?}");
    }

    #[test]
    fn concurrent_new_seals_have_exactly_one_winner_and_preserve_it() {
        for mnemonic in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let store = RecoveryStore::at(dir.path().join("recovery"));
            let pairs: Vec<_> = (0..3)
                .map(|i| {
                    let key = [i + 1; 32];
                    let secret = if mnemonic {
                        MnemonicPath::generate().unwrap().0
                    } else {
                        format!("concurrent-new-passphrase-{i}")
                    };
                    (key, secret)
                })
                .collect();
            let barrier = std::sync::Barrier::new(pairs.len());
            let successes = std::thread::scope(|scope| {
                let handles: Vec<_> = pairs
                    .iter()
                    .enumerate()
                    .map(|(index, (key, secret))| {
                        let store = &store;
                        let barrier = &barrier;
                        scope.spawn(move || {
                            barrier.wait();
                            let result = if mnemonic {
                                store.write_mnemonic_seal_new(key, secret)
                            } else {
                                store.write_passphrase_seal_new(key, secret)
                            };
                            result.ok().map(|path| (index, path))
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .filter_map(|handle| handle.join().unwrap())
                    .collect::<Vec<_>>()
            });
            assert_eq!(
                successes.len(),
                1,
                "exactly one no-clobber install must win"
            );
            let (winner, path) = &successes[0];
            let before = fs::read(path).unwrap();
            for (index, (key, secret)) in pairs.iter().enumerate() {
                let opened = if mnemonic {
                    store.unseal_mnemonic(secret)
                } else {
                    store.unseal_passphrase(secret)
                };
                if index == *winner {
                    assert_eq!(opened.unwrap(), *key);
                } else {
                    assert!(
                        opened.is_err(),
                        "a losing secret must not open the installed seal"
                    );
                }
                let retry = if mnemonic {
                    store.write_mnemonic_seal_new(key, secret)
                } else {
                    store.write_passphrase_seal_new(key, secret)
                };
                assert!(retry.is_err(), "every retry must refuse the existing seal");
                assert_eq!(
                    fs::read(path).unwrap(),
                    before,
                    "winning seal was overwritten"
                );
            }
            let leftovers: Vec<_> = fs::read_dir(store.root())
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|name| name.contains(CANDIDATE_SUFFIX))
                .collect();
            assert!(leftovers.is_empty(), "candidate leftovers: {leftovers:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn restore_into_refuses_dangling_symlink_target() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecoveryStore::at(dir.path().join("recovery"));
        let target = dir.path().join("data").join("record.key");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        let nowhere = dir.path().join("nowhere");
        std::os::unix::fs::symlink(&nowhere, &target).unwrap();
        let outcome = store
            .restore_into(&hex_encode(KEY_A), &target, None)
            .unwrap();
        match &outcome {
            RestoreOutcome::Rejected { reason } => assert_eq!(reason, "target_exists"),
            other => panic!("expected Rejected, got {other:?}"),
        }
        assert!(!nowhere.exists());
        assert!(fs::symlink_metadata(&target)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn remove_staging_is_silent_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("no-such-candidate");
        remove_staging(&missing);
    }

    #[cfg(unix)]
    #[test]
    fn remove_staging_warns_but_does_not_panic_on_error() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        fs::create_dir(&locked).unwrap();
        let victim = locked.join("victim.candidate");
        fs::write(&victim, "staging").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();
        remove_staging(&victim);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
        if victim.exists() {
            assert!(victim.exists());
        }
        // Else running as root: unlink succeeded despite the mode; no assertion.
    }
}
