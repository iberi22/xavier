//! Secrets management module
//!
//! Aggregates and re-exports the sub-modules within this module,
//! providing the public API surface for module consumers.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("Secret not found: {0}")]
    NotFound(String),
    #[error("Provider error: {0}")]
    ProviderError(String),
    #[error("Approval denied for operation: {0}")]
    ApprovalDenied(String),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Database error: {0}")]
    DatabaseError(String),
}

pub type SecretResult<T> = Result<T, SecretError>;

#[derive(Clone, PartialEq, Eq)]
pub struct Secret {
    pub key: String,
    pub value: String,
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Secret")
            .field("key", &"[REDACTED]")
            .field("value", &"[REDACTED]")
            .finish()
    }
}

#[deprecated(note = "SecretsManager is deprecated. Use Clavis or another secure provider.")]
#[allow(deprecated)]
pub struct SecretsManager {
    store: std::collections::HashMap<String, String>,
}

#[allow(deprecated)]
impl Default for SecretsManager {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(deprecated)]
impl SecretsManager {
    pub fn new() -> Self {
        Self {
            store: std::collections::HashMap::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }

    pub fn store(&mut self, key: String, value: String) -> SecretResult<()> {
        self.store.insert(key, value);
        Ok(())
    }

    pub fn get(&self, key: &str) -> SecretResult<String> {
        self.store.get(key).cloned().ok_or_else(|| SecretError::NotFound(key.to_string()))
    }

    pub fn delete(&mut self, key: &str) -> SecretResult<()> {
        self.store.remove(key);
        Ok(())
    }

    pub fn exists(&self, key: &str) -> bool {
        self.store.contains_key(key)
    }
}

// Lending engine
pub mod audit;
pub mod lending;
pub mod local;
pub mod local_vault;
pub mod openbao;
pub mod store;
#[cfg(test)]
mod tests;
pub mod vault;
