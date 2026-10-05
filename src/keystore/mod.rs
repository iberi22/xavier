//! Keystore infrastructure module for Xavier
//!
//! Handles master key management, persistence (keyring + fallback), and derivation of
//! encryption keys using HKDF-SHA256.

use anyhow::{anyhow, Result};
use hkdf::Hkdf;
use keyring::Entry;
use rand::RngCore;
use sha2::Sha256;
use std::fs;
use std::path::{Path, PathBuf};

use crate::crypto::encryption::{aes_decrypt, aes_encrypt, EncryptionError, NonceBytes};
use crate::utils::crypto::{hex_decode, hex_encode};

const SERVICE_NAME: &str = "xavier-memory-runtime";
const MASTER_KEY_ENTRY: &str = "master-key";
pub const MASTER_KEY_LEN: usize = 32; // 256 bits

/// Length of the derived fallback wrapping key (AES-256-GCM KEK).
const WRAPPING_KEY_LEN: usize = 32;

/// Compile-time guarantee that the wrapping key is the AES-256 size the master
/// key length already implies; a mismatch would not compile.
const _: () = assert!(WRAPPING_KEY_LEN == MASTER_KEY_LEN);

/// Domain separator of the current fallback wrapping-key derivation.
const WRAPPING_KEY_DOMAIN_V2: &[u8] = b"xavier-fallback-wrapping-key-v2";
/// Salt of the current derivation.
const FALLBACK_SALT_V2: &[u8] = b"xavier-fallback-salt-v2";
/// Salt of the legacy derivation (host name only), kept byte-exact for reads.
const FALLBACK_SALT_V1: &[u8] = b"xavier-fallback-salt-v1";

/// Create `dir` (recursively) with `0700` from the first syscall so the umask
/// never widens the window; pre-existing directories are tightened after.
pub fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Write the encrypted master key creating the file with `0600` atomically
/// (no create-then-chmod window), then re-assert `0600` for pre-existing files.
pub fn write_private_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Master Key Manager handles the core encryption key for the system.
pub struct MasterKeyManager {
    master_key: [u8; MASTER_KEY_LEN],
}

impl MasterKeyManager {
    /// Load or initialize the master key
    pub fn load_or_init() -> Result<Self> {
        if let Ok(key) = Self::load_from_keyring() {
            return Ok(Self { master_key: key });
        }

        if let Ok(key) = Self::load_from_fallback() {
            // Found in fallback, try to restore to keyring
            let _ = Self::save_to_keyring(&key);
            return Ok(Self { master_key: key });
        }

        // Initialize new master key
        let mut key = [0u8; MASTER_KEY_LEN];
        rand::thread_rng().fill_bytes(&mut key);

        // The master key always keeps its 0600 fallback copy: if the keyring later becomes
        // unavailable (headless service), a missing copy would silently mint a new key and
        // orphan every store encrypted with this one. Per-secret shadow copies are not kept.
        let keyring_res = Self::save_to_keyring(&key);
        if let Err(e) = &keyring_res {
            tracing::debug!("Keyring save failed: {e}; relying on encrypted file");
        }
        match (Self::save_to_fallback(&key), &keyring_res) {
            (Ok(()), _) => {}
            (Err(fb_err), Ok(())) => {
                tracing::warn!("Master key fallback copy not written ({fb_err}); keyring only")
            }
            (Err(fb_err), Err(e)) => {
                return Err(anyhow!(
                    "Failed to persist master key to both keyring ({e}) and fallback storage ({fb_err})"
                ))
            }
        }

        Ok(Self { master_key: key })
    }

    fn load_from_keyring() -> Result<[u8; MASTER_KEY_LEN]> {
        let entry = Entry::new(SERVICE_NAME, MASTER_KEY_ENTRY)?;
        let hex_key = entry.get_password()?;
        let key_vec = hex_decode(&hex_key)?;
        let mut key = [0u8; MASTER_KEY_LEN];
        if key_vec.len() != MASTER_KEY_LEN {
            return Err(anyhow!("Invalid master key length in keyring"));
        }
        key.copy_from_slice(&key_vec);
        Ok(key)
    }

    fn save_to_keyring(key: &[u8; MASTER_KEY_LEN]) -> Result<()> {
        let entry = Entry::new(SERVICE_NAME, MASTER_KEY_ENTRY)?;
        entry.set_password(&hex_encode(key))?;
        Ok(())
    }

    fn get_fallback_path() -> PathBuf {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let path = home.join(".xavier").join("master.key");
        crate::test_support::guard_user_path(&path);
        path
    }

    /// Stable per-installation identity sources, in resolution order.
    ///
    /// Every entry is fixed for the lifetime of an installation, so the derived
    /// wrapping key is identical after a reboot or a restart. Nothing that
    /// changes per boot (uptime, boot time, PIDs) is ever read from here.
    const MACHINE_MATERIAL_PATHS: [&str; 3] = [
        "/etc/machine-id",
        "/var/lib/dbus/machine-id",
        "/sys/class/dmi/id/product_uuid",
    ];

    /// Material mixed in when no machine-id source is readable (containers, sandboxes).
    const NO_MACHINE_MATERIAL: &str = "xavier-no-machine-material";

    /// First non-empty machine material found in `paths`, else the sentinel.
    fn read_machine_material_from(paths: &[&str]) -> String {
        for path in paths {
            if let Ok(raw) = fs::read_to_string(path) {
                let trimmed = raw.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }
        Self::NO_MACHINE_MATERIAL.to_string()
    }

    fn machine_material() -> String {
        Self::read_machine_material_from(&Self::MACHINE_MATERIAL_PATHS)
    }

    fn host_name() -> String {
        use sysinfo::System;
        System::host_name().unwrap_or_else(|| "unknown-host".to_string())
    }

    /// Current wrapping-key derivation: domain-separated host name, stable machine
    /// material and salt. Knowledge of the host name alone is no longer enough to
    /// unwrap `master.key`.
    fn derive_fallback_encryption_key(
        host_name: &str,
        machine_material: &str,
    ) -> [u8; WRAPPING_KEY_LEN] {
        let mut hasher = Sha256::default();
        use sha2::Digest;
        hasher.update(WRAPPING_KEY_DOMAIN_V2);
        hasher.update([0u8]);
        hasher.update(host_name.as_bytes());
        hasher.update([0u8]);
        hasher.update(machine_material.as_bytes());
        hasher.update([0u8]);
        hasher.update(FALLBACK_SALT_V2);

        let mut key = [0u8; WRAPPING_KEY_LEN];
        key.copy_from_slice(&hasher.finalize());
        key
    }

    /// Pre-hardening derivation, `SHA256(host_name || salt-v1)`, with no machine
    /// material. Byte-exact on purpose: it must unwrap files written before the
    /// material was mixed in. Never used to write.
    fn derive_legacy_fallback_encryption_key(host_name: &str) -> [u8; WRAPPING_KEY_LEN] {
        let mut hasher = Sha256::default();
        use sha2::Digest;
        hasher.update(host_name.as_bytes());
        hasher.update(FALLBACK_SALT_V1);

        let mut key = [0u8; WRAPPING_KEY_LEN];
        key.copy_from_slice(&hasher.finalize());
        key
    }

    fn get_fallback_encryption_key() -> [u8; WRAPPING_KEY_LEN] {
        Self::derive_fallback_encryption_key(&Self::host_name(), &Self::machine_material())
    }

    fn get_legacy_fallback_encryption_key() -> [u8; WRAPPING_KEY_LEN] {
        Self::derive_legacy_fallback_encryption_key(&Self::host_name())
    }

    fn load_from_fallback() -> Result<[u8; MASTER_KEY_LEN]> {
        Self::load_from_fallback_at(
            &Self::get_fallback_path(),
            &Self::get_fallback_encryption_key(),
            &Self::get_legacy_fallback_encryption_key(),
        )
    }

    /// Try the current wrapping key, then the legacy one. On legacy success the file
    /// is transparently re-wrapped in place with the current key; it is never
    /// deleted or renamed, and a failed re-wrap is not fatal (it is retried on the
    /// next start).
    fn load_from_fallback_at(
        path: &Path,
        current_key: &[u8; WRAPPING_KEY_LEN],
        legacy_key: &[u8; WRAPPING_KEY_LEN],
    ) -> Result<[u8; MASTER_KEY_LEN]> {
        if !path.exists() {
            return Err(anyhow!("Fallback master key file not found"));
        }

        let encrypted_data = fs::read(path)?;
        let (key, needs_rewrap) = match Self::unwrap_fallback_key(&encrypted_data, current_key)? {
            Some(key) => (key, false),
            None => {
                let key = Self::unwrap_fallback_key(&encrypted_data, legacy_key)?.ok_or_else(
                    || {
                        anyhow!(
                            "Failed to decrypt fallback master key with current or legacy derivation"
                        )
                    },
                )?;
                (key, true)
            }
        };

        if needs_rewrap {
            if let Err(e) = Self::write_fallback_at(&key, path, current_key) {
                tracing::warn!(
                    "Could not re-wrap fallback master key with current derivation: {e}"
                );
            }
        }

        Ok(key)
    }

    /// `Ok(None)` means "wrong wrapping key" (authentication failure), so the caller
    /// can try another derivation. Structural failures (garbled or truncated blob)
    /// are propagated instead of retried.
    fn unwrap_fallback_key(
        encrypted_data: &[u8],
        wrapping_key: &[u8; WRAPPING_KEY_LEN],
    ) -> Result<Option<[u8; MASTER_KEY_LEN]>> {
        match aes_decrypt(encrypted_data, wrapping_key) {
            Ok(plain) if plain.len() == MASTER_KEY_LEN => {
                let mut key = [0u8; MASTER_KEY_LEN];
                key.copy_from_slice(&plain);
                Ok(Some(key))
            }
            Ok(_) => Err(anyhow!("Invalid master key length in fallback")),
            Err(EncryptionError::AuthenticationFailed) => Ok(None),
            Err(e) => Err(anyhow!("Failed to decrypt fallback master key: {}", e)),
        }
    }

    fn save_to_fallback(key: &[u8; MASTER_KEY_LEN]) -> Result<()> {
        let path = Self::get_fallback_path();
        Self::write_fallback_at(key, &path, &Self::get_fallback_encryption_key())
    }

    #[cfg(test)]
    pub(crate) fn save_to_fallback_at(key: &[u8; MASTER_KEY_LEN], path: &Path) -> Result<()> {
        Self::write_fallback_at(key, path, &Self::get_fallback_encryption_key())
    }

    fn write_fallback_at(
        key: &[u8; MASTER_KEY_LEN],
        path: &Path,
        wrapping_key: &[u8; WRAPPING_KEY_LEN],
    ) -> Result<()> {
        if let Some(parent) = path.parent() {
            ensure_private_dir(parent)?;
        }

        let nonce = NonceBytes::generate();
        let encrypted = aes_encrypt(key, wrapping_key, &nonce)
            .map_err(|e| anyhow!("Failed to encrypt fallback master key: {}", e))?;

        write_private_file(path, &encrypted)?;
        Ok(())
    }

    /// Derive a specific key using HKDF-SHA256
    pub fn derive_key(&self, info: &[u8], output: &mut [u8]) -> Result<()> {
        let hkdf = Hkdf::<Sha256>::new(None, &self.master_key);
        hkdf.expand(info, output)
            .map_err(|e| anyhow!("HKDF expansion failed: {}", e))?;
        Ok(())
    }

    /// Get the derived auth database key
    pub fn auth_db_key(&self) -> Result<[u8; 32]> {
        let mut key = [0u8; 32];
        self.derive_key(b"xavier-auth-db-v1", &mut key)?;
        Ok(key)
    }

    /// Get the derived RSA keypair encryption key
    pub fn rsa_key(&self) -> Result<[u8; 32]> {
        let mut key = [0u8; 32];
        self.derive_key(b"xavier-rsa-keypair-v1", &mut key)?;
        Ok(key)
    }

    /// Get the derived secrets vault key
    pub fn vault_key(&self) -> Result<[u8; 32]> {
        let mut key = [0u8; 32];
        self.derive_key(b"xavier-secrets-vault-v1", &mut key)?;
        Ok(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_HOST: &str = "xavier-test-node";
    const TEST_MACHINE_ID: &str = "11111111-2222-3333-4444-555555555555";

    #[test]
    fn test_key_derivation_consistency() {
        let mut master_key = [0u8; 32];
        master_key[0] = 1;
        let manager = MasterKeyManager { master_key };

        let mut key1 = [0u8; 32];
        manager.derive_key(b"test-info", &mut key1).unwrap();

        let mut key2 = [0u8; 32];
        manager.derive_key(b"test-info", &mut key2).unwrap();

        assert_eq!(key1, key2);

        let mut key3 = [0u8; 32];
        manager.derive_key(b"other-info", &mut key3).unwrap();
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_fallback_key_file_is_0600() {
        let tmp = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Start loose so the 0700 assertion proves the helper tightens it.
            std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let key_path = tmp.path().join("master.key");
        let synthetic_key = [42u8; MASTER_KEY_LEN];

        MasterKeyManager::save_to_fallback_at(&synthetic_key, &key_path).unwrap();
        assert!(key_path.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(&key_path).unwrap();
            assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
            let parent_metadata = std::fs::metadata(tmp.path()).unwrap();
            assert_eq!(parent_metadata.permissions().mode() & 0o777, 0o700);
        }
    }

    /// The legacy derivation must stay byte-exact, otherwise already-wrapped
    /// `master.key` files become unreadable.
    #[test]
    fn test_legacy_derivation_is_byte_exact() {
        use sha2::Digest;

        let mut expected = Sha256::default();
        expected.update(b"legacy-node");
        expected.update(b"xavier-fallback-salt-v1");
        let mut expected_key = [0u8; WRAPPING_KEY_LEN];
        expected_key.copy_from_slice(&expected.finalize());

        assert_eq!(
            MasterKeyManager::derive_legacy_fallback_encryption_key("legacy-node"),
            expected_key
        );
    }

    /// AC: derivation changes when only the machine-id material changes.
    #[test]
    fn test_derivation_changes_when_only_machine_material_changes() {
        let other_machine_id = "99999999-8888-7777-6666-555555555555";

        let key = MasterKeyManager::derive_fallback_encryption_key(TEST_HOST, TEST_MACHINE_ID);
        let other = MasterKeyManager::derive_fallback_encryption_key(TEST_HOST, other_machine_id);

        assert_ne!(key, other);
        assert_ne!(
            MasterKeyManager::derive_fallback_encryption_key("other-node", TEST_MACHINE_ID),
            key,
            "host name must still contribute to the derivation"
        );
    }

    /// AC: deterministic across restarts, i.e. the same machine material always
    /// yields the same wrapping key.
    #[test]
    fn test_derivation_is_deterministic_for_the_same_machine() {
        let first = MasterKeyManager::derive_fallback_encryption_key(TEST_HOST, TEST_MACHINE_ID);
        let second = MasterKeyManager::derive_fallback_encryption_key(TEST_HOST, TEST_MACHINE_ID);
        assert_eq!(first, second);

        assert_ne!(
            first,
            MasterKeyManager::derive_legacy_fallback_encryption_key(TEST_HOST),
            "the current derivation must differ from the legacy one"
        );
    }

    /// AC: no boot-time material. The source list is pinned to per-installation
    /// files, and `/proc` (per-boot, volatile) is never a source.
    #[test]
    fn test_machine_material_sources_are_per_installation_files() {
        assert_eq!(
            MasterKeyManager::MACHINE_MATERIAL_PATHS,
            [
                "/etc/machine-id",
                "/var/lib/dbus/machine-id",
                "/sys/class/dmi/id/product_uuid"
            ]
        );
        for path in MasterKeyManager::MACHINE_MATERIAL_PATHS {
            assert!(
                !path.starts_with("/proc/"),
                "per-boot source {path} would break the derivation after a reboot"
            );
        }
    }

    /// Resolution order, whitespace trimming and the missing-source sentinel.
    #[test]
    fn test_machine_material_resolution_order() {
        let tmp = tempfile::tempdir().unwrap();
        let first = tmp.path().join("first-id");
        let second = tmp.path().join("second-id");
        let empty = tmp.path().join("empty-id");
        let missing = tmp.path().join("missing-id");

        fs::write(&first, format!("{TEST_MACHINE_ID}\n")).unwrap();
        fs::write(&second, "second-material\n").unwrap();
        fs::write(&empty, "   \n").unwrap();

        let first_path = first.to_str().unwrap();
        let second_path = second.to_str().unwrap();
        let empty_path = empty.to_str().unwrap();
        let missing_path = missing.to_str().unwrap();

        let paths: Vec<&str> = vec![first_path, second_path, empty_path, missing_path];
        assert_eq!(
            MasterKeyManager::read_machine_material_from(&paths),
            TEST_MACHINE_ID
        );

        let without_first: Vec<&str> = vec![missing_path, second_path];
        assert_eq!(
            MasterKeyManager::read_machine_material_from(&without_first),
            "second-material"
        );

        let only_empty: Vec<&str> = vec![empty_path, missing_path];
        assert_eq!(
            MasterKeyManager::read_machine_material_from(&only_empty),
            MasterKeyManager::NO_MACHINE_MATERIAL
        );
    }

    /// Real host: resolving twice returns the same material, so a restart re-derives
    /// the same wrapping key.
    #[test]
    fn test_machine_material_is_stable_on_this_host() {
        let first = MasterKeyManager::machine_material();
        let second = MasterKeyManager::machine_material();
        assert_eq!(
            first, second,
            "machine material must be stable across calls"
        );
        assert!(!first.is_empty());
    }

    /// AC: a key wrapped with the legacy derivation is still readable and is
    /// re-wrapped in place with the new derivation (0600, no delete/rename).
    #[test]
    fn test_legacy_fallback_file_is_readable_and_rewrapped() {
        let tmp = tempfile::tempdir().unwrap();
        let key_path = tmp.path().join("master.key");

        let current_key =
            MasterKeyManager::derive_fallback_encryption_key(TEST_HOST, TEST_MACHINE_ID);
        let legacy_key = MasterKeyManager::derive_legacy_fallback_encryption_key(TEST_HOST);
        assert_ne!(current_key, legacy_key);

        // Simulate a master.key written before the machine material was mixed in.
        let master = [7u8; MASTER_KEY_LEN];
        let legacy_blob = aes_encrypt(&master, &legacy_key, &NonceBytes::generate()).unwrap();
        fs::write(&key_path, &legacy_blob).unwrap();

        let loaded =
            MasterKeyManager::load_from_fallback_at(&key_path, &current_key, &legacy_key).unwrap();
        assert_eq!(loaded, master);

        let on_disk = fs::read(&key_path).unwrap();
        assert_ne!(on_disk, legacy_blob, "legacy blob must be re-wrapped");
        assert_eq!(
            aes_decrypt(&on_disk, &current_key).unwrap(),
            master.to_vec(),
            "re-wrapped file must open with the current derivation"
        );
        assert!(
            aes_decrypt(&on_disk, &legacy_key).is_err(),
            "re-wrapped file must no longer open with the legacy derivation"
        );
        assert!(
            key_path.exists(),
            "the file must never be deleted or renamed"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::metadata(&key_path).unwrap();
            assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        }
    }

    /// An already-current file is read without touching it.
    #[test]
    fn test_current_fallback_file_is_read_without_rewrap() {
        let tmp = tempfile::tempdir().unwrap();
        let key_path = tmp.path().join("master.key");

        let current_key =
            MasterKeyManager::derive_fallback_encryption_key(TEST_HOST, TEST_MACHINE_ID);
        let legacy_key = MasterKeyManager::derive_legacy_fallback_encryption_key(TEST_HOST);

        let master = [9u8; MASTER_KEY_LEN];
        MasterKeyManager::write_fallback_at(&master, &key_path, &current_key).unwrap();
        let before = fs::read(&key_path).unwrap();

        let loaded =
            MasterKeyManager::load_from_fallback_at(&key_path, &current_key, &legacy_key).unwrap();
        assert_eq!(loaded, master);
        assert_eq!(
            fs::read(&key_path).unwrap(),
            before,
            "an already-current file must be left byte-identical"
        );
    }

    /// A file we cannot decrypt is reported, never silently accepted nor overwritten.
    #[test]
    fn test_undecryptable_fallback_is_reported_and_left_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let key_path = tmp.path().join("master.key");

        let current_key =
            MasterKeyManager::derive_fallback_encryption_key(TEST_HOST, TEST_MACHINE_ID);
        let legacy_key = MasterKeyManager::derive_legacy_fallback_encryption_key(TEST_HOST);
        let foreign_key = [0x5au8; WRAPPING_KEY_LEN];

        let master = [3u8; MASTER_KEY_LEN];
        let foreign_blob = aes_encrypt(&master, &foreign_key, &NonceBytes::generate()).unwrap();
        fs::write(&key_path, &foreign_blob).unwrap();

        let result = MasterKeyManager::load_from_fallback_at(&key_path, &current_key, &legacy_key);
        assert!(result.is_err());
        assert_eq!(
            fs::read(&key_path).unwrap(),
            foreign_blob,
            "a file we cannot decrypt must never be overwritten"
        );
    }
}
