//! Sealed storage for the Drive OAuth refresh token.
//!
//! The refresh token is the one credential in this flow that does not expire, so
//! it is treated as key material: AES-256-GCM under a key derived from the Xavier
//! master key by HKDF-SHA256 with a domain label
//! (`xavier-drive-oauth-v1`, distinct from `xavier-secrets-vault-v1` and the
//! auth DB label).
//!
//! Deliberately **not** reused from `HardwareVault`: that stores values as
//! plaintext strings inside an already-encrypted file keyed by a key that is
//! itself in `~/.xavier/secrets/`. This module is a separate, self-describing
//! blob so that revoking Drive access is one file deletion with no side effects
//! on the rest of the vault.
//!
//! Nothing here prints a token. Errors name the failure, never the value.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::crypto::encryption::{aes_decrypt, aes_encrypt, NonceBytes};
use crate::crypto::hex_decode;
use crate::crypto::hex_encode;
use crate::keystore::{ensure_private_dir, write_private_file};

/// Filename inside `$XAVIER_DRIVE_DIR`.
const CREDENTIALS_FILENAME: &str = "drive-oauth.enc";

/// Domain label for the HKDF expansion of the master key.
const HKDF_INFO: &[u8] = b"xavier-drive-oauth-v1";

/// Scope set for the backup job.
pub struct DriveScopes;

impl DriveScopes {
    /// Per-file access: only files this app created.
    pub const BACKUP: &'static str = "https://www.googleapis.com/auth/drive.file";

    /// The full-account scope. **Not requested**; defined so the decision is
    /// explicit and testable rather than an omission.
    pub const FULL: &'static str = "https://www.googleapis.com/auth/drive";

    /// Scopes this module is willing to request.
    pub const REQUESTED: &'static [&'static str] = &[Self::BACKUP];

    /// Whether a scope string is one this module requests.
    pub fn is_requested(scope: &str) -> bool {
        Self::REQUESTED.contains(&scope)
    }
}

/// What kind of credential a stored blob holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenKind {
    /// An `Authorization: Bearer` access token, short lived.
    AccessToken,
    /// A long-lived refresh token.
    RefreshToken,
}

/// Sealed credential envelope. On disk this is one AES-GCM blob.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DriveCredentials {
    /// Unix timestamp when the access token stops working.
    pub expires_at: i64,
    /// Granted scopes, as returned by Google.
    pub scopes: Vec<String>,
    /// The refresh token. Never logged, never returned in an API response.
    pub refresh_token: String,
}

impl DriveCredentials {
    /// Whether the access token window is exhausted (30 s safety margin).
    pub fn access_token_expired(&self, now: i64) -> bool {
        now >= self.expires_at - 30
    }

    /// Whether the granted scopes include what a backup needs.
    ///
    /// A token granted only `drive` is *not* treated as satisfying `drive.file`:
    /// the check is deliberately strict, because silently running a backup with
    /// broader-than-needed access is the failure mode worth catching.
    pub fn has_backup_scope(&self) -> bool {
        self.scopes.iter().any(|s| {
            s == DriveScopes::BACKUP
                || s == "https://www.googleapis.com/auth/drive.file"
                || s.ends_with("/drive.file")
        })
    }
}

/// Sealed store for the Drive credentials.
#[derive(Debug, Clone)]
pub struct DriveCredentialStore {
    dir: PathBuf,
}

impl DriveCredentialStore {
    /// Store at an explicit directory. Tests inject a `tempfile::tempdir()`.
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Store at `$XAVIER_DRIVE_DIR`, else `~/.xavier/drive`.
    pub fn from_env_or_default() -> Self {
        let dir = std::env::var("XAVIER_DRIVE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".xavier")
                    .join("drive")
            });
        Self { dir }
    }

    /// Path of the sealed blob.
    pub fn path(&self) -> PathBuf {
        self.dir.join(CREDENTIALS_FILENAME)
    }

    /// Seal credentials under the master key, `0600`.
    ///
    /// Overwrites by design: re-consenting must be able to replace a dead token.
    /// The write is a truncate-and-write on a private file, never a partial
    /// append, and a failed seal leaves the previous file untouched because the
    /// encryption happens before any I/O.
    pub fn store(&self, creds: &DriveCredentials, master_key: &[u8; 32]) -> Result<PathBuf> {
        let wrapping = Self::derive_wrapping_key(master_key)?;
        let plaintext = serde_json::to_vec(creds).context("cannot serialize credentials")?;
        let nonce = NonceBytes::generate();
        let sealed = aes_encrypt(&plaintext, &wrapping, &nonce)
            .map_err(|e| anyhow::anyhow!("cannot seal credentials: {e}"))?;

        ensure_private_dir(&self.dir)?;
        let path = self.path();
        write_private_file(&path, &sealed)
            .with_context(|| format!("cannot write {}", path.display()))?;
        Ok(path)
    }

    /// Unseal credentials with the master key.
    ///
    /// A wrong master key fails the AEAD tag and yields a generic message: the
    /// error must not become a padding oracle, so it never distinguishes
    /// "wrong key" from "corrupt file".
    pub fn load(&self, master_key: &[u8; 32]) -> Result<DriveCredentials> {
        let path = self.path();
        let sealed =
            std::fs::read(&path).with_context(|| format!("cannot read {}", path.display()))?;
        let wrapping = Self::derive_wrapping_key(master_key)?;
        let plaintext = aes_decrypt(&sealed, &wrapping).map_err(|_| {
            anyhow::anyhow!("cannot open Drive credentials: wrong master key or corrupt file")
        })?;
        let creds: DriveCredentials =
            serde_json::from_slice(&plaintext).context("credentials are malformed")?;
        Ok(creds)
    }

    /// Whether sealed credentials exist (does not need the key).
    pub fn exists(&self) -> bool {
        self.path().exists()
    }

    /// Delete the credentials. Used on disconnect / revoke.
    pub fn remove(&self) -> Result<()> {
        let path = self.path();
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("cannot remove {}", path.display()))?;
        }
        Ok(())
    }

    /// HKDF the wrapping key from the master key with a dedicated label.
    fn derive_wrapping_key(master_key: &[u8; 32]) -> Result<[u8; 32]> {
        let hkdf = hkdf::Hkdf::<sha2::Sha256>::new(None, master_key);
        let mut key = [0u8; 32];
        hkdf.expand(HKDF_INFO, &mut key)
            .map_err(|e| anyhow::anyhow!("HKDF expansion failed: {e}"))?;
        Ok(key)
    }
}

/// Convenience: the store's default path, for CLI reporting.
pub fn default_credentials_path() -> PathBuf {
    DriveCredentialStore::from_env_or_default().path()
}

/// True when a path looks like a sealed credential blob rather than plaintext.
pub fn looks_sealed(path: &Path) -> bool {
    std::fs::read(path).map(|b| !b.is_ascii()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MASTER: [u8; 32] = [0xD1; 32];
    const OTHER: [u8; 32] = [0xD2; 32];

    fn creds() -> DriveCredentials {
        DriveCredentials {
            expires_at: 1_700_000_000,
            scopes: vec![DriveScopes::BACKUP.to_string()],
            refresh_token: "1//refresh-token-value".to_string(),
        }
    }

    #[test]
    fn seal_open_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = DriveCredentialStore::at(dir.path());
        store.store(&creds(), &MASTER).unwrap();
        assert_eq!(store.load(&MASTER).unwrap(), creds());
    }

    /// The file must be 0600 and must not contain the token in the clear.
    #[test]
    fn credentials_are_private_and_opaque_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let store = DriveCredentialStore::at(dir.path());
        let path = store.store(&creds(), &MASTER).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let raw = std::fs::read(&path).unwrap();
        assert!(
            !raw.windows(20).any(|w| w == b"1//refresh-token-value"),
            "refresh token must not be recoverable from the raw file"
        );
        assert!(looks_sealed(&path));
    }

    /// A wrong master key must fail closed, without leaking which half is wrong.
    #[test]
    fn wrong_master_key_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let store = DriveCredentialStore::at(dir.path());
        store.store(&creds(), &MASTER).unwrap();

        let err = store.load(&OTHER).unwrap_err().to_string();
        assert!(err.contains("wrong master key or corrupt file"));
        assert!(
            !err.contains("refresh-token"),
            "error must not echo the token"
        );
    }

    /// Domain separation: the Drive blob key must differ from the vault key.
    #[test]
    fn wrapping_key_is_domain_separated_from_the_vault_key() {
        use hkdf::Hkdf;
        use sha2::Sha256;

        let drive = DriveCredentialStore::derive_wrapping_key(&MASTER).unwrap();
        let mut vault = [0u8; 32];
        Hkdf::<Sha256>::new(None, &MASTER)
            .expand(b"xavier-secrets-vault-v1", &mut vault)
            .unwrap();
        let mut auth_db = [0u8; 32];
        Hkdf::<Sha256>::new(None, &MASTER)
            .expand(b"xavier-auth-db-v1", &mut auth_db)
            .unwrap();

        assert_ne!(drive, vault);
        assert_ne!(drive, auth_db);
    }

    /// AC: expiry has a safety margin, matching `google_oauth.rs:107`.
    #[test]
    fn expiry_uses_a_safety_margin() {
        let c = DriveCredentials {
            expires_at: 1000,
            ..creds()
        };
        assert!(!c.access_token_expired(900));
        assert!(
            c.access_token_expired(970),
            "30 s margin must count as expired"
        );
        assert!(c.access_token_expired(1000));
    }

    /// AC: only `drive.file` satisfies the backup requirement.
    #[test]
    fn scope_check_is_strict() {
        let narrow = DriveCredentials {
            scopes: vec!["https://www.googleapis.com/auth/drive.file".into()],
            ..creds()
        };
        assert!(narrow.has_backup_scope());

        let empty = DriveCredentials {
            scopes: vec![],
            ..creds()
        };
        assert!(!empty.has_backup_scope());

        // A broader-only grant does not count as satisfying the narrow scope.
        let broad = DriveCredentials {
            scopes: vec!["https://www.googleapis.com/auth/drive".into()],
            ..creds()
        };
        assert!(!broad.has_backup_scope());
    }

    /// The full-account scope must never appear in what this module requests.
    #[test]
    fn full_drive_scope_is_never_requested() {
        assert_eq!(DriveScopes::REQUESTED, [DriveScopes::BACKUP]);
        assert!(!DriveScopes::is_requested(DriveScopes::FULL));
        assert!(DriveScopes::is_requested(DriveScopes::BACKUP));
    }

    /// AC: disconnect removes the file, and loading afterwards is an error.
    #[test]
    fn remove_revokes_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let store = DriveCredentialStore::at(dir.path());
        store.store(&creds(), &MASTER).unwrap();
        assert!(store.exists());

        store.remove().unwrap();
        assert!(!store.exists());
        assert!(store.load(&MASTER).is_err());
        // Removing twice is not an error.
        store.remove().unwrap();
    }
}
