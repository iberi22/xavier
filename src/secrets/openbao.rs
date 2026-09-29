//! OpenBao/Vault secrets backend
//!
//! `OpenBaoSecretStore` is a non-functional stub: every `SecretStore` method
//! unconditionally returns `SecretError::ProviderError`. It is not wired to a
//! real OpenBao/Vault server and must not be treated as an available backend.
//! `crate::secrets::vault::HardwareVault` is the actual vault-backed store.
use crate::secrets::store::SecretStore;
use crate::secrets::{SecretError, SecretResult};
use std::future::Future;
use std::pin::Pin;

#[deprecated(
    note = "non-functional stub, all methods return ProviderError; use crate::secrets::vault::HardwareVault instead"
)]
pub struct OpenBaoSecretStore {
    _address: String,
    _token: String,
}

#[allow(deprecated)]
impl OpenBaoSecretStore {
    /// New.
    pub fn new(address: &str, token: &str) -> Self {
        Self {
            _address: address.to_string(),
            _token: token.to_string(),
        }
    }
}

#[allow(deprecated)]
impl SecretStore for OpenBaoSecretStore {
    fn get<'a>(
        &'a self,
        _key: &'a str,
    ) -> Pin<Box<dyn Future<Output = SecretResult<String>> + Send + 'a>> {
        Box::pin(async {
            Err(SecretError::ProviderError(
                "OpenBao provider not yet fully implemented".to_string(),
            ))
        })
    }

    fn set<'a>(
        &'a self,
        _key: &'a str,
        _value: &'a str,
    ) -> Pin<Box<dyn Future<Output = SecretResult<()>> + Send + 'a>> {
        Box::pin(async {
            Err(SecretError::ProviderError(
                "OpenBao provider not yet fully implemented".to_string(),
            ))
        })
    }

    fn delete<'a>(
        &'a self,
        _key: &'a str,
    ) -> Pin<Box<dyn Future<Output = SecretResult<()>> + Send + 'a>> {
        Box::pin(async {
            Err(SecretError::ProviderError(
                "OpenBao provider not yet fully implemented".to_string(),
            ))
        })
    }
}
