//! Hardware-backed secret vault using the system keyring.
//!
//! Provides secure storage for sensitive tokens (XAVIER_TOKEN, API keys)
//! using DPAPI/Credential Manager on Windows and Keychain on macOS.

use crate::secrets::{SecretError, SecretResult};
use keyring::Entry;

pub struct HardwareVault {
    service_name: String,
}

impl HardwareVault {
    pub fn new(service_name: &str) -> Self {
        Self {
            service_name: service_name.to_string(),
        }
    }

    pub fn store_secret(&self, key: &str, value: &str) -> SecretResult<()> {
        let entry = Entry::new(&self.service_name, key)
            .map_err(|e| SecretError::ProviderError(format!("Keyring error: {}", e)))?;
        entry
            .set_password(value)
            .map_err(|e| SecretError::ProviderError(format!("Failed to store secret: {}", e)))?;
        Ok(())
    }

    pub fn get_secret(&self, key: &str) -> SecretResult<String> {
        let entry = Entry::new(&self.service_name, key)
            .map_err(|e| SecretError::ProviderError(format!("Keyring error: {}", e)))?;
        entry.get_password().map_err(|e| match e {
            keyring::Error::NoEntry => SecretError::NotFound(key.to_string()),
            _ => SecretError::ProviderError(format!("Failed to retrieve secret: {}", e)),
        })
    }

    pub fn delete_secret(&self, key: &str) -> SecretResult<()> {
        let entry = Entry::new(&self.service_name, key)
            .map_err(|e| SecretError::ProviderError(format!("Keyring error: {}", e)))?;
        entry.delete_credential().map_err(|e| match e {
            keyring::Error::NoEntry => SecretError::NotFound(key.to_string()),
            _ => SecretError::ProviderError(format!("Failed to delete secret: {}", e)),
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
