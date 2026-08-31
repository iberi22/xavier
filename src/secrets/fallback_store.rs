//! FallbackSecretStore — Local→OpenBao→Memory (WAVE-1.02)
//!
//! Tries SecretStore backends in order until one succeeds (get returns Ok).
//! Set/delete try all and succeed if any succeeds.

use crate::secrets::store::SecretStore;
use crate::secrets::{SecretError, SecretResult};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Fallback chain for secrets: LocalVault→OpenBao→Memory
pub struct FallbackSecretStore {
    stores: Vec<Arc<dyn SecretStore>>,
}

impl FallbackSecretStore {
    pub fn new(stores: Vec<Arc<dyn SecretStore>>) -> Self {
        Self { stores }
    }

    /// Build from env XAVIER_SECRET_FALLBACK (default: local,openbao,memory)
    pub fn from_env_with_stores(stores: Vec<Arc<dyn SecretStore>>) -> Self {
        Self::new(stores)
    }

    pub fn from_chain_str(chain: &str) -> Vec<String> {
        chain
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect()
    }

    pub fn chain_from_env() -> Vec<String> {
        let chain = std::env::var("XAVIER_SECRET_FALLBACK")
            .unwrap_or_else(|_| "local,openbao,memory".to_string());
        Self::from_chain_str(&chain)
    }
}

impl SecretStore for FallbackSecretStore {
    fn get<'a>(
        &'a self,
        key: &'a str,
    ) -> Pin<Box<dyn Future<Output = SecretResult<String>> + Send + 'a>> {
        Box::pin(async move {
            let mut last_err = None;
            for store in &self.stores {
                match store.get(key).await {
                    Ok(v) => return Ok(v),
                    Err(e) => last_err = Some(e),
                }
            }
            Err(last_err.unwrap_or(SecretError::NotFound(key.to_string())))
        })
    }

    fn set<'a>(
        &'a self,
        key: &'a str,
        value: &'a str,
    ) -> Pin<Box<dyn Future<Output = SecretResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let mut last_err = None;
            for store in &self.stores {
                match store.set(key, value).await {
                    Ok(()) => return Ok(()),
                    Err(e) => last_err = Some(e),
                }
            }
            Err(last_err.unwrap_or(SecretError::ProviderError(
                "no secret stores configured".into(),
            )))
        })
    }

    fn delete<'a>(
        &'a self,
        key: &'a str,
    ) -> Pin<Box<dyn Future<Output = SecretResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let mut last_err = None;
            let mut any_ok = false;
            for store in &self.stores {
                match store.delete(key).await {
                    Ok(()) => any_ok = true,
                    Err(e) => last_err = Some(e),
                }
            }
            if any_ok {
                Ok(())
            } else {
                Err(last_err.unwrap_or(SecretError::NotFound(key.to_string())))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::local::LocalSecretStore;

    #[tokio::test]
    async fn test_fallback_get_set() {
        let s1 = Arc::new(LocalSecretStore::new());
        let s2 = Arc::new(LocalSecretStore::new());
        let fb = FallbackSecretStore::new(vec![s1.clone(), s2.clone()]);
        // set via fallback goes to first
        fb.set("k1", "v1").await.unwrap();
        assert_eq!(fb.get("k1").await.unwrap(), "v1");
        // second store fallback when first empty
        let empty = Arc::new(LocalSecretStore::new());
        let fb2 = FallbackSecretStore::new(vec![empty, s1]);
        assert_eq!(fb2.get("k1").await.unwrap(), "v1");
    }

    #[test]
    fn test_chain_parse() {
        assert_eq!(
            FallbackSecretStore::from_chain_str("local,openbao,memory"),
            vec!["local", "openbao", "memory"]
        );
    }
}
