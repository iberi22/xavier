//! Isolated HardwareVault implementation for testing.
//!
//! Provides a test-friendly wrapper around local encrypted storage without requiring
//! OS Keyring / D-Bus access or touching real user home directories.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;

use crate::secrets::vault::HardwareVault;

/// Isolated vault for testing without keyring dependencies.
pub struct IsolatedHardwareVault {
    vault: HardwareVault,
    _temp_dir: Arc<TempDir>,
}

impl IsolatedHardwareVault {
    /// Creates a new isolated hardware vault instance.
    pub fn new(service_name: &str) -> (HermeticTestEnv, HardwareVault) {
        let env = HermeticTestEnv::new();
        let vault = env.create_vault(service_name);
        (env, vault)
    }
}

/// RAII environment guard that redirects `HOME` and `XAVIER_DATA_DIR` to a temporary
/// directory, strips D-Bus session address to bypass host Keyring queries, and purges
/// state on drop.
pub struct HermeticTestEnv {
    temp_dir: Arc<TempDir>,
}

impl HermeticTestEnv {
    /// Creates a new isolated environment.
    pub fn new() -> Self {
        let temp_dir = tempfile::tempdir().expect("create hermetic tempdir");
        let path = temp_dir.path().to_path_buf();

        std::env::set_var("HOME", &path);
        std::env::set_var("XAVIER_DATA_DIR", &path);
        std::env::remove_var("DBUS_SESSION_BUS_ADDRESS");

        crate::test_support::isolate_user_dirs();

        Self {
            temp_dir: Arc::new(temp_dir),
        }
    }

    /// Access the underlying temporary directory path.
    pub fn path(&self) -> &Path {
        self.temp_dir.path()
    }

    /// Construct a new `HardwareVault` bound to this isolated environment.
    pub fn create_vault(&self, service_name: &str) -> HardwareVault {
        HardwareVault::new(service_name)
    }

    /// Construct an `Arc<HardwareVault>` bound to this isolated environment.
    pub fn create_arc_vault(&self, service_name: &str) -> Arc<HardwareVault> {
        Arc::new(self.create_vault(service_name))
    }
}

impl Default for HermeticTestEnv {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII Drop guard to purge sensitive keys from a `HardwareVault` upon completion or panic.
pub struct SecretKeyDropGuard {
    vault: HardwareVault,
    keys: Vec<String>,
}

impl SecretKeyDropGuard {
    /// Create a drop guard for a single key.
    pub fn single(vault: HardwareVault, key: impl Into<String>) -> Self {
        Self {
            vault,
            keys: vec![key.into()],
        }
    }

    /// Create a drop guard for multiple keys.
    pub fn multi(vault: HardwareVault, keys: Vec<String>) -> Self {
        Self { vault, keys }
    }

    /// Register another key for cleanup on drop.
    pub fn add_key(&mut self, key: impl Into<String>) {
        self.keys.push(key.into());
    }
}

impl Drop for SecretKeyDropGuard {
    fn drop(&mut self) {
        for key in &self.keys {
            let _ = self.vault.delete_secret(key);
        }
    }
}

/// Create an isolated vault instance for integration tests.
pub fn isolated_vault(service_name: &str) -> (HermeticTestEnv, HardwareVault) {
    let env = HermeticTestEnv::new();
    let vault = env.create_vault(service_name);
    (env, vault)
}
