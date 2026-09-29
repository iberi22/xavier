//! Security Initializer for Xavier
//!
//! Orchestrates the first boot security setup, including Master Key generation,
//! RSA keypair creation, and encrypted database initialization.

use crate::codebase::connection_manager::ConnectionManager;
use crate::secrets::local_vault::LocalSecretsVault;
use crate::security::encryption_keys::MasterKeyManager;
use crate::security::rsa_keys::RsaKeypairManager;
use anyhow::Result;
use std::path::Path;

/// Handles the initial security setup of the system.
pub struct SecurityInitializer;

impl SecurityInitializer {
    /// Run the full security initialization process.
    pub async fn initialize() -> Result<()> {
        println!("Initializing Xavier security system...");

        // 1. Load or initialize Master Key
        let master_mgr = MasterKeyManager::load_or_init()?;
        println!("✅ Master Key initialized");

        // 2. Ensure RSA Keypair exists and is protected
        let rsa_mgr = RsaKeypairManager::init_default(&master_mgr)?;
        rsa_mgr.ensure_keypair()?;
        println!("✅ RSA Keypair secured");

        // 3. Initialize Local Secrets Vault, then use the instance to tighten
        // and verify the storage directory permissions instead of dropping it.
        let vault = LocalSecretsVault::init_default(&master_mgr)?;
        tighten_storage_dir_permissions(vault.storage_dir())?;
        println!("✅ Local Secrets Vault ready");

        // 4. Trigger auth database creation/encryption
        let cm = ConnectionManager::global();
        cm.connect("auth", "")?;

        cm.with_conn("auth", |conn| {
            conn.execute_batch("PRAGMA integrity_check;")?;
            Ok(())
        })
        .await?;
        println!("✅ Encrypted Auth Database initialized");

        println!("Xavier security system initialization complete.");
        Ok(())
    }
}

/// Force the secrets storage directory to mode `0700` and verify it held.
/// A verification mismatch logs a warning instead of failing: `LocalSecretsVault::
/// init_default` already guarantees `0700`, so this must never abort
/// `SecurityInitializer::initialize`. Factored out so it is directly testable
/// without the real keyring or the real `~/.xavier`.
fn tighten_storage_dir_permissions(dir: &Path) -> Result<()> {
    crate::security::encryption_keys::ensure_private_dir(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir)?.permissions().mode() & 0o777;
        if mode != 0o700 {
            tracing::warn!("secrets storage directory has insecure permissions: {mode:o}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    #[cfg(unix)]
    fn test_tighten_storage_dir_permissions_forces_0700() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempdir().unwrap();
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o755)).unwrap();

        let result = tighten_storage_dir_permissions(tmp.path());
        assert!(
            result.is_ok(),
            "a verification mismatch must warn, not abort initialize()"
        );

        let mode = std::fs::metadata(tmp.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "secrets storage directory must be 0700");
    }
}
