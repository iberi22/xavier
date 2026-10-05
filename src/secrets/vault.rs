//! Hardware-backed secret vault using the system keyring.
//!
//! Provides secure storage for sensitive tokens (XAVIER_TOKEN, API keys)
//! using DPAPI/Credential Manager on Windows and Keychain on macOS.
//!
//! Falls back to local encrypted file vault when the system keyring
//! is unavailable (e.g., non-interactive Windows sessions, CI, etc.).

use std::sync::OnceLock;

use crate::crypto::encryption::{aes_decrypt, aes_encrypt, NonceBytes};
use crate::keystore::{ensure_private_dir, write_private_file, MasterKeyManager};
use crate::secrets::{SecretError, SecretResult};
use keyring::Entry;

/// Fallback vault storage, lazily initialized per namespace.
struct VaultBackend {
    storage_dir: std::path::PathBuf,
    vault_key: [u8; 32],
}

/// Backend shared by every service except the isolated ones (legacy layout:
/// `~/.xavier/secrets`, key derived without the service name).
static BACKEND: OnceLock<Option<VaultBackend>> = OnceLock::new();
/// Isolated backend of the Clavis provider-key vault.
static CLAVIS_BACKEND: OnceLock<Option<VaultBackend>> = OnceLock::new();

/// Services with their own storage directory and a service-bound derived key.
/// Existing services keep the legacy layout so stored secrets stay readable.
fn is_isolated_service(service_name: &str) -> bool {
    service_name == crate::clavis::CLAVIS_VAULT_SERVICE
}

fn build_backend(isolated_service: Option<&str>) -> Option<VaultBackend> {
    // Initialize master key (handles keyring + file fallback internally)
    match MasterKeyManager::load_or_init() {
        Ok(mkm) => {
            let home = match dirs::home_dir() {
                Some(h) => h,
                None => {
                    tracing::warn!("HardwareVault: no home dir, fallback vault unavailable");
                    return None;
                }
            };
            let storage_dir = match isolated_service {
                Some(service) => home.join(".xavier").join("vaults").join(service),
                None => home.join(".xavier").join("secrets"),
            };
            if let Err(e) = ensure_private_dir(&storage_dir) {
                tracing::warn!("HardwareVault: cannot create secrets dir: {e}");
                return None;
            }
            #[cfg(unix)]
            if let Some(parent) = storage_dir.parent() {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
            }
            // Derive a vault-specific key from the master key
            use sha2::{Digest, Sha256};
            let vault_key: [u8; 32] = {
                let mut hasher = Sha256::new();
                hasher.update(b"xavier-hardware-vault-fallback-v1");
                if let Some(service) = isolated_service {
                    // Namespace separator: an isolated vault key never equals
                    // the global one, even over the same directory.
                    hasher.update(b"|namespace|");
                    hasher.update(service.as_bytes());
                    hasher.update(b"|");
                }
                hasher.update(mkm.vault_key().unwrap_or([0u8; 32]));
                hasher.finalize().into()
            };
            Some(VaultBackend {
                storage_dir,
                vault_key,
            })
        }
        Err(e) => {
            tracing::warn!("HardwareVault: master key init failed: {e}");
            None
        }
    }
}

fn init_backend(service_name: &str) -> &'static Option<VaultBackend> {
    if is_isolated_service(service_name) {
        CLAVIS_BACKEND.get_or_init(|| build_backend(Some(service_name)))
    } else {
        BACKEND.get_or_init(|| build_backend(None))
    }
}

pub struct HardwareVault {
    service_name: String,
    #[cfg(test)]
    custom_storage_dir: Option<std::path::PathBuf>,
    #[cfg(test)]
    custom_vault_key: Option<[u8; 32]>,
    #[cfg(test)]
    keyring_stub_ok: Option<bool>,
}

impl HardwareVault {
    /// New.
    pub fn new(service_name: &str) -> Self {
        Self {
            service_name: service_name.to_string(),
            #[cfg(test)]
            custom_storage_dir: None,
            #[cfg(test)]
            custom_vault_key: None,
            #[cfg(test)]
            keyring_stub_ok: None,
        }
    }

    /// Fully isolate a vault instance from the real `~/.xavier` tree: injected
    /// storage dir and key mean `backend()` is never reached, so
    /// `MasterKeyManager::load_or_init()` never reads or creates the real
    /// `master.key`.
    #[cfg(test)]
    pub(crate) fn isolated(mut self, storage: std::path::PathBuf, key: [u8; 32]) -> Self {
        self.custom_storage_dir = Some(storage);
        self.custom_vault_key = Some(key);
        self.keyring_stub_ok = Some(false);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_keyring_stub(mut self, succeed: bool) -> Self {
        self.keyring_stub_ok = Some(succeed);
        self
    }

    /// Store secret. Writes the fallback file ONLY when try_keyring_store failed.
    pub fn store_secret(&self, key: &str, value: &str) -> SecretResult<()> {
        // Try keyring first; in non-interactive/headless environments (e.g. macOS CI, SSH remotes),
        // OS Keychain access may return permission denied or be unavailable.
        match self.try_keyring_store(key, value) {
            Ok(()) => Ok(()),
            Err(e) => {
                tracing::debug!(
                    service = %self.service_name,
                    key = %key,
                    error = %e,
                    "OS Keychain / hardware keystore unavailable or permission denied, using local encrypted fallback vault"
                );
                self.try_fallback_store(key, value)
            }
        }
    }

    /// Get secret. The local fallback only ever reads this vault's own
    /// namespace directory; it never falls through to another service's store.
    pub fn get_secret(&self, key: &str) -> SecretResult<String> {
        // Try keyring first
        match self.try_keyring_get(key) {
            Ok(val) => return Ok(val),
            Err(e) => {
                tracing::debug!(
                    service = %self.service_name,
                    key = %key,
                    error = %e,
                    "OS Keychain / hardware keystore get failed, attempting local encrypted fallback vault"
                );
            }
        }
        // Fallback to local encrypted vault
        self.try_fallback_get(key)
    }

    /// Delete secret.
    pub fn delete_secret(&self, key: &str) -> SecretResult<()> {
        let keyring_res = self.try_keyring_delete(key);
        if let Err(ref e) = keyring_res {
            tracing::debug!(
                service = %self.service_name,
                key = %key,
                error = %e,
                "OS Keychain / hardware keystore delete failed, attempting fallback vault deletion"
            );
        }
        let keyring_ok = keyring_res.is_ok();
        let fallback_ok = self.try_fallback_delete(key).is_ok();
        if keyring_ok || fallback_ok {
            Ok(())
        } else {
            Err(SecretError::NotFound(key.to_string()))
        }
    }

    // --- keyring helpers ---

    fn try_keyring_store(&self, key: &str, value: &str) -> SecretResult<()> {
        #[cfg(test)]
        if let Some(stub_ok) = self.keyring_stub_ok {
            return if stub_ok {
                Ok(())
            } else {
                Err(SecretError::ProviderError("Keyring stub error".to_string()))
            };
        }
        let entry = Entry::new(&self.service_name, key)
            .map_err(|e| SecretError::ProviderError(format!("Keyring error: {}", e)))?;
        entry
            .set_password(value)
            .map_err(|e| SecretError::ProviderError(format!("Failed to store secret: {}", e)))?;
        Ok(())
    }

    fn try_keyring_get(&self, key: &str) -> SecretResult<String> {
        #[cfg(test)]
        if let Some(false) = self.keyring_stub_ok {
            return Err(SecretError::NotFound(key.to_string()));
        }
        let entry = Entry::new(&self.service_name, key)
            .map_err(|e| SecretError::ProviderError(format!("Keyring error: {}", e)))?;
        entry.get_password().map_err(|e| match e {
            keyring::Error::NoEntry => SecretError::NotFound(key.to_string()),
            _ => SecretError::ProviderError(format!("Failed to retrieve secret: {}", e)),
        })
    }

    fn try_keyring_delete(&self, key: &str) -> SecretResult<()> {
        #[cfg(test)]
        if let Some(false) = self.keyring_stub_ok {
            return Err(SecretError::NotFound(key.to_string()));
        }
        let entry = Entry::new(&self.service_name, key)
            .map_err(|e| SecretError::ProviderError(format!("Keyring error: {}", e)))?;
        entry.delete_credential().map_err(|e| match e {
            keyring::Error::NoEntry => SecretError::NotFound(key.to_string()),
            _ => SecretError::ProviderError(format!("Failed to delete secret: {}", e)),
        })?;
        Ok(())
    }

    // --- fallback vault helpers ---

    fn backend(&self) -> SecretResult<&'static VaultBackend> {
        init_backend(&self.service_name).as_ref().ok_or_else(|| {
            SecretError::ProviderError("Fallback vault unavailable (no master key)".to_string())
        })
    }

    fn storage_dir(&self) -> SecretResult<std::path::PathBuf> {
        #[cfg(test)]
        if let Some(ref dir) = self.custom_storage_dir {
            return Ok(dir.clone());
        }

        let mut base = self.backend()?.storage_dir.clone();
        if self.service_name != "xavier" {
            base.push(&self.service_name);
            if !base.exists() {
                let _ = std::fs::create_dir_all(&base);
            }
        }
        Ok(base)
    }

    /// Vault key for the local fallback files. Tests inject a synthetic key so
    /// the real master key file is never touched.
    fn fallback_key(&self) -> SecretResult<[u8; 32]> {
        #[cfg(test)]
        if let Some(key) = self.custom_vault_key {
            return Ok(key);
        }

        Ok(self.backend()?.vault_key)
    }

    fn try_fallback_store(&self, key: &str, value: &str) -> SecretResult<()> {
        let storage_dir = self.storage_dir()?;
        ensure_private_dir(&storage_dir)
            .map_err(|e| SecretError::ProviderError(format!("Cannot create dir: {e}")))?;

        let vault_key = self.fallback_key()?;
        let path = storage_dir.join(format!("{}.enc", key));
        let nonce = NonceBytes::generate();
        let encrypted = aes_encrypt(value.as_bytes(), &vault_key, &nonce)
            .map_err(|e| SecretError::ProviderError(format!("Encryption failed: {e}")))?;
        write_private_file(&path, &encrypted)
            .map_err(|e| SecretError::ProviderError(format!("Fallback write failed: {e}")))?;
        Ok(())
    }

    fn try_fallback_get(&self, key: &str) -> SecretResult<String> {
        let storage_dir = self.storage_dir()?;
        let path = storage_dir.join(format!("{}.enc", key));
        if !path.exists() {
            return Err(SecretError::NotFound(key.to_string()));
        }
        let vault_key = self.fallback_key()?;
        let encrypted_data =
            std::fs::read(&path).map_err(|_| SecretError::NotFound(key.to_string()))?;
        let mut decrypted = aes_decrypt(&encrypted_data, &vault_key)
            .map_err(|e| SecretError::ProviderError(format!("Decryption failed: {e}")))?;
        let result = String::from_utf8(decrypted.clone())
            .map_err(|_| SecretError::ProviderError("Secret contains invalid UTF-8".to_string()));
        // Zeroize plaintext decrypted memory buffer to prevent secret leaks
        for byte in decrypted.iter_mut() {
            *byte = 0;
        }
        result
    }

    fn try_fallback_delete(&self, key: &str) -> SecretResult<()> {
        let storage_dir = self.storage_dir()?;
        let path = storage_dir.join(format!("{}.enc", key));
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| SecretError::ProviderError(format!("Fallback delete failed: {e}")))?;
            Ok(())
        } else {
            Err(SecretError::NotFound(key.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_clavis_service_is_isolated() {
        assert!(is_isolated_service(crate::clavis::CLAVIS_VAULT_SERVICE));
        for other in ["xavier", "xavier-auth2", "x"] {
            assert!(!is_isolated_service(other), "{other}");
        }
    }

    #[test]
    #[ignore = "Requires interactive keyring access"]
    fn test_hardware_vault_ops() {
        let vault = HardwareVault::new("xavier-test-vault");
        let key = "test-token";
        let value = "super-secret-value";

        vault.store_secret(key, value).unwrap();
        assert_eq!(vault.get_secret(key).unwrap(), value);
        vault.delete_secret(key).unwrap();
        assert!(vault.get_secret(key).is_err());
    }

    #[test]
    fn test_hardware_vault_headless_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = HardwareVault::new("xavier-test-headless-vault")
            .isolated(tmp.path().join("secrets"), [7u8; 32]);
        let key = "headless-test-secret-key";
        let value = "headless-secret-value-12345";

        // Store secret (uses fallback store when keyring unavailable or also populates fallback)
        vault
            .try_fallback_store(key, value)
            .expect("Fallback store failed");

        // Verify fallback get
        let retrieved = vault.try_fallback_get(key).expect("Fallback get failed");
        assert_eq!(retrieved, value);

        // Verify public get_secret works via keyring or fallback
        let retrieved_public = vault.get_secret(key).expect("get_secret failed");
        assert_eq!(retrieved_public, value);

        // Clean up secret
        vault.delete_secret(key).expect("delete_secret failed");
        assert!(vault.get_secret(key).is_err());
    }

    #[test]
    fn test_fallback_store_writes_0600() {
        let tmp = tempfile::tempdir().unwrap();
        let secrets_dir = tmp.path().join("secrets");
        let vault =
            HardwareVault::new("xavier-test-0600").isolated(secrets_dir.clone(), [11u8; 32]);
        let key = "test-key-0600";
        let val = "test-value-0600";

        vault.try_fallback_store(key, val).unwrap();

        let enc_path = secrets_dir.join(format!("{key}.enc"));
        assert!(enc_path.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&enc_path).unwrap();
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
            let parent_meta = std::fs::metadata(&secrets_dir).unwrap();
            assert_eq!(parent_meta.permissions().mode() & 0o777, 0o700);
        }
    }

    #[test]
    fn test_fallback_dir_is_0700() {
        let tmp = tempfile::tempdir().unwrap();
        let secrets_dir = tmp.path().join("secrets");
        let vault =
            HardwareVault::new("xavier-test-0700").isolated(secrets_dir.clone(), [13u8; 32]);
        let key = "test-key-0700";
        let val = "test-value-0700";

        vault.try_fallback_store(key, val).unwrap();

        let enc_path = secrets_dir.join(format!("{key}.enc"));
        assert!(enc_path.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&enc_path).unwrap();
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
            let parent_meta = std::fs::metadata(&secrets_dir).unwrap();
            assert_eq!(parent_meta.permissions().mode() & 0o777, 0o700);
        }
    }

    #[test]
    fn test_no_fallback_write_when_keyring_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let secrets_dir = tmp.path().join("secrets");
        let vault = HardwareVault::new("xavier-test-no-shadow")
            .isolated(secrets_dir.clone(), [17u8; 32])
            .with_keyring_stub(true);
        let key = "test-key-no-shadow";
        let val = "test-value-no-shadow";

        vault.store_secret(key, val).unwrap();

        let enc_path = secrets_dir.join(format!("{key}.enc"));
        assert!(!enc_path.exists());

        if secrets_dir.exists() {
            let enc_count = std::fs::read_dir(&secrets_dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("enc"))
                .count();
            assert_eq!(enc_count, 0);
        }
    }

    /// A pre-existing `.enc` file left at `0644` (or any other mode) must be
    /// tightened to `0600` by the fallback writer, since `OpenOptions::mode`
    /// only applies at creation time.
    #[test]
    fn test_fallback_store_tightens_preexisting_file() {
        let tmp = tempfile::tempdir().unwrap();
        let secrets_dir = tmp.path().join("secrets");
        let vault =
            HardwareVault::new("xavier-test-preexisting").isolated(secrets_dir.clone(), [19u8; 32]);
        let key = "test-key-preexisting";
        let val = "test-value-preexisting";

        std::fs::create_dir_all(&secrets_dir).unwrap();
        let enc_path = secrets_dir.join(format!("{key}.enc"));
        std::fs::write(&enc_path, b"stale").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&enc_path, std::fs::Permissions::from_mode(0o644)).unwrap();
        }

        vault.try_fallback_store(key, val).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&enc_path).unwrap();
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        }
        assert_eq!(vault.try_fallback_get(key).unwrap(), val);
    }
}
