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

/// Create a new private file with mode `0600` atomically using `create_new(true)`
/// and `O_NOFOLLOW` on Unix. Fails if the path already exists (including a dangling symlink).
pub fn create_private_file_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
        opts.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = opts.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

/// Master Key Manager handles the core encryption key for the system.
pub struct MasterKeyManager {
    master_key: [u8; MASTER_KEY_LEN],
}

impl MasterKeyManager {
    /// Load or initialize the master key
    pub fn load_or_init() -> Result<Self> {
        let keyring = match Self::load_from_keyring() {
            Ok(key) => Ok(Some(key)),
            Err(e)
                if matches!(
                    e.downcast_ref::<keyring::Error>(),
                    Some(keyring::Error::NoEntry)
                ) =>
            {
                Ok(None)
            }
            Err(e) => Err(e),
        };
        Self::load_or_init_at(
            &Self::get_fallback_path(),
            &Self::get_fallback_encryption_key(),
            &Self::get_legacy_fallback_encryption_key(),
            &Self::node_data_dir(),
            keyring,
            Self::save_to_keyring,
            create_private_file_new,
        )
    }

    /// Data dir the node record key and the store live under.
    fn node_data_dir() -> PathBuf {
        crate::memory::sqlite_vec_store::at_rest::record_key_path()
            .ancestors()
            .nth(2)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    }

    /// True when `data_dir` already holds a node record key or a store database,
    /// i.e. a new master key could not read what is already there.
    fn data_dir_has_existing_material(data_dir: &Path) -> bool {
        if data_dir.join("node").join("record.key").exists() {
            return true;
        }
        match fs::read_dir(data_dir) {
            Ok(entries) => entries.flatten().any(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.starts_with("vec-store.sqlite3") || name.starts_with("memory-store.sqlite3")
            }),
            Err(e) => e.kind() != std::io::ErrorKind::NotFound,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn load_or_init_at(
        path: &Path,
        current_key: &[u8; WRAPPING_KEY_LEN],
        legacy_key: &[u8; WRAPPING_KEY_LEN],
        data_dir: &Path,
        keyring: Result<Option<[u8; MASTER_KEY_LEN]>>,
        save_keyring: impl FnOnce(&[u8; MASTER_KEY_LEN]) -> Result<()>,
        create_fallback: impl FnOnce(&Path, &[u8]) -> std::io::Result<()>,
    ) -> Result<Self> {
        let existing: Result<Option<[u8; MASTER_KEY_LEN]>> = match fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
            Ok(_) => Self::load_from_fallback_at(path, current_key, legacy_key, true).map(Some),
        };
        if let Ok(Some(key)) = &keyring {
            match existing {
                Ok(Some(fallback)) if fallback != *key => {
                    return Err(Self::init_error("keyring and fallback master keys differ"));
                }
                Err(e) => tracing::warn!(
                    "Fallback master key file unusable ({e}); using the keyring key, file left untouched"
                ),
                _ => {}
            }
            return Ok(Self { master_key: *key });
        }
        match existing {
            Err(e) => return Err(Self::init_error(e)),
            Ok(Some(key)) => {
                let _ = save_keyring(&key);
                return Ok(Self { master_key: key });
            }
            Ok(None) => {}
        }
        // No key anywhere: only mint when nothing else could depend on an older key.
        if Self::data_dir_has_existing_material(data_dir) {
            let state = match &keyring {
                Err(e) => format!("keyring unavailable ({e})"),
                Ok(_) => "keyring has no entry".to_string(),
            };
            return Err(Self::init_error(format!(
                "{state} and existing node data found in {}; unlock the keyring or restore master.key with `xavier recovery restore`",
                data_dir.display()
            )));
        }
        if let Err(e) = &keyring {
            tracing::warn!(
                "Keyring unavailable ({e}); minting a new master key in the encrypted file"
            );
        }

        let mut key = [0u8; MASTER_KEY_LEN];
        rand::thread_rng().fill_bytes(&mut key);
        if let Some(parent) = path.parent() {
            ensure_private_dir(parent)?;
        }
        let encrypted = aes_encrypt(&key, current_key, &NonceBytes::generate())?;
        match create_fallback(path, &encrypted) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Another writer created the file first: adopt its key.
                key = Self::load_from_fallback_at(path, current_key, legacy_key, true)
                    .map_err(Self::init_error)?;
            }
            Err(e) => return Err(Self::init_error(e)),
        }
        if let Err(e) = save_keyring(&key) {
            tracing::debug!("Keyring save failed: {e}; relying on encrypted file");
        }
        Ok(Self { master_key: key })
    }

    fn init_error(error: impl std::fmt::Display) -> anyhow::Error {
        anyhow!(
            "Master key initialization failed closed: {error}. Use xavier recovery or restore the original host identity before retrying"
        )
    }

    /// Read an existing master key without minting, saving, or rewrapping it.
    pub fn load_existing() -> Result<Option<Self>> {
        if let Ok(key) = Self::load_from_keyring() {
            return Ok(Some(Self { master_key: key }));
        }
        let path = Self::get_fallback_path();
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
            Ok(_) => Self::load_from_fallback(false).map(|key| Some(Self { master_key: key })),
        }
    }

    /// Check value of the master key itself, without exposing its bytes.
    pub fn kcv_hex(&self) -> String {
        crate::recovery::kcv::encode_kcv(&self.master_key)
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

    fn load_from_fallback(rewrap: bool) -> Result<[u8; MASTER_KEY_LEN]> {
        Self::load_from_fallback_at(
            &Self::get_fallback_path(),
            &Self::get_fallback_encryption_key(),
            &Self::get_legacy_fallback_encryption_key(),
            rewrap,
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
        rewrap: bool,
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

        if needs_rewrap && rewrap {
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
impl MasterKeyManager {
    /// Build a manager directly from a fixed key without touching the OS
    /// keyring or any file. Test-only.
    pub(crate) fn from_key_for_test(master_key: [u8; MASTER_KEY_LEN]) -> Self {
        Self { master_key }
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
            MasterKeyManager::load_from_fallback_at(&key_path, &current_key, &legacy_key, true)
                .unwrap();
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

    #[test]
    fn test_legacy_fallback_probe_does_not_rewrap() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("master.key");
        let current = MasterKeyManager::derive_fallback_encryption_key(TEST_HOST, TEST_MACHINE_ID);
        let legacy = MasterKeyManager::derive_legacy_fallback_encryption_key(TEST_HOST);
        let key = [0x37; MASTER_KEY_LEN];
        let blob = aes_encrypt(&key, &legacy, &NonceBytes::generate()).unwrap();
        fs::write(&path, &blob).unwrap();
        let loaded =
            MasterKeyManager::load_from_fallback_at(&path, &current, &legacy, false).unwrap();
        assert_eq!(loaded, key);
        assert_eq!(fs::read(&path).unwrap(), blob);
        let manager = MasterKeyManager::from_key_for_test(key);
        assert_eq!(manager.kcv_hex(), crate::recovery::kcv::encode_kcv(&key));
        assert_ne!(
            manager.kcv_hex(),
            crate::recovery::kcv::encode_kcv(&manager.vault_key().unwrap())
        );
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
            MasterKeyManager::load_from_fallback_at(&key_path, &current_key, &legacy_key, true)
                .unwrap();
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

        let result =
            MasterKeyManager::load_from_fallback_at(&key_path, &current_key, &legacy_key, true);
        assert!(result.is_err());
        assert_eq!(
            fs::read(&key_path).unwrap(),
            foreign_blob,
            "a file we cannot decrypt must never be overwritten"
        );
    }
    struct Init {
        _tmp: tempfile::TempDir,
        path: PathBuf,
        data_dir: PathBuf,
        current: [u8; WRAPPING_KEY_LEN],
        legacy: [u8; WRAPPING_KEY_LEN],
    }

    impl Init {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let data_dir = tmp.path().join("data");
            fs::create_dir_all(&data_dir).unwrap();
            Self {
                path: tmp.path().join("home").join("master.key"),
                data_dir,
                current: MasterKeyManager::derive_fallback_encryption_key(
                    TEST_HOST,
                    TEST_MACHINE_ID,
                ),
                legacy: MasterKeyManager::derive_legacy_fallback_encryption_key(TEST_HOST),
                _tmp: tmp,
            }
        }

        fn write_file(&self, key: &[u8; MASTER_KEY_LEN]) -> Vec<u8> {
            fs::create_dir_all(self.path.parent().unwrap()).unwrap();
            let blob = aes_encrypt(key, &self.current, &NonceBytes::generate()).unwrap();
            fs::write(&self.path, &blob).unwrap();
            blob
        }

        fn write_foreign_file(&self) -> Vec<u8> {
            fs::create_dir_all(self.path.parent().unwrap()).unwrap();
            let blob = aes_encrypt(
                &[3u8; MASTER_KEY_LEN],
                &[0x5au8; WRAPPING_KEY_LEN],
                &NonceBytes::generate(),
            )
            .unwrap();
            fs::write(&self.path, &blob).unwrap();
            blob
        }

        /// Returns the result plus (keyring saves, create calls).
        fn run(
            &self,
            keyring: Result<Option<[u8; MASTER_KEY_LEN]>>,
        ) -> (
            Result<MasterKeyManager>,
            Option<[u8; MASTER_KEY_LEN]>,
            usize,
        ) {
            let saved = std::cell::Cell::new(None);
            let created = std::cell::Cell::new(0usize);
            let result = MasterKeyManager::load_or_init_at(
                &self.path,
                &self.current,
                &self.legacy,
                &self.data_dir,
                keyring,
                |k| {
                    saved.set(Some(*k));
                    Ok(())
                },
                |p, b| {
                    created.set(created.get() + 1);
                    create_private_file_new(p, b)
                },
            );
            (result, saved.get(), created.get())
        }
    }

    fn keyring_err() -> Result<Option<[u8; MASTER_KEY_LEN]>> {
        Err(anyhow!("keyring locked"))
    }

    #[test]
    fn init_no_file_and_no_entry_mints_private_file() {
        let t = Init::new();
        let (result, saved, created) = t.run(Ok(None));
        let key = result.unwrap().master_key;
        assert_eq!(created, 1);
        assert_eq!(saved, Some(key));
        let blob = fs::read(&t.path).unwrap();
        assert_eq!(aes_decrypt(&blob, &t.current).unwrap(), key.to_vec());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&t.path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn init_keyring_error_mints_only_on_empty_data_dir() {
        let t = Init::new();
        let (result, _, created) = t.run(keyring_err());
        assert!(result.is_ok());
        assert_eq!(created, 1);
        assert!(t.path.exists());
    }

    #[test]
    fn init_keyring_error_with_record_key_fails_closed() {
        let t = Init::new();
        fs::create_dir_all(t.data_dir.join("node")).unwrap();
        fs::write(t.data_dir.join("node").join("record.key"), b"x").unwrap();
        let (result, saved, created) = t.run(keyring_err());
        let msg = result.err().expect("must fail closed").to_string();
        assert!(msg.contains("xavier recovery restore"), "{msg}");
        assert_eq!(created, 0);
        assert!(saved.is_none());
        assert!(!t.path.exists());
    }

    #[test]
    fn init_no_entry_with_record_key_fails_closed() {
        let t = Init::new();
        fs::create_dir_all(t.data_dir.join("node")).unwrap();
        fs::write(t.data_dir.join("node").join("record.key"), b"x").unwrap();
        let (result, saved, created) = t.run(Ok(None));
        let msg = result.err().expect("must fail closed").to_string();
        assert!(msg.contains("xavier recovery restore"), "{msg}");
        assert_eq!(created, 0);
        assert!(saved.is_none());
        assert!(!t.path.exists());
    }

    #[test]
    fn init_no_entry_with_store_database_fails_closed() {
        let t = Init::new();
        fs::write(t.data_dir.join("memory-store.sqlite3-wal"), b"db").unwrap();
        let (result, saved, created) = t.run(Ok(None));
        assert!(result.is_err());
        assert_eq!(created, 0);
        assert!(saved.is_none());
        assert!(!t.path.exists());
    }

    #[test]
    fn create_private_file_new_never_truncates_or_follows() {
        let t = Init::new();
        fs::create_dir_all(t.path.parent().unwrap()).unwrap();
        create_private_file_new(&t.path, b"first").unwrap();
        let err = create_private_file_new(&t.path, b"second").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&t.path).unwrap(), b"first");
        #[cfg(unix)]
        {
            let link = t.path.with_file_name("link.key");
            std::os::unix::fs::symlink(t.path.with_file_name("nowhere.key"), &link).unwrap();
            assert!(create_private_file_new(&link, b"x").is_err());
            assert!(!t.path.with_file_name("nowhere.key").exists());
        }
    }

    #[test]
    fn init_mint_path_does_not_overwrite_a_file_that_appears_first() {
        let t = Init::new();
        let other = [0x12u8; MASTER_KEY_LEN];
        let blob = aes_encrypt(&other, &t.current, &NonceBytes::generate()).unwrap();
        let result = MasterKeyManager::load_or_init_at(
            &t.path,
            &t.current,
            &t.legacy,
            &t.data_dir,
            Ok(None),
            |_| Ok(()),
            |p, b| {
                fs::write(p, &blob).unwrap();
                create_private_file_new(p, b)
            },
        );
        assert_eq!(result.unwrap().master_key, other);
        assert_eq!(fs::read(&t.path).unwrap(), blob);
    }

    #[test]
    fn init_keyring_error_with_store_database_fails_closed() {
        let t = Init::new();
        fs::write(t.data_dir.join("vec-store.sqlite3"), b"db").unwrap();
        let (result, _, created) = t.run(keyring_err());
        assert!(result.is_err());
        assert_eq!(created, 0);
        assert!(!t.path.exists());
    }

    #[test]
    fn init_keyring_error_with_unlisted_data_dir_state_fails_closed() {
        let t = Init::new();
        // An unreadable data dir cannot prove it is empty.
        fs::remove_dir_all(&t.data_dir).unwrap();
        fs::write(&t.data_dir, b"not a dir").unwrap();
        let (result, _, created) = t.run(keyring_err());
        assert!(result.is_err());
        assert_eq!(created, 0);
    }

    #[test]
    fn init_keyring_hit_without_file_creates_nothing() {
        let t = Init::new();
        let key = [0x11u8; MASTER_KEY_LEN];
        let (result, saved, created) = t.run(Ok(Some(key)));
        assert_eq!(result.unwrap().master_key, key);
        assert_eq!(created, 0);
        assert!(saved.is_none());
        assert!(!t.path.exists());
    }

    #[test]
    fn init_keyring_miss_adopts_decryptable_file() {
        let t = Init::new();
        let key = [0x22u8; MASTER_KEY_LEN];
        let blob = t.write_file(&key);
        let (result, saved, created) = t.run(Ok(None));
        assert_eq!(result.unwrap().master_key, key);
        assert_eq!(saved, Some(key));
        assert_eq!(created, 0);
        assert_eq!(fs::read(&t.path).unwrap(), blob);
    }

    #[test]
    fn init_keyring_miss_undecryptable_file_fails_untouched() {
        let t = Init::new();
        let blob = t.write_foreign_file();
        let (result, saved, created) = t.run(Ok(None));
        assert!(result.is_err());
        assert!(saved.is_none());
        assert_eq!(created, 0);
        assert_eq!(fs::read(&t.path).unwrap(), blob);
    }

    #[test]
    fn init_keyring_miss_undecryptable_file_with_keyring_error_fails_untouched() {
        let t = Init::new();
        let blob = t.write_foreign_file();
        let (result, _, created) = t.run(keyring_err());
        assert!(result.is_err());
        assert_eq!(created, 0);
        assert_eq!(fs::read(&t.path).unwrap(), blob);
    }

    #[test]
    fn init_keyring_and_file_mismatch_fails_untouched() {
        let t = Init::new();
        let blob = t.write_file(&[0x33u8; MASTER_KEY_LEN]);
        let (result, saved, created) = t.run(Ok(Some([0x44u8; MASTER_KEY_LEN])));
        assert!(result.is_err());
        assert!(saved.is_none());
        assert_eq!(created, 0);
        assert_eq!(fs::read(&t.path).unwrap(), blob);
    }

    #[test]
    fn init_keyring_hit_with_undecryptable_file_uses_keyring() {
        let t = Init::new();
        let blob = t.write_foreign_file();
        let key = [0x55u8; MASTER_KEY_LEN];
        let (result, saved, created) = t.run(Ok(Some(key)));
        assert_eq!(result.unwrap().master_key, key);
        assert!(saved.is_none());
        assert_eq!(created, 0);
        assert_eq!(fs::read(&t.path).unwrap(), blob);
    }

    #[test]
    fn init_path_that_is_a_directory_fails_untouched() {
        let t = Init::new();
        fs::create_dir_all(&t.path).unwrap();
        let (result, saved, created) = t.run(Ok(None));
        assert!(result.is_err());
        assert!(saved.is_none());
        assert_eq!(created, 0);
        assert!(t.path.is_dir());
    }

    #[test]
    fn init_lost_create_race_adopts_other_writers_key() {
        let t = Init::new();
        let other = [0x66u8; MASTER_KEY_LEN];
        let blob = aes_encrypt(&other, &t.current, &NonceBytes::generate()).unwrap();
        let saved = std::cell::Cell::new(None);
        let result = MasterKeyManager::load_or_init_at(
            &t.path,
            &t.current,
            &t.legacy,
            &t.data_dir,
            Ok(None),
            |k| {
                saved.set(Some(*k));
                Ok(())
            },
            |p, _| {
                fs::write(p, &blob).unwrap();
                Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
            },
        );
        assert_eq!(result.unwrap().master_key, other);
        assert_eq!(saved.get(), Some(other));
        assert_eq!(fs::read(&t.path).unwrap(), blob);
    }

    #[test]
    fn init_legacy_file_is_rewrapped_with_same_key() {
        let t = Init::new();
        let key = [0x77u8; MASTER_KEY_LEN];
        fs::create_dir_all(t.path.parent().unwrap()).unwrap();
        let legacy_blob = aes_encrypt(&key, &t.legacy, &NonceBytes::generate()).unwrap();
        fs::write(&t.path, &legacy_blob).unwrap();
        let (result, _, _) = t.run(Ok(None));
        assert_eq!(result.unwrap().master_key, key);
        let on_disk = fs::read(&t.path).unwrap();
        assert_eq!(aes_decrypt(&on_disk, &t.current).unwrap(), key.to_vec());
    }
}
