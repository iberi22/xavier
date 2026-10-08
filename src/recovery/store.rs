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
//! 1. write candidate to  <dir>/<name>.candidate   (0600)
//! 2. verify its KCV against the manifest          <- fails here => nothing changes
//! 3. atomic rename(candidate, <dir>/<name>)        <- only after verification
//! ```
//!
//! A failed verification leaves both the live key and the candidate untouched.
//! There is no code path in this module that calls `fs::write` on an existing key
//! file, and [`RestoreOutcome`] reports which of the two cases occurred.
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
    /// Generated because nothing existed.
    Generated(PathBuf),
    /// Nothing available: the node keeps running but writes plaintext.
    Unavailable(String),
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
            let _ = fs::remove_file(&staging);
            return Err(e.context(format!(
                "seal verification failed; {} was left untouched",
                path.display()
            )));
        }

        if let Err(e) = fs::rename(&staging, &path) {
            let _ = fs::remove_file(&staging);
            bail!("cannot install seal at {}: {e}", path.display());
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
    /// `expected_kcv` is the KCV the key must match; pass `None` to skip
    /// verification (not recommended — that is how a wrong key gets installed).
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
                let _ = fs::remove_file(&staging);
                Ok(RestoreOutcome::Installed)
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(&staging);
                Ok(RestoreOutcome::Rejected {
                    reason: "target_exists".to_string(),
                })
            }
            Err(e) => {
                let _ = fs::remove_file(&staging);
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

    /// Report whether a recoverable path exists, without touching any key.
    pub fn status(
        &self,
        crypt_passphrase_backed_up: Option<bool>,
    ) -> crate::recovery::RecoveryStatus {
        let manifest = self.read_manifest().ok().flatten();
        let has_kcv = self.read_kcv().ok().flatten().is_some();
        crate::recovery::RecoveryStatus {
            mnemonic_seal_present: manifest.is_some() && has_kcv,
            passphrase_seal_present: manifest.is_some() && has_kcv,
            crypt_passphrase_backed_up,
        }
    }
}

/// Where the node record key lives, resolved the same way the store resolves it.
fn record_key_path() -> PathBuf {
    crate::memory::sqlite_vec_store::at_rest::record_key_path()
}

/// The live key source, for operator reporting.
///
/// `Generated` is kept distinct from `File` on purpose: a key that was just
/// minted means every existing row is already unreadable, which is the single
/// most important thing this module can report.
pub fn live_key_source() -> KeySourceReport {
    use crate::memory::sqlite_vec_store::at_rest::KeySource as Src;
    match crate::memory::sqlite_vec_store::at_rest::resolve_record_key_with_source() {
        (Some(_), Src::Env) => KeySourceReport::Env,
        (Some(_), Src::File(p)) => KeySourceReport::File(p),
        (Some(_), Src::Generated(p)) => KeySourceReport::Generated(p),
        (_, Src::Unavailable(reason)) => KeySourceReport::Unavailable(reason),
        (None, other) => KeySourceReport::Unavailable(format!("{other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_A: [u8; 32] = [0xA1; 32];
    const KEY_B: [u8; 32] = [0xB2; 32];

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

        assert!(!store.status(Some(false)).is_recoverable());
        assert!(store.status(Some(true)).is_recoverable());
        assert!(!store.status(None).is_recoverable());
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
}
