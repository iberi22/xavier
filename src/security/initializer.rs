// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Security Initializer for Xavier
//!
//! Orchestrates the first boot security setup, including Master Key generation,
//! RSA keypair creation, and encrypted database initialization.

use crate::codebase::connection_manager::ConnectionManager;
use crate::secrets::local_vault::LocalSecretsVault;
use crate::security::encryption_keys::MasterKeyManager;
use crate::security::rsa_keys::RsaKeypairManager;
use anyhow::Result;

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

        // 3. Initialize Local Secrets Vault
        let _vault = LocalSecretsVault::init_default(&master_mgr)?;
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
